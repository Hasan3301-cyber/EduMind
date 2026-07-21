use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use reqwest::{Client, Url, header::CONTENT_TYPE};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    config::types::{ExecutionCapsConfig, ExternalToolConfig},
    infra::{EduMindError, Result, run_blocking},
    security::WriteSandbox,
};

use super::converter::{ConverterRequest, run_converter};

/// A generated, downloaded, or normalized local image artifact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ImageArtifact {
    pub output_path: String,
    pub mime_type: String,
    pub generated: bool,
    pub description: String,
}

/// A local image artifact matching an offline search query.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ImageSearchResult {
    pub path: String,
    pub name: String,
    pub mime_type: String,
}

/// Local-first image operations with an optional guarded raster converter.
#[derive(Clone)]
pub struct ImageEngine {
    output_root: PathBuf,
    caps: ExecutionCapsConfig,
    converter: ExternalToolConfig,
    client: Client,
}

impl ImageEngine {
    /// Creates an image engine that never uses an implicit remote image provider.
    pub fn new(
        output_root: PathBuf,
        caps: ExecutionCapsConfig,
        converter: ExternalToolConfig,
    ) -> Result<Self> {
        let client = Client::builder().build().map_err(EduMindError::from)?;
        Ok(Self {
            output_root,
            caps,
            converter,
            client,
        })
    }

    /// Indicates whether SVG rasterization can use a configured external converter.
    #[must_use]
    pub fn converter_enabled(&self) -> bool {
        self.converter.enabled
    }

    /// Creates a deterministic SVG study visual, preserving offline operation by default.
    pub fn generate(
        &self,
        sandbox: &WriteSandbox,
        prompt: &str,
        name: Option<&str>,
    ) -> Result<ImageArtifact> {
        let prompt = prompt.trim();
        if prompt.is_empty() || prompt.chars().count() > 2_000 {
            return Err(EduMindError::Tool(
                "image_generate requires a prompt containing 1-2000 characters".to_owned(),
            ));
        }
        let stem = safe_stem(name.unwrap_or("study-visual"));
        let destination = self
            .output_root
            .join("images")
            .join(format!("{stem}-{}.svg", Uuid::new_v4()));
        let output_path = sandbox
            .write(destination, render_prompt_svg(prompt).as_bytes())?
            .display()
            .to_string();
        Ok(ImageArtifact {
            output_path,
            mime_type: "image/svg+xml".to_owned(),
            generated: true,
            description: format!("Offline study visual generated from: {prompt}"),
        })
    }

    /// Lists images already stored in OUTPUT/images without contacting a remote search provider.
    pub fn search(&self, query: &str) -> Result<Vec<ImageSearchResult>> {
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return Err(EduMindError::Tool(
                "image_search requires a non-empty query".to_owned(),
            ));
        }
        let directory = self.output_root.join("images");
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(EduMindError::StorageIo {
                    path: directory,
                    source,
                });
            }
        };
        let mut matches = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_name()?.to_str()?.to_owned();
                let lower = name.to_ascii_lowercase();
                lower.contains(&query).then(|| ImageSearchResult {
                    mime_type: mime_from_extension(
                        path.extension().and_then(|value| value.to_str()),
                    ),
                    path: path.display().to_string(),
                    name,
                })
            })
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| left.name.cmp(&right.name));
        matches.truncate(50);
        Ok(matches)
    }

    /// Downloads one HTTPS image only when the server declares a bounded image payload.
    pub async fn download(
        &self,
        sandbox: &WriteSandbox,
        source: &str,
        name: Option<&str>,
    ) -> Result<ImageArtifact> {
        let url = Url::parse(source.trim()).map_err(|error| {
            EduMindError::Tool(format!("image_download received an invalid URL: {error}"))
        })?;
        if url.scheme() != "https" || url.host_str().is_none() {
            return Err(EduMindError::Tool(
                "image_download accepts HTTPS URLs with a host only".to_owned(),
            ));
        }
        let fallback_name = url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .unwrap_or("image")
            .to_owned();
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| EduMindError::Tool(format!("image download failed: {error}")))?
            .error_for_status()
            .map_err(|error| {
                EduMindError::Tool(format!("image server rejected request: {error}"))
            })?;
        let content_length = response.content_length().ok_or_else(|| {
            EduMindError::Tool(
                "image_download requires a server Content-Length for bounded intake".to_owned(),
            )
        })?;
        if content_length > u64::try_from(self.caps.max_write_bytes).unwrap_or(u64::MAX) {
            return Err(EduMindError::Tool(format!(
                "image download exceeds the {} byte write cap",
                self.caps.max_write_bytes
            )));
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let (extension, mime_type) = image_type(&content_type)?;
        let bytes = response
            .bytes()
            .await
            .map_err(|error| EduMindError::Tool(format!("image body read failed: {error}")))?;
        if bytes.len() > self.caps.max_write_bytes {
            return Err(EduMindError::Tool(format!(
                "image download exceeds the {} byte write cap",
                self.caps.max_write_bytes
            )));
        }
        let stem = safe_stem(name.unwrap_or(&fallback_name));
        let destination = self.output_root.join("images").join(format!(
            "{stem}-{}.{}",
            Uuid::new_v4(),
            extension
        ));
        let output_path = sandbox.write(destination, &bytes)?.display().to_string();
        Ok(ImageArtifact {
            output_path,
            mime_type: mime_type.to_owned(),
            generated: false,
            description: format!("Downloaded bounded HTTPS image from {source}"),
        })
    }

    /// Copies native raster files or routes SVG conversion through a configured guarded converter.
    pub async fn ensure_raster(
        &self,
        sandbox: &WriteSandbox,
        source: &Path,
    ) -> Result<ImageArtifact> {
        let source = source.to_path_buf();
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| {
                EduMindError::Tool("image_ensure_raster source has no file extension".to_owned())
            })?;
        let input_name = source
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                EduMindError::Tool("image_ensure_raster source has no file name".to_owned())
            })?
            .to_owned();
        let maximum = self.caps.max_write_bytes;
        let bytes = run_blocking(move || read_bounded_image(&source, maximum)).await?;
        let stem = safe_stem(
            Path::new(&input_name)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("image"),
        );
        if is_raster_extension(&extension) {
            let destination = self
                .output_root
                .join("images")
                .join(format!("{stem}-raster.{extension}"));
            let output_path = sandbox.write(destination, &bytes)?.display().to_string();
            return Ok(ImageArtifact {
                output_path,
                mime_type: mime_from_extension(Some(&extension)),
                generated: false,
                description: "Verified and copied native raster image".to_owned(),
            });
        }
        if extension != "svg" {
            return Err(EduMindError::Tool(
                "image_ensure_raster supports PNG, JPEG, WEBP, GIF, or SVG input".to_owned(),
            ));
        }
        let destination = self
            .output_root
            .join("images")
            .join(format!("{stem}-raster.png"));
        let converted = run_converter(
            ConverterRequest {
                tool_name: "image_engine",
                config: &self.converter,
                caps: &self.caps,
                input_name: &input_name,
                input: bytes,
                format: "png",
                destination: &destination,
            },
            sandbox,
        )
        .await?;
        Ok(ImageArtifact {
            output_path: converted.output_path,
            mime_type: "image/png".to_owned(),
            generated: false,
            description: "SVG rasterized by the configured guarded image converter".to_owned(),
        })
    }
}

