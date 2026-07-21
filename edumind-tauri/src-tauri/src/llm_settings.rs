use std::{
    fs,
    net::IpAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use edumind::{
    config::{
        EduMindConfig,
        types::{ModelConfig, ModelProviderConfig, ModelProviderKind},
    },
    gateway::agent::SAFE_DESKTOP_TOOLS,
    security::secrets::KeyringSecretStore,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;

const PROVIDER_ID: &str = "desktop-llm";
const KEYCHAIN_SERVICE: &str = "edumind.desktop";
const KEYCHAIN_SECRET_NAME: &str = "llm-provider-api-key";
const SETTINGS_FILE_NAME: &str = "llm-provider.json";
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_MODEL: &str = "gpt-4o-mini";
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmProviderInput {
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedLlmProvider {
    pub base_url: String,
    pub model: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmProviderStatus {
    pub base_url: String,
    pub model: String,
    pub api_key_configured: bool,
    pub configured: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmProviderConnectionCheck {
    pub status: String,
    pub message: String,
}

pub fn normalize_input(
    input: LlmProviderInput,
) -> Result<(PersistedLlmProvider, Option<String>), String> {
    let profile = PersistedLlmProvider {
        base_url: normalize_base_url(&input.base_url)?,
        model: normalize_model(&input.model)?,
    };
    let api_key = input
        .api_key
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty());
    if let Some(api_key) = &api_key
        && !is_supported_api_key(api_key)
    {
        return Err("Enter an ASCII API key without control characters.".to_owned());
    }
    Ok((profile, api_key))
}

pub fn load_profile(data_dir: &Path) -> Result<Option<PersistedLlmProvider>, String> {
    let path = settings_path(data_dir);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path)
        .map_err(|error| format!("could not read LLM provider settings: {error}"))?;
    let profile = serde_json::from_slice::<PersistedLlmProvider>(&bytes)
        .map_err(|error| format!("could not read LLM provider settings: {error}"))?;
    Ok(Some(PersistedLlmProvider {
        base_url: normalize_base_url(&profile.base_url)?,
        model: normalize_model(&profile.model)?,
    }))
}

pub fn save_profile(data_dir: &Path, profile: &PersistedLlmProvider) -> Result<(), String> {
    fs::create_dir_all(data_dir)
        .map_err(|error| format!("could not create the settings directory: {error}"))?;
    let bytes = serde_json::to_vec_pretty(profile)
        .map_err(|error| format!("could not save LLM provider settings: {error}"))?;
    fs::write(settings_path(data_dir), bytes)
        .map_err(|error| format!("could not save LLM provider settings: {error}"))
}

pub fn status(data_dir: &Path) -> Result<LlmProviderStatus, String> {
    let profile = load_profile(data_dir)?;
    let api_key_configured = load_api_key()?.is_some_and(|key| !key.trim().is_empty());
    Ok(match profile {
        Some(profile) => LlmProviderStatus {
            base_url: profile.base_url,
            model: profile.model,
            api_key_configured,
            configured: true,
        },
        None => LlmProviderStatus {
            base_url: DEFAULT_BASE_URL.to_owned(),
            model: DEFAULT_MODEL.to_owned(),
            api_key_configured,
            configured: false,
        },
    })
}

pub async fn test_connection(data_dir: &Path) -> Result<LlmProviderConnectionCheck, String> {
    let Some(profile) = load_profile(data_dir)? else {
        return Ok(connection_check(
            "not_configured",
            "Save a provider before testing the connection.",
        ));
    };
    let api_key = load_api_key()?;
    let api_key_configured = api_key.as_deref().is_some_and(|key| !key.trim().is_empty());
    if let Some(api_key) = api_key.as_deref().filter(|key| !key.trim().is_empty())
        && !is_supported_api_key(api_key)
    {
        return Ok(connection_check(
            "invalid_key",
            "The stored API key contains unsupported characters. Re-enter the raw key in Administration.",
        ));
    }

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not prepare the provider connection test.".to_owned())?;
    let mut request = client
        .post(chat_completion_url(&profile.base_url)?)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body("{}");
    if let Some(api_key) = api_key.as_deref().filter(|key| !key.trim().is_empty()) {
        request = request.bearer_auth(api_key);
    }

    Ok(match request.send().await {
        Ok(response) => connection_from_status(response.status().as_u16(), api_key_configured),
        Err(error) if error.is_timeout() => connection_check(
            "timeout",
            "The provider did not respond in time. Check its availability and your network connection.",
        ),
        Err(error) if error.is_connect() => connection_check(
            "connection_failed",
            "EduMind could not establish a connection. Check the base URL, firewall, VPN/proxy, and network route.",
        ),
        Err(error) if error.is_builder() => connection_check(
            "request_invalid",
            "EduMind could not prepare the diagnostic request. Re-enter the raw API key in Administration.",
        ),
        Err(_) => connection_check(
            "request_failed",
            "EduMind could not complete the diagnostic request. Try again after checking the provider settings.",
        ),
    })
}

pub fn store_api_key(api_key: &str) -> Result<(), String> {
    let store = secret_store()?;
    store
        .set(KEYCHAIN_SECRET_NAME, api_key)
        .map_err(|error| format!("could not store the API key in the native keychain: {error}"))
}

pub fn clear_api_key() -> Result<bool, String> {
    let store = secret_store()?;
    store
        .delete(KEYCHAIN_SECRET_NAME)
        .map_err(|error| format!("could not clear the API key from the native keychain: {error}"))
}

pub fn load_api_key() -> Result<Option<String>, String> {
    let store = secret_store()?;
    let secret = store
        .get(KEYCHAIN_SECRET_NAME)
        .map_err(|error| format!("could not access the native keychain: {error}"))?;
    Ok(secret.map(|value| value.expose().to_owned()))
}

pub fn apply_profile(
    config: &mut EduMindConfig,
    profile: &PersistedLlmProvider,
    api_key: Option<String>,
) {
    config.models.default_provider = PROVIDER_ID.to_owned();
    config
        .models
        .providers
        .retain(|provider| provider.id != PROVIDER_ID);
    config.models.providers.push(ModelProviderConfig {
        id: PROVIDER_ID.to_owned(),
        kind: ModelProviderKind::OpenAiCompatible,
        base_url: Some(profile.base_url.clone()),
        api_key,
        models: vec![ModelConfig {
            id: profile.model.clone(),
            ..ModelConfig::default()
        }],
    });
    config.agents.defaults.default_model = format!("{PROVIDER_ID}/{}", profile.model);
    let default_agent_id = config.agents.defaults.default_agent.clone();
    if let Some(agent) = config
        .agents
        .list
        .iter_mut()
        .find(|agent| agent.id == default_agent_id)
    {
        for tool in SAFE_DESKTOP_TOOLS {
            if !agent.allowed_tools.iter().any(|allowed| allowed == tool) {
                agent.allowed_tools.push((*tool).to_owned());
            }
        }
    }
}

fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SETTINGS_FILE_NAME)
}

fn secret_store() -> Result<KeyringSecretStore, String> {
    KeyringSecretStore::new(KEYCHAIN_SERVICE)
        .map_err(|error| format!("could not initialize native keychain storage: {error}"))
}

fn chat_completion_url(base_url: &str) -> Result<Url, String> {
    let mut url =
        Url::parse(base_url).map_err(|_| "The saved LLM base URL is invalid.".to_owned())?;
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&format!("{path}/chat/completions"));
    Ok(url)
}

