use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use crate::infra::{EduMindError, Result};

use super::{RouteRequest, RouteResolution, RoutingTable, session_key::stable_session_key};

/// Hot-reloadable route-table service with deterministic specificity matching.
#[derive(Clone, Debug)]
pub struct ModuleRouter {
    source_path: Option<PathBuf>,
    table: Arc<RwLock<RoutingTable>>,
}

impl ModuleRouter {
    /// Loads a validated route table from a YAML file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let source_path = path.as_ref().to_path_buf();
        let table = load_table(&source_path)?;
        Ok(Self {
            source_path: Some(source_path),
            table: Arc::new(RwLock::new(table)),
        })
    }

    /// Builds a router from an already materialized table, useful for tests and embeds.
    pub fn from_table(table: RoutingTable) -> Result<Self> {
        table.validate()?;
        Ok(Self {
            source_path: None,
            table: Arc::new(RwLock::new(table)),
        })
    }

    /// Resolves the highest-specificity matching route, falling back to `default`.
    pub fn resolve(&self, request: &RouteRequest) -> Result<RouteResolution> {
        let table = self
            .table
            .read()
            .map_err(|error| EduMindError::Routing(format!("route table lock failed: {error}")))?;

        let mut matched_rule_index = None;
        let mut target = table.default.clone();
        let mut best_specificity = 0;
        for (index, rule) in table.routes.iter().enumerate() {
            let specificity = rule.specificity();
            if rule.matches(request) && specificity > best_specificity {
                target = rule.target();
                matched_rule_index = Some(index);
                best_specificity = specificity;
            }
        }

        Ok(RouteResolution {
            target,
            session_key: stable_session_key(request),
            matched_rule_index,
        })
    }

    /// Atomically replaces the active table with a newly parsed file-backed table.
    pub fn reload(&self) -> Result<()> {
        let source_path = self.source_path.as_ref().ok_or_else(|| {
            EduMindError::Routing(
                "cannot reload a router created from an in-memory table".to_owned(),
            )
        })?;
        let next_table = load_table(source_path)?;
        let mut table = self
            .table
            .write()
            .map_err(|error| EduMindError::Routing(format!("route table lock failed: {error}")))?;
        *table = next_table;
        Ok(())
    }

    /// Returns a snapshot of the active routing table.
    pub fn table(&self) -> Result<RoutingTable> {
        self.table
            .read()
            .map(|table| table.clone())
            .map_err(|error| EduMindError::Routing(format!("route table lock failed: {error}")))
    }

    /// Returns the file reloaded by this router, if it has one.
    #[must_use]
    pub fn source_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }
}

fn load_table(path: &Path) -> Result<RoutingTable> {
    let contents = fs::read_to_string(path).map_err(|error| {
        EduMindError::Routing(format!(
            "failed to read routing table {}: {error}",
            path.display()
        ))
    })?;
    let table = serde_yaml::from_str::<RoutingTable>(&contents).map_err(|error| {
        EduMindError::Routing(format!(
            "failed to parse routing table {}: {error}",
            path.display()
        ))
    })?;
    table.validate()?;
    Ok(table)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::ModuleRouter;
    use crate::routing::{RouteRequest, RouteRule, RouteTarget, RoutingTable};

    #[test]
    fn chooses_the_most_specific_match_and_uses_first_rule_for_ties() {
        let router = ModuleRouter::from_table(RoutingTable {
            default: RouteTarget::default(),
            routes: vec![
                RouteRule {
                    channel: "desktop".to_owned(),
                    module_id: "general".to_owned(),
                    agent_id: "master".to_owned(),
                    ..RouteRule::default()
                },
                RouteRule {
                    channel: "desktop".to_owned(),
                    peer_id: Some("learner-1".to_owned()),
                    module_id: "personal".to_owned(),
                    agent_id: "master".to_owned(),
                    ..RouteRule::default()
                },
                RouteRule {
                    channel: "desktop".to_owned(),
                    peer_id: Some("learner-1".to_owned()),
                    module_id: "ignored-tie".to_owned(),
                    agent_id: "master".to_owned(),
                    ..RouteRule::default()
                },
            ],
        })
        .unwrap();

        let resolution = router
            .resolve(&RouteRequest {
                channel: "desktop".to_owned(),
                peer_id: Some("learner-1".to_owned()),
                ..RouteRequest::default()
            })
            .unwrap();

        assert_eq!(resolution.target.module_id, "personal");
        assert_eq!(resolution.matched_rule_index, Some(1));
    }

    #[test]
    fn invalid_reload_keeps_the_previous_active_table() {
        let path = std::env::temp_dir().join(format!(
            "edumind-routing-{}-{}.yaml",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(
            &path,
            "default:\n  module_id: initial\n  agent_id: master\nroutes: []\n",
        )
        .unwrap();
        let router = ModuleRouter::from_file(&path).unwrap();
        fs::write(&path, "default: [not valid\n").unwrap();

        assert!(router.reload().is_err());
        assert_eq!(router.table().unwrap().default.module_id, "initial");

        fs::remove_file(path).unwrap();
    }
}