fn read_bounded_image(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path).map_err(|source| EduMindError::StorageIo {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(EduMindError::Tool(
            "image source must be a regular file".to_owned(),
        ));
    }
    if metadata.len() > u64::try_from(maximum).unwrap_or(u64::MAX) {
        return Err(EduMindError::Tool(format!(
            "image source exceeds the {maximum} byte write cap"
        )));
    }
    fs::read(path).map_err(|source| EduMindError::StorageIo {
        path: path.to_path_buf(),
        source,
    })
}

fn image_type(content_type: &str) -> Result<(&'static str, &'static str)> {
    match content_type {
        "image/png" => Ok(("png", "image/png")),
        "image/jpeg" => Ok(("jpg", "image/jpeg")),
        "image/webp" => Ok(("webp", "image/webp")),
        "image/gif" => Ok(("gif", "image/gif")),
        "image/svg+xml" => Ok(("svg", "image/svg+xml")),
        _ => Err(EduMindError::Tool(
            "image_download requires an image/* Content-Type".to_owned(),
        )),
    }
}

fn is_raster_extension(extension: &str) -> bool {
    matches!(extension, "png" | "jpg" | "jpeg" | "webp" | "gif")
}

fn mime_from_extension(extension: Option<&str>) -> String {
    match extension.map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
    .to_owned()
}

fn safe_stem(value: &str) -> String {
    let stem = value
        .chars()
        .filter_map(|character| {
            (character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
                .then_some(character.to_ascii_lowercase())
        })
        .take(60)
        .collect::<String>();
    if stem.is_empty() {
        "image".to_owned()
    } else {
        stem
    }
}

fn render_prompt_svg(prompt: &str) -> String {
    let lines = wrapped_lines(prompt, 42, 8)
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            format!(
                "<text x=\"96\" y=\"{}\" fill=\"#f8fafc\" font-size=\"42\" font-family=\"Arial,sans-serif\">{}</text>",
                245 + index * 58,
                escape_xml(&line)
            )
        })
        .collect::<String>();
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1200\" height=\"630\" viewBox=\"0 0 1200 630\"><defs><linearGradient id=\"g\" x1=\"0\" x2=\"1\" y1=\"0\" y2=\"1\"><stop stop-color=\"#4f46e5\"/><stop offset=\"1\" stop-color=\"#0f766e\"/></linearGradient></defs><rect width=\"1200\" height=\"630\" rx=\"32\" fill=\"url(#g)\"/><text x=\"96\" y=\"138\" fill=\"#c7d2fe\" font-size=\"28\" font-weight=\"700\" font-family=\"Arial,sans-serif\">EDUMIND · STUDY VISUAL</text>{}</svg>",
        lines
    )
}

fn wrapped_lines(value: &str, width: usize, limit: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in value.split_whitespace() {
        if !current.is_empty() && current.chars().count() + word.chars().count() + 1 > width {
            lines.push(current);
            current = String::new();
            if lines.len() == limit {
                return lines;
            }
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() && lines.len() < limit {
        lines.push(current);
    }
    lines
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use uuid::Uuid;

    use super::ImageEngine;
    use crate::{
        config::types::{ExecutionCapsConfig, ExternalToolConfig, SecurityConfig},
        security::WriteSandbox,
    };

    #[test]
    fn generates_an_offline_svg_artifact() {
        let base = env::temp_dir().join(format!("edumind-images-{}", Uuid::new_v4()));
        let output = base.join("OUTPUT");
        let security = SecurityConfig {
            allowed_tool_write_roots: vec![output.clone()],
            ..SecurityConfig::default()
        };
        let engine = ImageEngine::new(
            output,
            ExecutionCapsConfig::default(),
            ExternalToolConfig::default(),
        )
        .unwrap();
        let image = engine
            .generate(
                &WriteSandbox::from_config(&security),
                "A concept map for biology",
                None,
            )
            .unwrap();

        assert!(image.output_path.ends_with(".svg"));
        fs::remove_dir_all(base).unwrap();
    }
}