fn connection_from_status(status: u16, api_key_configured: bool) -> LlmProviderConnectionCheck {
    match status {
        200..=299 => connection_check(
            "ready",
            "The provider accepted the safe diagnostic request. Chat is ready.",
        ),
        400 | 422 => connection_check(
            "reachable",
            "The chat endpoint is reachable. It rejected the empty diagnostic request as expected; send a chat to confirm access.",
        ),
        401 | 403 if api_key_configured => connection_check(
            "auth_failed",
            "The provider is reachable but rejected the stored API key. Re-enter an active raw key in Administration.",
        ),
        401 | 403 => connection_check(
            "api_key_required",
            "The provider is reachable and requires an API key. Add one in Administration.",
        ),
        404 | 405 => connection_check(
            "endpoint_mismatch",
            "The provider host is reachable, but this base URL does not expose /chat/completions. Check the base URL, usually ending in /v1.",
        ),
        429 => connection_check(
            "rate_limited",
            "The provider is reachable but rate-limited this diagnostic. Wait briefly and try again.",
        ),
        500..=599 => connection_check(
            "provider_unavailable",
            format!("The provider is reachable but returned HTTP {status}. Try again later."),
        ),
        _ => connection_check(
            "provider_error",
            format!("The provider is reachable but returned HTTP {status}."),
        ),
    }
}

