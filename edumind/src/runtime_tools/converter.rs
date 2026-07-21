use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;
use uuid::Uuid;

use crate::{
    config::types::{ExecutionCapsConfig, ExternalToolConfig},
    infra::{EduMindError, Result, run_blocking},
    security::WriteSandbox,
};

use super::{GuardedCommandSpec, python_executable, run_guarded_command};

/// Result of a converter command that wrote a verified artifact through the write sandbox.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExternalConversion {
    pub output_path: String,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

/// All staged input and destination details for one guarded converter execution.
pub struct ConverterRequest<'a> {
    pub tool_name: &'a str,
    pub config: &'a ExternalToolConfig,
    pub caps: &'a ExecutionCapsConfig,
    pub input_name: &'a str,
    pub input: Vec<u8>,
    pub format: &'a str,
    pub destination: &'a Path,
}

/// Runs an optional converter with a staged input and copies only its declared output to storage.
pub async fn run_converter(
    request: ConverterRequest<'_>,
    sandbox: &WriteSandbox,
) -> Result<ExternalConversion> {
    let ConverterRequest {
        tool_name,
        config,
        caps,
        input_name,
        input,
        format,
        destination,
    } = request;
    if !config.enabled {
        return Err(EduMindError::Tool(format!(
            "{tool_name} conversion is disabled; enable its tools configuration before requesting `{format}` output"
        )));
    }
    let command = config
        .command
        .as_deref()
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .ok_or_else(|| {
            EduMindError::Tool(format!(
                "{tool_name} conversion requires a configured executable command"
            ))
        })?;
    let program = if command.eq_ignore_ascii_case("python") {
        python_executable()
    } else {
        command.to_owned()
    };
    if Path::new(input_name)
        .file_name()
        .and_then(|name| name.to_str())
        != Some(input_name)
        || !format
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return Err(EduMindError::Tool(
            "converter input names and formats must be simple alphanumeric file values".to_owned(),
        ));
    }
    if input.len() > caps.max_write_bytes {
        return Err(EduMindError::Tool(format!(
            "converter input exceeds the {} byte write cap",
            caps.max_write_bytes
        )));
    }

    let workspace = std::env::temp_dir().join(format!("edumind-converter-{}", Uuid::new_v4()));
    let staged_input = workspace.join(input_name);
    let staged_output = workspace.join(format!("result.{format}"));
    let setup_workspace = workspace.clone();
    let setup_input = staged_input.clone();
    run_blocking(move || {
        fs::create_dir(&setup_workspace).map_err(|source| EduMindError::StorageIo {
            path: setup_workspace.clone(),
            source,
        })?;
        fs::write(&setup_input, input).map_err(|source| EduMindError::StorageIo {
            path: setup_input,
            source,
        })
    })
    .await?;

    let process_result = run_guarded_command(
        GuardedCommandSpec::from_execution_caps(program, caps)?
            .args([
                "--input".to_owned(),
                staged_input.to_string_lossy().into_owned(),
                "--output".to_owned(),
                staged_output.to_string_lossy().into_owned(),
                "--format".to_owned(),
                format.to_owned(),
            ])
            .working_directory(&workspace),
    )
    .await;

    let conversion = match process_result {
        Ok(result) if result.success => {
            let output_path = staged_output.clone();
            let maximum = caps.max_write_bytes;
            let bytes = run_blocking(move || read_converter_output(&output_path, maximum)).await?;
            let destination = destination.to_path_buf();
            let sandbox = sandbox.clone();
            let written = run_blocking(move || sandbox.write(destination, &bytes)).await?;
            Ok(ExternalConversion {
                output_path: written.display().to_string(),
                stdout: result.stdout.text,
                stderr: result.stderr.text,
                stdout_truncated: result.stdout.truncated,
                stderr_truncated: result.stderr.truncated,
            })
        }
        Ok(result) => Err(EduMindError::Tool(format!(
            "{tool_name} converter failed{}: {}",
            result
                .exit_code
                .map_or_else(String::new, |code| format!(" with exit code {code}")),
            result.stderr.text.trim()
        ))),
        Err(error) => Err(EduMindError::Tool(format!(
            "{tool_name} converter could not start: {error}"
        ))),
    };
    let cleanup_workspace = workspace.clone();
    let _ = run_blocking(move || cleanup_workspace_path(&cleanup_workspace)).await;
    conversion
}

fn read_converter_output(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let bytes = fs::read(path).map_err(|source| EduMindError::StorageIo {
        path: path.to_path_buf(),
        source,
    })?;
    if bytes.is_empty() {
        return Err(EduMindError::Tool(
            "converter completed without producing an output artifact".to_owned(),
        ));
    }
    if bytes.len() > maximum {
        return Err(EduMindError::Tool(format!(
            "converter output exceeds the {maximum} byte write cap"
        )));
    }
    Ok(bytes)
}

fn cleanup_workspace_path(path: &PathBuf) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(|source| EduMindError::StorageIo {
            path: path.clone(),
            source,
        })?;
    }
    Ok(())
}
