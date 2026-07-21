use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use axum::{Json, extract::State, http::StatusCode};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    gateway::AppState,
    infra::{EduMindError, Result},
    security::WriteSandbox,
};

const MAX_TITLE_CHARS: usize = 180;
const MAX_CONTENT_BYTES: usize = 512 * 1024;
const MAX_FOLDER_SEGMENTS: usize = 4;
const MAX_FOLDER_SEGMENT_CHARS: usize = 80;
const MAX_FILENAME_ATTEMPTS: usize = 100;

type ApiError = (StatusCode, Json<Value>);
type ApiResult<T> = std::result::Result<Json<T>, ApiError>;

/// The durable artifact format explicitly selected by a student.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClassNotesExportFormat {
    Html,
    Pdf,
}

impl ClassNotesExportFormat {
    const fn extension(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Pdf => "pdf",
        }
    }
}

/// A deliberate export request for already-generated Class Notes content.
#[derive(Debug, Deserialize)]
pub struct ExportClassNotesRequest {
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default = "default_format")]
    pub format: String,
}

/// Paths returned after a Class Notes artifact is safely written beneath OUTPUT.
#[derive(Clone, Debug, Serialize)]
pub struct ClassNotesExport {
    pub document_id: String,
    pub format: String,
    pub output_directory: String,
    pub source_html_path: String,
    pub artifact_path: String,
}

/// Saves one generated Class Notes response as an HTML source or a configured PDF artifact.
pub async fn export_notes(
    State(state): State<AppState>,
    Json(request): Json<ExportClassNotesRequest>,
) -> ApiResult<ClassNotesExport> {
    let title = normalize_title(&request.title).map_err(api_error)?;
    validate_content(&request.content).map_err(api_error)?;
    let format = parse_format(&request.format).map_err(api_error)?;
    let output_folder =
        normalize_output_folder(request.destination.as_deref()).map_err(api_error)?;

    let runtime_tools = state.runtime_tools();
    let runtime_status = runtime_tools.status();
    if format == ClassNotesExportFormat::Pdf && !runtime_status.document_converter_enabled {
        return Err(converter_unavailable());
    }

    let config = state.config_snapshot().map_err(api_error)?;
    let sandbox = WriteSandbox::from_config(&config.security);
    let now = Utc::now();
    let document = runtime_tools
        .create_document(&sandbox, &title, &request.content, now)
        .map_err(api_error)?;
    let html = runtime_tools
        .convert_document(&sandbox, &document.id, "html")
        .await
        .map_err(api_error)?;
    let converted = if format == ClassNotesExportFormat::Pdf {
        Some(
            runtime_tools
                .convert_document(&sandbox, &document.id, "pdf")
                .await
                .map_err(api_error)?,
        )
    } else {
        None
    };

    let output_root = PathBuf::from(runtime_status.output_root);
    let destination_directory = output_root.join("ClassNotes").join(output_folder);
    let filename_stem =
        next_available_stem(&destination_directory, &title, now).map_err(api_error)?;
    let source_html_path = copy_runtime_artifact(
        &sandbox,
        &output_root,
        &html.output_path,
        destination_directory
            .join("sources")
            .join(format!("{filename_stem}.html")),
    )
    .map_err(api_error)?;
    let artifact_path = match converted {
        Some(conversion) => copy_runtime_artifact(
            &sandbox,
            &output_root,
            &conversion.output_path,
            destination_directory.join(format!("{filename_stem}.pdf")),
        )
        .map_err(api_error)?,
        None => copy_runtime_artifact(
            &sandbox,
            &output_root,
            &html.output_path,
            destination_directory.join(format!("{filename_stem}.html")),
        )
        .map_err(api_error)?,
    };

    Ok(Json(ClassNotesExport {
        document_id: document.id,
        format: format.extension().to_owned(),
        output_directory: destination_directory.display().to_string(),
        source_html_path,
        artifact_path,
    }))
}

fn default_format() -> String {
    "html".to_owned()
}

fn parse_format(value: &str) -> Result<ClassNotesExportFormat> {
    match value.trim().to_ascii_lowercase().as_str() {
        "html" => Ok(ClassNotesExportFormat::Html),
        "pdf" => Ok(ClassNotesExportFormat::Pdf),
        _ => Err(EduMindError::Tool(
            "Class Notes exports support only html or pdf formats".to_owned(),
        )),
    }
}

