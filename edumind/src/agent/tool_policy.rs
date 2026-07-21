use std::fmt;

use crate::config::types::{ToolProfile, ToolsConfig};

use super::{AgentProfile, ToolClass, ToolDef};

/// A specific reason a tool call was denied before execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolPolicyDenial {
    AgentDisabled,
    EmptyAllowlist,
    NotAllowed,
    ProfileRestricted,
}

impl fmt::Display for ToolPolicyDenial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::AgentDisabled => "the agent is disabled",
            Self::EmptyAllowlist => "the agent has an empty tool allow-list",
            Self::NotAllowed => "the tool is not allow-listed for this agent",
            Self::ProfileRestricted => "the active tool profile restricts this capability",
        };
        formatter.write_str(message)
    }
}

/// Result of evaluating an agent/tool pair against policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolPolicyDecision {
    Allowed,
    Denied(ToolPolicyDenial),
}

impl ToolPolicyDecision {
    /// Returns whether the request may proceed to rate limiting and execution.
    #[must_use]
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// Fail-closed tool authorization policy for every agent request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolPolicy {
    enforce_allowlist: bool,
    profile: ToolProfile,
}

impl ToolPolicy {
    /// Builds policy from the active tools configuration.
    #[must_use]
    pub fn from_config(config: &ToolsConfig) -> Self {
        Self {
            enforce_allowlist: config.enforce_allowlist,
            profile: config.profile,
        }
    }

    /// Checks an individual call. An empty allow-list always denies all tools.
    #[must_use]
    pub fn authorize(&self, agent: &AgentProfile, tool: &ToolDef) -> ToolPolicyDecision {
        if !agent.enabled {
            return ToolPolicyDecision::Denied(ToolPolicyDenial::AgentDisabled);
        }
        if agent.allowed_tools.is_empty() {
            return ToolPolicyDecision::Denied(ToolPolicyDenial::EmptyAllowlist);
        }
        if self.enforce_allowlist
            && !agent
                .allowed_tools
                .iter()
                .any(|allowed| allowed == &tool.name)
        {
            return ToolPolicyDecision::Denied(ToolPolicyDenial::NotAllowed);
        }
        if !self.profile_allows(tool.class) {
            return ToolPolicyDecision::Denied(ToolPolicyDenial::ProfileRestricted);
        }
        ToolPolicyDecision::Allowed
    }

    fn profile_allows(&self, class: ToolClass) -> bool {
        match self.profile {
            ToolProfile::Safe => class == ToolClass::Read,
            ToolProfile::Balanced => class != ToolClass::Execution,
            ToolProfile::Full => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ToolPolicy, ToolPolicyDecision, ToolPolicyDenial};
    use crate::{
        agent::{AgentRegistry, ToolRegistry},
        config::{EduMindConfig, types::ToolProfile},
    };

    #[test]
    fn empty_allowlists_deny_every_tool_even_without_enforcement() {
        let mut config = EduMindConfig::default();
        config.tools.enforce_allowlist = false;
        let agent = AgentRegistry::from_config(&config)
            .unwrap()
            .resolve(None)
            .unwrap();
        let tool = ToolRegistry::standard().get("memory_search").unwrap();

        assert_eq!(
            ToolPolicy::from_config(&config.tools).authorize(&agent, &tool),
            ToolPolicyDecision::Denied(ToolPolicyDenial::EmptyAllowlist)
        );
    }

    #[test]
    fn safe_profile_restricts_mutating_tools_after_allowlist_check() {
        let mut config = EduMindConfig::default();
        config.agents.list[0].allowed_tools = vec!["write".to_owned()];
        config.tools.profile = ToolProfile::Safe;
        let agent = AgentRegistry::from_config(&config)
            .unwrap()
            .resolve(None)
            .unwrap();
        let tool = ToolRegistry::standard().get("write").unwrap();

        assert_eq!(
            ToolPolicy::from_config(&config.tools).authorize(&agent, &tool),
            ToolPolicyDecision::Denied(ToolPolicyDenial::ProfileRestricted)
        );
    }

    #[test]
    fn allowlisted_reads_pass_the_safe_profile() {
        let mut config = EduMindConfig::default();
        config.agents.list[0].allowed_tools = vec!["memory_search".to_owned()];
        let agent = AgentRegistry::from_config(&config)
            .unwrap()
            .resolve(None)
            .unwrap();
        let tool = ToolRegistry::standard().get("memory_search").unwrap();

        assert!(
            ToolPolicy::from_config(&config.tools)
                .authorize(&agent, &tool)
                .is_allowed()
        );
    }
}
