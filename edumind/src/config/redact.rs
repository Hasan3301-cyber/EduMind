use serde_json::Value;

use crate::{config::types::EduMindConfig, infra::Result};

/// Produces a JSON-safe view of configuration without secret values.
pub fn redact_config(config: &EduMindConfig) -> Result<Value> {
    let mut value = serde_json::to_value(config)?;
    redact_value(&mut value);
    Ok(value)
}

/// Replaces values under recursively sensitive object keys with a stable marker.
pub fn redact_value(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(redact_value),
        Value::Object(values) => {
            for (key, value) in values {
                if is_sensitive_key(key) && !value.is_null() {
                    *value = Value::String("[REDACTED]".to_owned());
                } else {
                    redact_value(value);
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    ["api_key", "token", "password", "secret", "authorization"]
        .iter()
        .any(|marker| key.contains(marker))
}

#[cfg(test)]
mod tests {
    use crate::config::{
        redact_config,
        types::{AuthMode, EduMindConfig},
    };

    #[test]
    fn redacts_auth_and_provider_secrets() {
        let mut config = EduMindConfig::default();
        config.gateway.auth.mode = AuthMode::Token;
        config.gateway.auth.token = Some("gateway-secret".to_owned());
        config.models.providers[0].api_key = Some("provider-secret".to_owned());

        let redacted = redact_config(&config).unwrap();

        assert_eq!(redacted["gateway"]["auth"]["token"], "[REDACTED]");
        assert_eq!(redacted["models"]["providers"][0]["api_key"], "[REDACTED]");
        assert_eq!(redacted["meta"]["name"], "EduMind");
    }
}