fn normalize_title(value: &str) -> Result<String> {
    let title = value.trim();
    if title.is_empty() {
        return Err(EduMindError::Tool(
            "Class Notes export requires a title".to_owned(),
        ));
    }
    Ok(title.chars().take(MAX_TITLE_CHARS).collect())
}

fn validate_content(value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(EduMindError::Tool(
            "Class Notes export requires generated note content".to_owned(),
        ));
    }
    if value.len() > MAX_CONTENT_BYTES {
        return Err(EduMindError::Tool(format!(
            "Class Notes export exceeds the {MAX_CONTENT_BYTES} byte limit"
        )));
    }
    Ok(())
}

fn normalize_output_folder(value: Option<&str>) -> Result<PathBuf> {
    let raw = value.unwrap_or("General").trim();
    if raw.is_empty() {
        return Ok(PathBuf::from("General"));
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err(EduMindError::Security(
            "choose a folder inside EduMind OUTPUT/ClassNotes, not an absolute path".to_owned(),
        ));
    }

    let mut normalized = PathBuf::new();
    let mut count = 0usize;
    for component in path.components() {
        let Component::Normal(segment) = component else {
            return Err(EduMindError::Security(
                "the Class Notes destination cannot contain . or .. path segments".to_owned(),
            ));
        };
        let segment = segment.to_str().ok_or_else(|| {
            EduMindError::Security(
                "the Class Notes destination must use valid Unicode folder names".to_owned(),
            )
        })?;
        if !valid_folder_segment(segment) {
            return Err(EduMindError::Security(
                "destination folder names may use letters, numbers, spaces, hyphens, and underscores only"
                    .to_owned(),
            ));
        }
        count += 1;
        if count > MAX_FOLDER_SEGMENTS {
            return Err(EduMindError::Security(format!(
                "choose no more than {MAX_FOLDER_SEGMENTS} nested destination folders"
            )));
        }
        normalized.push(segment);
    }
    if normalized.as_os_str().is_empty() {
        return Err(EduMindError::Security(
            "choose a valid destination folder inside OUTPUT/ClassNotes".to_owned(),
        ));
    }
    Ok(normalized)
}

fn valid_folder_segment(value: &str) -> bool {
    let count = value.chars().count();
    count > 0
        && count <= MAX_FOLDER_SEGMENT_CHARS
        && value
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, ' ' | '-' | '_'))
}

fn next_available_stem(directory: &Path, title: &str, now: DateTime<Utc>) -> Result<String> {
    let topic = filename_component(title);
    let base = format!("ClassNotes_{topic}_{}", now.format("%Y-%m-%d"));
    for attempt in 0..MAX_FILENAME_ATTEMPTS {
        let stem = if attempt == 0 {
            base.clone()
        } else {
            format!("{base}-{attempt}")
        };
        let html = directory.join(format!("{stem}.html"));
        let pdf = directory.join(format!("{stem}.pdf"));
        if !html.exists() && !pdf.exists() {
            return Ok(stem);
        }
    }
    Err(EduMindError::StorageIo {
        path: directory.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "too many matching Class Notes export files",
        ),
    })
}

