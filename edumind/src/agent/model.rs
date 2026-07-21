use std::{collections::BTreeMap, fmt, str::FromStr};

use crate::{
    config::{
        EduMindConfig,
        types::{ModelProviderConfig, ModelProviderKind},
    },
    infra::{EduMindError, Result},
};

use super::{AgentProfile, AgentRegistry};

/// Canonical `provider/model` reference used for agent model resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelReference {
    pub provider_id: String,
    pub model_id: String,
}

impl fmt::Display for ModelReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.provider_id, self.model_id)
    }
}

impl FromStr for ModelReference {
    type Err = EduMindError;

    fn from_str(reference: &str) -> Result<Self> {
        let (provider_id, model_id) = reference.trim().split_once('/').ok_or_else(|| {
            EduMindError::Agent(format!(
                "model reference `{reference}` must use the provider/model format"
            ))
        })?;
        if provider_id.trim().is_empty() || model_id.trim().is_empty() {
            return Err(EduMindError::Agent(format!(
                "model reference `{reference}` must include provider and model IDs"
            )));
        }
        Ok(Self {
            provider_id: provider_id.trim().to_owned(),
            model_id: model_id.trim().to_owned(),
        })
    }
}

/// Non-secret model metadata selected for one agent turn.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedModel {
    pub reference: ModelReference,
    pub provider_kind: ModelProviderKind,
    pub base_url: Option<String>,
    pub context_window: u32,
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
}

/// Resolves request, agent, and default model settings without exposing provider secrets.
#[derive(Clone, Debug)]
pub struct ModelResolver {
    agents: AgentRegistry,
    providers: BTreeMap<String, ModelProviderConfig>,
    default_model: String,
}

impl ModelResolver {
    /// Builds a resolver from the validated active configuration.
    pub fn from_config(config: &EduMindConfig) -> Result<Self> {
        let agents = AgentRegistry::from_config(config)?;
        let providers = config
            .models
            .providers
            .iter()
            .cloned()
            .map(|provider| (provider.id.clone(), provider))
            .collect();
        Ok(Self {
            agents,
            providers,
            default_model: config.agents.defaults.default_model.clone(),
        })
    }

    /// Resolves a model for the selected agent, preferring request then agent then defaults.
    pub fn resolve_for_agent(
        &self,
        agent_id: Option<&str>,
        requested_model: Option<&str>,
    ) -> Result<ResolvedModel> {
        let agent = self.agents.resolve(agent_id)?;
        self.resolve(&agent, requested_model)
    }

    /// Resolves a model for an already selected agent profile.
    pub fn resolve(
        &self,
        agent: &AgentProfile,
        requested_model: Option<&str>,
    ) -> Result<ResolvedModel> {
        let reference = requested_model
            .filter(|value| !value.trim().is_empty())
            .or(agent.model.as_deref())
            .unwrap_or(&self.default_model)
            .parse::<ModelReference>()?;
        let provider = self.providers.get(&reference.provider_id).ok_or_else(|| {
            EduMindError::Agent(format!(
                "model provider `{}` is not configured",
                reference.provider_id
            ))
        })?;
        let model = provider
            .models
            .iter()
            .find(|candidate| candidate.id == reference.model_id)
            .ok_or_else(|| {
                EduMindError::Agent(format!(
                    "model `{}` is not configured for provider `{}`",
                    reference.model_id, reference.provider_id
                ))
            })?;
        Ok(ResolvedModel {
            reference,
            provider_kind: provider.kind,
            base_url: provider.base_url.clone(),
            context_window: model.context_window,
            input_cost_per_million: model.input_cost_per_million,
            output_cost_per_million: model.output_cost_per_million,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ModelReference, ModelResolver};
    use crate::config::EduMindConfig;

    #[test]
    fn parses_canonical_model_references() {
        let reference = "local/llama3.2".parse::<ModelReference>().unwrap();

        assert_eq!(reference.provider_id, "local");
        assert_eq!(reference.model_id, "llama3.2");
        assert_eq!(reference.to_string(), "local/llama3.2");
    }

    #[test]
    fn request_model_overrides_the_agent_default() {
        let config = EduMindConfig::default();
        let resolver = ModelResolver::from_config(&config).unwrap();

        let model = resolver
            .resolve_for_agent(None, Some("local/llama3.2"))
            .unwrap();

        assert_eq!(model.context_window, 32_768);
        assert_eq!(model.reference.to_string(), "local/llama3.2");
    }

    #[test]
    fn rejects_unknown_model_providers() {
        let resolver = ModelResolver::from_config(&EduMindConfig::default()).unwrap();

        assert!(
            resolver
                .resolve_for_agent(None, Some("missing/model"))
                .is_err()
        );
    }
}
