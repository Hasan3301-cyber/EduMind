use serde::{Deserialize, Serialize};

use crate::infra::{EduMindError, Result};

/// Route table loaded from `routing.yaml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoutingTable {
    pub default: RouteTarget,
    pub routes: Vec<RouteRule>,
}

impl RoutingTable {
    /// Validates route targets and selectors before the table becomes active.
    pub fn validate(&self) -> Result<()> {
        self.default.validate("default")?;
        for (index, rule) in self.routes.iter().enumerate() {
            if rule.channel.trim().is_empty() {
                return Err(EduMindError::Routing(format!(
                    "routes[{index}].channel must not be empty"
                )));
            }
            validate_optional_selector(&rule.account_id, index, "account_id")?;
            validate_optional_selector(&rule.peer_id, index, "peer_id")?;
            validate_optional_selector(&rule.guild_id, index, "guild_id")?;
            validate_optional_selector(&rule.team_id, index, "team_id")?;
            rule.target().validate(&format!("routes[{index}]"))?;
        }
        Ok(())
    }
}

fn validate_optional_selector(value: &Option<String>, index: usize, name: &str) -> Result<()> {
    if value
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(EduMindError::Routing(format!(
            "routes[{index}].{name} must not be blank when set"
        )));
    }
    Ok(())
}

/// A module and agent target selected for an incoming message.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RouteTarget {
    pub module_id: String,
    pub agent_id: String,
}

impl RouteTarget {
    fn validate(&self, name: &str) -> Result<()> {
        if self.module_id.trim().is_empty() || self.agent_id.trim().is_empty() {
            return Err(EduMindError::Routing(format!(
                "{name} requires non-empty module_id and agent_id"
            )));
        }
        Ok(())
    }
}

impl Default for RouteTarget {
    fn default() -> Self {
        Self {
            module_id: "student-os".to_owned(),
            agent_id: "master".to_owned(),
        }
    }
}

/// A route that matches a channel plus optional account and conversation selectors.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RouteRule {
    pub channel: String,
    #[serde(alias = "accountId")]
    pub account_id: Option<String>,
    #[serde(alias = "peerId")]
    pub peer_id: Option<String>,
    #[serde(alias = "guildId")]
    pub guild_id: Option<String>,
    #[serde(alias = "teamId")]
    pub team_id: Option<String>,
    #[serde(alias = "moduleId")]
    pub module_id: String,
    #[serde(alias = "agentId")]
    pub agent_id: String,
}

impl RouteRule {
    /// Returns whether all configured selectors match a request.
    #[must_use]
    pub fn matches(&self, request: &RouteRequest) -> bool {
        self.channel == request.channel
            && selector_matches(&self.account_id, &request.account_id)
            && selector_matches(&self.peer_id, &request.peer_id)
            && selector_matches(&self.guild_id, &request.guild_id)
            && selector_matches(&self.team_id, &request.team_id)
    }

    /// Returns a deterministic specificity score used to rank matching rules.
    #[must_use]
    pub fn specificity(&self) -> usize {
        1 + usize::from(self.account_id.is_some())
            + usize::from(self.peer_id.is_some())
            + usize::from(self.guild_id.is_some())
            + usize::from(self.team_id.is_some())
    }

    /// Materializes the target portion of the route rule.
    #[must_use]
    pub fn target(&self) -> RouteTarget {
        RouteTarget {
            module_id: self.module_id.clone(),
            agent_id: self.agent_id.clone(),
        }
    }
}

fn selector_matches(expected: &Option<String>, actual: &Option<String>) -> bool {
    expected
        .as_ref()
        .is_none_or(|expected| actual.as_ref() == Some(expected))
}

/// Identifies the source account and conversation being routed.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RouteRequest {
    pub channel: String,
    pub account_id: Option<String>,
    pub peer_id: Option<String>,
    pub guild_id: Option<String>,
    pub team_id: Option<String>,
}

/// The table result, including the stable session identity for this conversation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RouteResolution {
    pub target: RouteTarget,
    pub session_key: String,
    pub matched_rule_index: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::{RouteRule, RoutingTable};

    #[test]
    fn rejects_blank_route_selectors() {
        let mut table = RoutingTable::default();
        table.routes.push(RouteRule {
            channel: "desktop".to_owned(),
            account_id: Some(" ".to_owned()),
            ..RouteRule::default()
        });

        assert!(table.validate().is_err());
    }
}