fn filename_component(value: &str) -> String {
    let mut result = String::new();
    let mut last_was_separator = false;
    for character in value.chars() {
        if character.is_alphanumeric() {
            result.push(character);
            last_was_separator = false;
        } else if !last_was_separator {
            result.push('-');
            last_was_separator = true;
        }
        if result.chars().count() >= 80 {
            break;
        }
    }
    let trimmed = result.trim_matches('-');
    if trimmed.is_empty() {
        "Notes".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn copy_runtime_artifact(
    sandbox: &WriteSandbox,
    output_root: &Path,
    source_path: &str,
    destination: PathBuf,
) -> Result<String> {
    let canonical_root =
        fs::canonicalize(output_root).map_err(|source| EduMindError::StorageIo {
            path: output_root.to_path_buf(),
            source,
        })?;
    let source = PathBuf::from(source_path);
    let canonical_source =
        fs::canonicalize(&source).map_err(|source_error| EduMindError::StorageIo {
            path: source.clone(),
            source: source_error,
        })?;
    if !canonical_source.starts_with(&canonical_root) {
        return Err(EduMindError::Security(
            "runtime conversion returned an artifact outside EduMind OUTPUT".to_owned(),
        ));
    }
    let bytes = fs::read(&canonical_source).map_err(|source_error| EduMindError::StorageIo {
        path: canonical_source,
        source: source_error,
    })?;
    sandbox
        .write(destination, &bytes)
        .map(|path| path.display().to_string())
}

fn converter_unavailable() -> ApiError {
    (
        StatusCode::CONFLICT,
        Json(json!({
            "error": {
                "code": "document_converter_unavailable",
                "message": "PDF export needs a configured local document converter. You can still save the note as HTML under OUTPUT/ClassNotes."
            }
        })),
    )
}

fn api_error(error: EduMindError) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": {
                "code": "class_notes_export_invalid",
                "message": error.to_string()
            }
        })),
    )
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    use super::{
        ClassNotesExportFormat, ExportClassNotesRequest, export_notes, filename_component,
        next_available_stem, normalize_output_folder, parse_format,
    };
    use crate::{config::EduMindConfig, gateway::AppState};

    #[test]
    fn keeps_student_selected_folders_relative_to_class_notes_output() {
        assert_eq!(
            normalize_output_folder(Some("Semester 1/Calculus")).unwrap(),
            std::path::PathBuf::from("Semester 1").join("Calculus")
        );
        assert!(normalize_output_folder(Some("../outside")).is_err());
        assert!(normalize_output_folder(Some("D:\\outside")).is_err());
    }

    #[test]
    fn creates_a_canonical_class_notes_filename() {
        let directory = std::env::temp_dir().join("edumind-class-notes-export-test");
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();

        assert_eq!(
            filename_component("Limits & continuity"),
            "Limits-continuity"
        );
        assert_eq!(
            next_available_stem(&directory, "Limits & continuity", now).unwrap(),
            "ClassNotes_Limits-continuity_2026-07-20"
        );
    }

    #[test]
    fn accepts_only_html_or_pdf_exports() {
        assert_eq!(parse_format("html").unwrap(), ClassNotesExportFormat::Html);
        assert_eq!(parse_format("PDF").unwrap(), ClassNotesExportFormat::Pdf);
        assert!(parse_format("docx").is_err());
    }

    #[tokio::test]
    async fn exports_html_beneath_the_selected_class_notes_folder() {
        let root = std::env::temp_dir().join(format!("edumind-class-notes-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut config = EduMindConfig::default();
        config.meta.data_dir = root.clone();
        config.memory.db_path = root.join("memory.db");
        config.security.action_password_hash_path = root.join("action-password.argon2");
        config.security.allowed_tool_write_roots = vec![root.join("OUTPUT")];
        let state = AppState::in_memory(config).unwrap();

        let result = export_notes(
            axum::extract::State(state),
            axum::Json(ExportClassNotesRequest {
                title: "Limits review".to_owned(),
                content: "A limit describes a value a function approaches.".to_owned(),
                destination: Some("Semester 1/Calculus".to_owned()),
                format: "html".to_owned(),
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(result.format, "html");
        assert!(result.artifact_path.contains("OUTPUT"));
        assert!(result.artifact_path.contains("ClassNotes"));
        assert!(result.artifact_path.contains("Semester 1"));
        assert!(result.source_html_path.contains("sources"));
        assert!(!result.artifact_path.contains("sources"));
        assert!(result.artifact_path.ends_with(".html"));
        let expected_directory = std::fs::canonicalize(
            root.join("OUTPUT")
                .join("ClassNotes")
                .join("Semester 1")
                .join("Calculus"),
        )
        .unwrap();
        assert_eq!(
            std::path::Path::new(&result.artifact_path).parent(),
            Some(expected_directory.as_path())
        );
        assert!(std::path::Path::new(&result.artifact_path).is_file());
        assert!(std::path::Path::new(&result.source_html_path).is_file());

        std::fs::remove_dir_all(root).unwrap();
    }
}
