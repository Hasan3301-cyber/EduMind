use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    config::types::OcrConfig,
    infra::{EduMindError, Result, run_blocking},
    runtime_tools::{GuardedCommandSpec, run_guarded_command},
};

/// OCR behavior selected for one full-text ingest request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrMode {
    #[default]
    Auto,
    Force,
    Off,
}

/// Runs `ocrmypdf` through the guarded process runner and returns sidecar text.
pub async fn ocr_pdf(config: &OcrConfig, pdf_path: &Path) -> Result<String> {
    if !config.enabled {
        return Err(EduMindError::Research(
            "OCR is disabled; enable tools.ocr.enabled to process scanned PDFs".to_owned(),
        ));
    }
    let command = config
        .command
        .as_deref()
        .filter(|command| !command.trim().is_empty())
        .ok_or_else(|| {
            EduMindError::Research("OCR requires a configured tools.ocr.command".to_owned())
        })?;
    let workspace = run_blocking(create_ocr_workspace).await?;
    let sidecar = workspace.join("ocr.txt");
    let output = workspace.join("ocr.pdf");
    let command_result = run_guarded_command(
        GuardedCommandSpec::new(command)
            .args([
                "--force-ocr".to_owned(),
                "--sidecar".to_owned(),
                sidecar.to_string_lossy().into_owned(),
                "-l".to_owned(),
                config.language.trim().to_owned(),
                "-q".to_owned(),
                pdf_path.to_string_lossy().into_owned(),
                output.to_string_lossy().into_owned(),
            ])
            .timeout(Duration::from_secs(config.timeout_secs)),
    )
    .await;
    let text_result = match command_result {
        Ok(result) if result.success => {
            let sidecar = sidecar.clone();
            run_blocking(move || read_sidecar(&sidecar)).await
        }
        Ok(result) => Err(EduMindError::Research(format!(
            "OCR command failed{}: {}",
            result
                .exit_code
                .map_or_else(String::new, |code| format!(" with exit code {code}")),
            result.stderr.text.trim()
        ))),
        Err(error) => Err(EduMindError::Research(format!(
            "OCR command is unavailable or could not start: {error}"
        ))),
    };
    let cleanup_workspace = workspace.clone();
    let _ = run_blocking(move || cleanup_ocr_workspace(&cleanup_workspace)).await;
    text_result
}

/// Stages PDF bytes in a temporary location before executing the normal OCR flow.
pub async fn ocr_pdf_bytes(config: &OcrConfig, bytes: Vec<u8>) -> Result<String> {
    let workspace = run_blocking(create_ocr_workspace).await?;
    let input = workspace.join("source.pdf");
    let staged_input = input.clone();
    run_blocking(move || {
        fs::write(&staged_input, bytes).map_err(|source| EduMindError::StorageIo {
            path: staged_input,
            source,
        })
    })
    .await?;
    let text_result = ocr_pdf(config, &input).await;
    let cleanup_workspace = workspace.clone();
    let _ = run_blocking(move || cleanup_ocr_workspace(&cleanup_workspace)).await;
    text_result
}

fn create_ocr_workspace() -> Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("edumind-ocr-{}", Uuid::new_v4()));
    fs::create_dir(&path).map_err(|source| EduMindError::StorageIo {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

fn read_sidecar(path: &Path) -> Result<String> {
    let text = fs::read_to_string(path).map_err(|source| EduMindError::StorageIo {
        path: path.to_path_buf(),
        source,
    })?;
    let text = text.trim().to_owned();
    if text.is_empty() {
        return Err(EduMindError::Research(
            "OCR completed without producing any sidecar text".to_owned(),
        ));
    }
    Ok(text)
}

fn cleanup_ocr_workspace(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(|source| EduMindError::StorageIo {
            path: path.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{OcrMode, ocr_pdf};
    use crate::config::types::OcrConfig;

    #[tokio::test]
    async fn disabled_ocr_returns_a_clear_recoverable_error() {
        let error = ocr_pdf(&OcrConfig::default(), std::path::Path::new("missing.pdf"))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("OCR is disabled"));
    }

    #[tokio::test]
    async fn missing_ocr_command_degrades_with_a_clear_error() {
        let config = OcrConfig {
            enabled: true,
            command: Some("edumind-missing-ocr-command".to_owned()),
            ..OcrConfig::default()
        };
        let error = ocr_pdf(&config, std::path::Path::new("missing.pdf"))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("OCR command is unavailable"));
    }

    #[test]
    fn ocr_modes_default_to_auto_and_round_trip() {
        assert_eq!(OcrMode::default(), OcrMode::Auto);
        assert_eq!(serde_json::to_string(&OcrMode::Force).unwrap(), "\"force\"");
    }
}
