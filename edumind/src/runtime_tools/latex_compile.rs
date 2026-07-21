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

use super::{GuardedCommandSpec, run_guarded_command};

/// Successful guarded LaTeX compilation output.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LatexCompilation {
    pub source_path: String,
    pub output_path: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

/// Compiles a standalone `.tex` file in a temporary workspace and copies the PDF into OUTPUT.
pub async fn compile_latex(
    source_path: &Path,
    output_root: &Path,
    config: &ExternalToolConfig,
    caps: &ExecutionCapsConfig,
    sandbox: &WriteSandbox,
) -> Result<LatexCompilation> {
    if !config.enabled {
        return Err(EduMindError::Tool(
            "LaTeX compilation is disabled; enable tools.latex_compile.enabled first".to_owned(),
        ));
    }
    let command = config
        .command
        .as_deref()
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .ok_or_else(|| {
            EduMindError::Tool("LaTeX compilation requires tools.latex_compile.command".to_owned())
        })?;
    if source_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("tex"))
    {
        return Err(EduMindError::Tool(
            "latex_compile accepts only .tex source files".to_owned(),
        ));
    }
    let source_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| EduMindError::Tool("LaTeX source path has no file name".to_owned()))?
        .to_owned();
    let source_stem = source_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| EduMindError::Tool("LaTeX source path has no file stem".to_owned()))?
        .to_owned();
    let source = source_path.to_path_buf();
    let source_limit = caps.max_write_bytes;
    let source_contents = run_blocking(move || read_latex_source(&source, source_limit)).await?;

    let workspace = std::env::temp_dir().join(format!("edumind-latex-{}", Uuid::new_v4()));
    let staged_source = workspace.join(&source_name);
    let setup_workspace = workspace.clone();
    let setup_source = staged_source.clone();
    run_blocking(move || {
        fs::create_dir(&setup_workspace).map_err(|source| EduMindError::StorageIo {
            path: setup_workspace.clone(),
            source,
        })?;
        fs::write(&setup_source, source_contents).map_err(|source| EduMindError::StorageIo {
            path: setup_source,
            source,
        })
    })
    .await?;

    let process_result = run_guarded_command(
        GuardedCommandSpec::from_execution_caps(command, caps)?
            .args([
                "-interaction=nonstopmode".to_owned(),
                "-halt-on-error".to_owned(),
                format!("-output-directory={}", workspace.to_string_lossy()),
                source_name,
            ])
            .working_directory(&workspace),
    )
    .await;

    let compilation = match process_result {
        Ok(result) if result.success => {
            let staged_pdf = workspace.join(format!("{source_stem}.pdf"));
            let maximum = caps.max_write_bytes;
            let pdf_bytes = run_blocking(move || read_pdf_output(&staged_pdf, maximum)).await?;
            let output = output_root.join("latex").join(format!("{source_stem}.pdf"));
            let sandbox = sandbox.clone();
            let written = run_blocking(move || sandbox.write(output, &pdf_bytes)).await?;
            Ok(LatexCompilation {
                source_path: source_path.display().to_string(),
                output_path: written.display().to_string(),
                exit_code: result.exit_code,
                stdout: result.stdout.text,
                stderr: result.stderr.text,
                stdout_truncated: result.stdout.truncated,
                stderr_truncated: result.stderr.truncated,
            })
        }
        Ok(result) => Err(EduMindError::Tool(format!(
            "LaTeX compiler failed{}: {}",
            result
                .exit_code
                .map_or_else(String::new, |code| format!(" with exit code {code}")),
            result.stderr.text.trim()
        ))),
        Err(error) => Err(EduMindError::Tool(format!(
            "LaTeX compiler could not start: {error}"
        ))),
    };
    let cleanup_workspace = workspace.clone();
    let _ = run_blocking(move || cleanup_latex_workspace(&cleanup_workspace)).await;
    compilation
}

fn read_latex_source(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path).map_err(|source| EduMindError::StorageIo {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(EduMindError::Tool(
            "latex_compile source must be a regular file".to_owned(),
        ));
    }
    if metadata.len() > u64::try_from(maximum).unwrap_or(u64::MAX) {
        return Err(EduMindError::Tool(format!(
            "LaTeX source exceeds the {maximum} byte write cap"
        )));
    }
    fs::read(path).map_err(|source| EduMindError::StorageIo {
        path: path.to_path_buf(),
        source,
    })
}

fn read_pdf_output(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let bytes = fs::read(path).map_err(|source| EduMindError::StorageIo {
        path: path.to_path_buf(),
        source,
    })?;
    if bytes.is_empty() || !bytes.starts_with(b"%PDF-") {
        return Err(EduMindError::Tool(
            "LaTeX compiler did not produce a valid PDF artifact".to_owned(),
        ));
    }
    if bytes.len() > maximum {
        return Err(EduMindError::Tool(format!(
            "LaTeX PDF exceeds the {maximum} byte write cap"
        )));
    }
    Ok(bytes)
}

fn cleanup_latex_workspace(path: &PathBuf) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(|source| EduMindError::StorageIo {
            path: path.clone(),
            source,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::compile_latex;
    use crate::{
        config::types::{ExecutionCapsConfig, ExternalToolConfig, SecurityConfig},
        security::WriteSandbox,
    };

    #[tokio::test]
    async fn disabled_latex_has_a_clear_error() {
        let error = compile_latex(
            Path::new("missing.tex"),
            Path::new("OUTPUT"),
            &ExternalToolConfig::default(),
            &ExecutionCapsConfig::default(),
            &WriteSandbox::from_config(&SecurityConfig::default()),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("LaTeX compilation is disabled"));
    }
}