fn connection_check(
    status: impl Into<String>,
    message: impl Into<String>,
) -> LlmProviderConnectionCheck {
    LlmProviderConnectionCheck {
        status: status.into(),
        message: message.into(),
    }
}

fn normalize_base_url(value: &str) -> Result<String, String> {
    let mut url = Url::parse(value.trim()).map_err(|_| "The base URL is invalid.".to_owned())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("The base URL must use HTTP(S).".to_owned());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "The base URL must not contain credentials, query data, or fragments.".to_owned(),
        );
    }
    let host = url.host_str().unwrap_or_default();
    if url.scheme() == "http" && !is_loopback_host(host) {
        return Err("Remote providers must use an HTTPS base URL.".to_owned());
    }
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&path);
    Ok(url.to_string().trim_end_matches('/').to_owned())
}

fn normalize_model(value: &str) -> Result<String, String> {
    let model = value.trim();
    if model.is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
        return Err("Enter a valid model identifier.".to_owned());
    }
    Ok(model.to_owned())
}

fn is_supported_api_key(value: &str) -> bool {
    value.len() <= 8_192 && value.is_ascii() && !value.chars().any(char::is_control)
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::{LlmProviderInput, apply_profile, connection_from_status, normalize_input};
    use edumind::config::EduMindConfig;

    #[test]
    fn accepts_https_and_loopback_http_provider_urls() {
        let remote = normalize_input(LlmProviderInput {
            base_url: "https://api.openai.com/v1/".to_owned(),
            model: "gpt-4o-mini".to_owned(),
            api_key: None,
        })
        .unwrap();
        let local = normalize_input(LlmProviderInput {
            base_url: "http://127.0.0.1:11434/v1".to_owned(),
            model: "llama3.2".to_owned(),
            api_key: None,
        });

        assert_eq!(remote.0.base_url, "https://api.openai.com/v1");
        assert!(local.is_ok());
    }

    #[test]
    fn rejects_insecure_remote_provider_urls() {
        let result = normalize_input(LlmProviderInput {
            base_url: "http://example.com/v1".to_owned(),
            model: "example-model".to_owned(),
            api_key: None,
        });

        assert!(result.is_err());
    }

    #[test]
    fn rejects_non_ascii_api_keys() {
        let result = normalize_input(LlmProviderInput {
            base_url: "https://api.openai.com/v1".to_owned(),
            model: "gpt-4o-mini".to_owned(),
            api_key: Some("sk-example\u{200b}".to_owned()),
        });

        assert!(result.is_err());
    }

    #[test]
    fn reports_rejected_stored_keys_without_exposing_them() {
        let check = connection_from_status(401, true);

        assert_eq!(check.status, "auth_failed");
        assert!(!check.message.contains("sk-"));
    }

    #[test]
    fn applies_the_profile_without_exposing_the_key() {
        let (profile, _) = normalize_input(LlmProviderInput {
            base_url: "https://api.openai.com/v1".to_owned(),
            model: "gpt-4o-mini".to_owned(),
            api_key: None,
        })
        .unwrap();
        let mut config = EduMindConfig::default();

        apply_profile(&mut config, &profile, Some("secret".to_owned()));

        assert_eq!(config.models.default_provider, "desktop-llm");
        assert_eq!(
            config.agents.defaults.default_model,
            "desktop-llm/gpt-4o-mini"
        );
        assert_eq!(
            config.models.providers.last().unwrap().models[0].id,
            "gpt-4o-mini"
        );
    }
}
