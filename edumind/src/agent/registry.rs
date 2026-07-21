use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::{
    config::{EduMindConfig, types::AgentConfig},
    infra::{EduMindError, Result},
};

/// Resolved, immutable agent settings used by the runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub model: Option<String>,
    pub workspace: Option<PathBuf>,
    pub system_prompt: String,
    pub allowed_channels: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub allowed_subagents: Vec<String>,
    pub identity: String,
    pub timeout_secs: u64,
    pub max_concurrent_runs: Option<u16>,
    pub max_spawn_depth: u8,
}

impl From<&AgentConfig> for AgentProfile {
    fn from(config: &AgentConfig) -> Self {
        Self {
            id: config.id.clone(),
            name: config.name.clone(),
            enabled: config.enabled,
            model: config.model.clone(),
            workspace: config.workspace.clone(),
            system_prompt: config.system_prompt.clone(),
            allowed_channels: config.allowed_channels.clone(),
            allowed_tools: config.allowed_tools.clone(),
            allowed_subagents: config.allowed_subagents.clone(),
            identity: config.identity.clone(),
            timeout_secs: config.timeout_secs,
            max_concurrent_runs: config.max_concurrent_runs,
            max_spawn_depth: config.max_spawn_depth,
        }
    }
}

/// Registry that resolves enabled agents from the active configuration.
#[derive(Clone, Debug)]
pub struct AgentRegistry {
    agents: BTreeMap<String, AgentProfile>,
    default_agent_id: String,
    default_max_concurrent_runs: u16,
}

impl AgentRegistry {
    /// Builds an agent registry after checking all configuration invariants.
    pub fn from_config(config: &EduMindConfig) -> Result<Self> {
        config.validate()?;
        let agents = config
            .agents
            .list
            .iter()
            .map(|agent| (agent.id.clone(), AgentProfile::from(agent)))
            .collect();
        Ok(Self {
            agents,
            default_agent_id: config.agents.defaults.default_agent.clone(),
            default_max_concurrent_runs: config.agents.defaults.max_concurrent_runs,
        })
    }

    /// Resolves a requested agent or the configured default and rejects disabled agents.
    pub fn resolve(&self, requested_id: Option<&str>) -> Result<AgentProfile> {
        let agent_id = requested_id.unwrap_or(&self.default_agent_id);
        let agent = self.agents.get(agent_id).cloned().ok_or_else(|| {
            EduMindError::Agent(format!("configured agent `{agent_id}` does not exist"))
        })?;
        if !agent.enabled {
            return Err(EduMindError::Agent(format!(
                "configured agent `{agent_id}` is disabled"
            )));
        }
        Ok(agent)
    }

    /// Returns an agent regardless of its enabled state for administrative checks.
    #[must_use]
    pub fn get(&self, agent_id: &str) -> Option<AgentProfile> {
        self.agents.get(agent_id).cloned()
    }

    /// Returns all configured agents in deterministic ID order.
    #[must_use]
    pub fn all(&self) -> Vec<AgentProfile> {
        self.agents.values().cloned().collect()
    }

    /// Returns the configured default agent identifier.
    #[must_use]
    pub fn default_agent_id(&self) -> &str {
        &self.default_agent_id
    }

    /// Resolves the maximum parallel runs allowed for an agent.
    #[must_use]
    pub fn max_concurrent_runs(&self, agent: &AgentProfile) -> u16 {
        agent
            .max_concurrent_runs
            .unwrap_or(self.default_max_concurrent_runs)
    }
}

/// Shared concurrency limiter for direct agent runs in one runtime instance.
#[derive(Clone)]
pub struct AgentRunLimiter {
    registry: AgentRegistry,
    active_runs: Arc<Mutex<HashMap<String, usize>>>,
}

impl AgentRunLimiter {
    /// Creates a limiter that shares run counts across its clones.
    #[must_use]
    pub fn new(registry: AgentRegistry) -> Self {
        Self {
            registry,
            active_runs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Reserves a direct-run slot until the returned permit is dropped.
    pub fn try_acquire(&self, agent: &AgentProfile) -> Result<AgentRunPermit> {
        let max_concurrent = usize::from(self.registry.max_concurrent_runs(agent));
        let mut active_runs = self.active_runs.lock().map_err(|error| {
            EduMindError::Agent(format!("agent concurrency lock failed: {error}"))
        })?;
        let active = active_runs.entry(agent.id.clone()).or_default();
        if *active >= max_concurrent {
            return Err(EduMindError::Agent(format!(
                "agent `{}` has reached its concurrent-run limit",
                agent.id
            )));
        }
        *active += 1;
        Ok(AgentRunPermit {
            agent_id: agent.id.clone(),
            active_runs: Arc::clone(&self.active_runs),
        })
    }

    /// Returns the active direct-run count for one agent.
    pub fn active_count(&self, agent_id: &str) -> Result<usize> {
        self.active_runs
            .lock()
            .map(|active_runs| active_runs.get(agent_id).copied().unwrap_or_default())
            .map_err(|error| EduMindError::Agent(format!("agent concurrency lock failed: {error}")))
    }
}

/// RAII reservation that releases an agent concurrency slot on drop.
pub struct AgentRunPermit {
    agent_id: String,
    active_runs: Arc<Mutex<HashMap<String, usize>>>,
}

impl Drop for AgentRunPermit {
    fn drop(&mut self) {
        let Ok(mut active_runs) = self.active_runs.lock() else {
            return;
        };
        let remove_entry = if let Some(active) = active_runs.get_mut(&self.agent_id) {
            *active = active.saturating_sub(1);
            *active == 0
        } else {
            false
        };
        if remove_entry {
            active_runs.remove(&self.agent_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentRegistry, AgentRunLimiter};
    use crate::config::EduMindConfig;

    #[test]
    fn resolves_the_configured_default_agent() {
        let registry = AgentRegistry::from_config(&EduMindConfig::default()).unwrap();

        let agent = registry.resolve(None).unwrap();

        assert_eq!(agent.id, "master");
        assert_eq!(agent.name, "Master Agent");
    }

    #[test]
    fn rejects_disabled_agents() {
        let mut config = EduMindConfig::default();
        config.agents.list[0].enabled = false;

        assert!(AgentRegistry::from_config(&config).is_err());
    }

    #[test]
    fn direct_run_limiter_releases_slots_with_its_permit() {
        let mut config = EduMindConfig::default();
        config.agents.defaults.max_concurrent_runs = 1;
        let registry = AgentRegistry::from_config(&config).unwrap();
        let agent = registry.resolve(None).unwrap();
        let limiter = AgentRunLimiter::new(registry);
        let permit = limiter.try_acquire(&agent).unwrap();

        assert!(limiter.try_acquire(&agent).is_err());
        drop(permit);
        assert!(limiter.try_acquire(&agent).is_ok());
    }
}
