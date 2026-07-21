use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde_yaml::Value;

use crate::{
    config::types::EduMindConfig,
    infra::{EduMindError, Result},
};

/// Loads, expands, deserializes, and validates a YAML configuration file.
pub fn load_from_path(path: impl AsRef<Path>) -> Result<EduMindConfig> {
    let path = path.as_ref().to_path_buf();
    let contents = fs::read_to_string(&path).map_err(|source| EduMindError::ConfigIo {
        path: path.clone(),
        source,
    })?;
    load_from_str_at_path(&contents, path)
}

/// Parses configuration text using an in-memory path for diagnostic messages.
pub fn load_from_str(contents: &str) -> Result<EduMindConfig> {
    load_from_str_at_path(contents, PathBuf::from("<inline>"))
}

fn load_from_str_at_path(contents: &str, path: PathBuf) -> Result<EduMindConfig> {
    let mut value =
        serde_yaml::from_str::<Value>(contents).map_err(|source| EduMindError::ConfigParse {
            path: path.clone(),
            source,
        })?;
    expand_value(&mut value)?;
    let config = serde_yaml::from_value::<EduMindConfig>(value)
        .map_err(|source| EduMindError::ConfigParse { path, source })?;
    config.validate()?;
    Ok(config)
}

/// Expands `${VARIABLE}` occurrences while leaving unrelated text unchanged.
pub fn expand_environment(input: &str) -> Result<String> {
    let mut expanded = String::new();
    let mut remaining = input;

    while let Some(start) = remaining.find("${") {
        expanded.push_str(&remaining[..start]);
        let variable_start = start + 2;
        let Some(relative_end) = remaining[variable_start..].find('}') else {
            return Err(EduMindError::ConfigValidation(format!(
                "unterminated environment expression in `{input}`"
            )));
        };
        let variable_end = variable_start + relative_end;
        let variable = &remaining[variable_start..variable_end];
        if variable.is_empty() {
            return Err(EduMindError::ConfigValidation(
                "environment expression variable name must not be empty".to_owned(),
            ));
        }
        let replacement = env::var(variable)
            .map_err(|_| EduMindError::MissingEnvironmentVariable(variable.to_owned()))?;
        expanded.push_str(&replacement);
        remaining = &remaining[variable_end + 1..];
    }
    expanded.push_str(remaining);
    Ok(expanded)
}

fn expand_value(value: &mut Value) -> Result<()> {
    match value {
        Value::String(text) => {
            let expanded = expand_environment(text)?;
            *text = expand_home(&expanded)?;
        }
        Value::Sequence(values) => {
            for value in values {
                expand_value(value)?;
            }
        }
        Value::Mapping(values) => {
            for value in values.values_mut() {
                expand_value(value)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Tagged(_) => {}
    }
    Ok(())
}

fn expand_home(value: &str) -> Result<String> {
    if value != "~" && !value.starts_with("~/") && !value.starts_with("~\\") {
        return Ok(value.to_owned());
    }
    let home = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .ok_or(EduMindError::HomeDirectoryUnavailable)?;
    let suffix = value[1..].trim_start_matches(['/', '\\']);
    Ok(PathBuf::from(home)
        .join(suffix)
        .to_string_lossy()
        .into_owned())
}

#[cfg(test)]
mod tests {
    use serde_yaml::Value;

    use super::{expand_environment, load_from_str};

    #[test]
    fn expands_environment_values_before_deserializing() {
        unsafe {
            std::env::set_var("EDUMIND_CONFIG_TEST_TOKEN", "test-token");
        }
        let config = load_from_str(
            "gateway:\n  auth:\n    mode: token\n    token: ${EDUMIND_CONFIG_TEST_TOKEN}\n",
        )
        .unwrap();

        assert_eq!(config.gateway.auth.token.as_deref(), Some("test-token"));
        assert_eq!(
            expand_environment("prefix-${EDUMIND_CONFIG_TEST_TOKEN}").unwrap(),
            "prefix-test-token"
        );
    }

    #[test]
    fn parses_and_round_trips_the_documented_example() {
        let config = load_from_str(include_str!("../../config.example.yaml")).unwrap();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let decoded = load_from_str(&yaml).unwrap();

        assert_eq!(decoded, config);
    }

    #[test]
    fn documented_example_declares_explicit_safe_defaults() {
        let document =
            serde_yaml::from_str::<Value>(include_str!("../../config.example.yaml")).unwrap();

        assert_eq!(
            document["gateway"]["bind_address"].as_str(),
            Some("127.0.0.1")
        );
        assert_eq!(document["gateway"]["auth"]["mode"].as_str(), Some("none"));
        assert!(
            document["gateway"]["request_body_max_bytes"]
                .as_u64()
                .is_some_and(|value| value > 0)
        );
        assert_eq!(document["tools"]["profile"].as_str(), Some("safe"));
        assert_eq!(document["tools"]["enforce_allowlist"].as_bool(), Some(true));
        assert!(
            document["tools"]["rate_limit_per_minute"]
                .as_u64()
                .is_some_and(|value| value > 0)
        );
        assert!(
            document["tools"]["audit_log_capacity"]
                .as_u64()
                .is_some_and(|value| value > 0)
        );
        assert!(
            document["tools"]["max_tool_rounds"]
                .as_u64()
                .is_some_and(|value| value > 0)
        );
        assert_eq!(
            document["security"]["action_password_required"].as_bool(),
            Some(true)
        );
        assert_eq!(
            document["security"]["restrict_tool_writes"].as_bool(),
            Some(true)
        );
        assert!(
            document["security"]["allowed_tool_write_roots"]
                .as_sequence()
                .is_some_and(|roots| !roots.is_empty())
        );
        for key in [
            "max_total_per_day",
            "max_network_per_day",
            "max_execution_per_day",
        ] {
            assert!(
                document["security"]["tool_daily_limits"][key]
                    .as_u64()
                    .is_some_and(|value| value > 0),
                "security.tool_daily_limits.{key} must be explicit and positive"
            );
        }
        for key in [
            "max_tool_timeout_secs",
            "max_output_bytes",
            "max_write_bytes",
            "process_memory_limit_mb",
        ] {
            assert!(
                document["security"]["execution"][key]
                    .as_u64()
                    .is_some_and(|value| value > 0),
                "security.execution.{key} must be explicit and positive"
            );
        }
        assert_eq!(
            document["security"]["execution"]["windows_job_objects"].as_bool(),
            Some(true)
        );
    }
}
