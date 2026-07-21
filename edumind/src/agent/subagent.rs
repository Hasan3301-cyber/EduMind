use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::infra::{EduMindError, Result};

use super::AgentRegistry;

/// A scheduled subagent invocation that holds one target-agent concurrency slot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubagentTicket {
    pub id: Uuid,
    pub parent_agent_id: String,
    pub agent_id: String,
    pub session_key: String,
    pub depth: u8,
    pub created_at: DateTime<Utc>,
}

/// Enforces configured subagent relationships, spawn depth, and concurrency limits.
#[derive(Clone)]
pub struct SubagentRegistry {
    agents: AgentRegistry,
    active_runs: Arc<Mutex<HashMap<String, usize>>>,
}

impl SubagentRegistry {
    /// Creates a registry backed by the same resolved agent configuration as the runner.
    #[must_use]
    pub fn new(agents: AgentRegistry) -> Self {
        Self {
            agents,
            active_runs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Schedules an allowed subagent and reserves its concurrency slot.
    pub fn spawn(
        &self,
        parent_agent_id: &str,
        parent_depth: u8,
        agent_id: &str,
        session_key: &str,
        now: DateTime<Utc>,
    ) -> Result<SubagentTicket> {
        if session_key.trim().is_empty() {
            return Err(EduMindError::Agent(
                "subagent session key must not be empty".to_owned(),
            ));
        }
        let parent = self.agents.resolve(Some(parent_agent_id))?;
        if !parent
            .allowed_subagents
            .iter()
            .any(|allowed| allowed == agent_id)
        {
            return Err(EduMindError::Agent(format!(
                "agent `{parent_agent_id}` is not allowed to spawn `{agent_id}`"
            )));
        }
        let depth = parent_depth
            .checked_add(1)
            .ok_or_else(|| EduMindError::Agent("subagent spawn depth overflowed".to_owned()))?;
        if depth > parent.max_spawn_depth {
            return Err(EduMindError::Agent(format!(
                "agent `{parent_agent_id}` cannot spawn beyond depth {}",
                parent.max_spawn_depth
            )));
        }
        let target = self.agents.resolve(Some(agent_id))?;
        let max_concurrent = usize::from(self.agents.max_concurrent_runs(&target));
        let mut active_runs = self.active_runs.lock().map_err(|error| {
            EduMindError::Agent(format!("subagent registry lock failed: {error}"))
        })?;
        let active = active_runs.entry(target.id.clone()).or_default();
        if *active >= max_concurrent {
            return Err(EduMindError::Agent(format!(
                "agent `{agent_id}` has reached its concurrent-run limit"
            )));
        }
        *active += 1;
        Ok(SubagentTicket {
            id: Uuid::new_v4(),
            parent_agent_id: parent.id,
            agent_id: target.id,
            session_key: session_key.to_owned(),
            depth,
            created_at: now,
        })
    }

    /// Releases a completed or cancelled subagent ticket's concurrency slot.
    pub fn finish(&self, ticket: &SubagentTicket) -> Result<()> {
        let mut active_runs = self.active_runs.lock().map_err(|error| {
            EduMindError::Agent(format!("subagent registry lock failed: {error}"))
        })?;
        let remove_entry = if let Some(active) = active_runs.get_mut(&ticket.agent_id) {
            *active = active.saturating_sub(1);
            *active == 0
        } else {
            false
        };
        if remove_entry {
            active_runs.remove(&ticket.agent_id);
        }
        Ok(())
    }

    /// Returns the current active count for an agent.
    pub fn active_count(&self, agent_id: &str) -> Result<usize> {
        self.active_runs
            .lock()
            .map(|active_runs| active_runs.get(agent_id).copied().unwrap_or_default())
            .map_err(|error| EduMindError::Agent(format!("subagent registry lock failed: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::SubagentRegistry;
    use crate::{
        agent::AgentRegistry,
        config::{EduMindConfig, types::AgentConfig},
    };

    fn configured_registry() -> SubagentRegistry {
        let mut config = EduMindConfig::default();
        config.agents.list[0].allowed_subagents = vec!["researcher".to_owned()];
        let researcher = AgentConfig {
            id: "researcher".to_owned(),
            name: "Research Agent".to_owned(),
            max_concurrent_runs: Some(1),
            ..AgentConfig::default()
        };
        config.agents.list.push(researcher);
        SubagentRegistry::new(AgentRegistry::from_config(&config).unwrap())
    }

    #[test]
    fn enforces_subagent_allowlists_and_concurrency() {
        let registry = configured_registry();
        let now = Utc::now();
        let ticket = registry
            .spawn("master", 0, "researcher", "desktop:research", now)
            .unwrap();

        assert_eq!(registry.active_count("researcher").unwrap(), 1);
        assert!(
            registry
                .spawn("master", 0, "researcher", "desktop:research-2", now)
                .is_err()
        );

        registry.finish(&ticket).unwrap();
        assert_eq!(registry.active_count("researcher").unwrap(), 0);
    }

    #[test]
    fn rejects_unallowlisted_subagents() {
        let registry = configured_registry();

        assert!(
            registry
                .spawn("master", 0, "master", "desktop:research", Utc::now())
                .is_err()
        );
    }
}
