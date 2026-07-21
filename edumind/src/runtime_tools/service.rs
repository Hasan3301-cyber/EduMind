use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::{
    config::{
        EduMindConfig,
        types::{ExternalToolConfig, ModelProviderKind},
    },
    infra::{EduMindError, Result},
    mcp::NotebookLmRouter,
    security::WriteSandbox,
};

use super::{
    ConverterRequest, DocumentArtifact, DocumentConversion, DocumentSummary, ImageArtifact,
    ImageEngine, ImageSearchResult, LatexCompilation, PdfAnalysis, PdfTextExtraction,
    RuntimeArtifactStore, SlideCheck, SlideDeck, SlideDeckSummary, SlideOverflow, SlideRender,
    analyze_pdf, compile_latex, extract_pdf, run_converter,
};

/// Visible local runtime capability state for the desktop administration surface.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuntimeToolsStatus {
    pub output_root: String,
    pub latex_enabled: bool,
    pub document_converter_enabled: bool,
    pub slide_converter_enabled: bool,
    pub image_converter_enabled: bool,
    pub notebooklm_enabled: bool,
    pub local_model_configured: bool,
}

/// Output from a guarded external PPTX export helper.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PresentationBuild {
    pub deck_id: String,
    pub output_path: String,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

/// Coordinates durable local artifacts and optional external runtime integrations.
#[derive(Clone)]
pub struct RuntimeToolService {
    output_root: PathBuf,
    artifacts: RuntimeArtifactStore,
    latex: ExternalToolConfig,
    document_engine: ExternalToolConfig,
    slide_engine: ExternalToolConfig,
    execution: crate::config::types::ExecutionCapsConfig,
    images: ImageEngine,
    notebooklm: NotebookLmRouter,
    local_model_configured: bool,
}

impl RuntimeToolService {
    /// Builds the local-first runtime services from one validated gateway configuration.
    pub fn new(config: &EduMindConfig) -> Result<Self> {
        let output_root = config.meta.data_dir.join("OUTPUT");
        let artifacts = RuntimeArtifactStore::open(output_root.clone())?;
        let images = ImageEngine::new(
            output_root.clone(),
            config.security.execution.clone(),
            config.tools.image_engine.clone(),
        )?;
        Ok(Self {
            output_root,
            artifacts,
            latex: config.tools.latex_compile.clone(),
            document_engine: config.tools.document_engine.clone(),
            slide_engine: config.tools.slide_engine.clone(),
            execution: config.security.execution.clone(),
            images,
            notebooklm: NotebookLmRouter::new(
                config.tools.notebooklm.clone(),
                config.tools.notebooklm_py.clone(),
            )?,
            local_model_configured: config.models.providers.iter().any(|provider| {
                provider.kind == ModelProviderKind::Ollama && !provider.models.is_empty()
            }),
        })
    }

    /// Returns local feature availability without exposing executable commands or endpoints.
    #[must_use]
    pub fn status(&self) -> RuntimeToolsStatus {
        RuntimeToolsStatus {
            output_root: self.output_root.display().to_string(),
            latex_enabled: self.latex.enabled,
            document_converter_enabled: self.document_engine.enabled,
            slide_converter_enabled: self.slide_engine.enabled,
            image_converter_enabled: self.images.converter_enabled(),
            notebooklm_enabled: self.notebooklm.enabled(),
            local_model_configured: self.local_model_configured,
        }
    }

    /// Creates a versioned local HTML document.
    pub fn create_document(
        &self,
        sandbox: &WriteSandbox,
        title: &str,
        content: &str,
        now: DateTime<Utc>,
    ) -> Result<DocumentArtifact> {
        self.artifacts.create_document(sandbox, title, content, now)
    }

    /// Loads one local document by ID.
    pub fn document(&self, id: &str) -> Result<Option<DocumentArtifact>> {
        self.artifacts.document(id)
    }

    /// Lists local document metadata.
    pub fn list_documents(&self) -> Result<Vec<DocumentSummary>> {
        self.artifacts.list_documents()
    }

    /// Modifies a local document while retaining a restore point.
    pub fn modify_document(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        title: Option<String>,
        content: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<DocumentArtifact> {
        self.artifacts
            .modify_document(sandbox, id, title, content, now)
    }

    /// Restores a local document version as a new revision.
    pub fn restore_document(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        version: u64,
        now: DateTime<Utc>,
    ) -> Result<DocumentArtifact> {
        self.artifacts.restore_document(sandbox, id, version, now)
    }

    /// Converts local text formats directly, or stages other formats through a configured helper.
    pub async fn convert_document(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        format: &str,
    ) -> Result<DocumentConversion> {
        let format = normalized_format(format)?;
        if matches!(format.as_str(), "html" | "txt" | "md" | "markdown") {
            return self.artifacts.convert_document(sandbox, id, &format);
        }
        let destination = self
            .output_root
            .join("documents")
            .join(format!("{id}.{format}"));
        let result = run_converter(
            ConverterRequest {
                tool_name: "document_engine",
                config: &self.document_engine,
                caps: &self.execution,
                input_name: "document.html",
                input: self.artifacts.document_html(id)?,
                format: &format,
                destination: &destination,
            },
            sandbox,
        )
        .await?;
        Ok(DocumentConversion {
            id: id.to_owned(),
            format,
            output_path: result.output_path,
            external_converter: true,
        })
    }

    /// Creates a local HTML slide deck.
    pub fn create_deck(
        &self,
        sandbox: &WriteSandbox,
        title: &str,
        body: &str,
        now: DateTime<Utc>,
    ) -> Result<SlideDeck> {
        self.artifacts.create_deck(sandbox, title, body, now)
    }

    /// Loads one local slide deck by ID.
    pub fn deck(&self, id: &str) -> Result<Option<SlideDeck>> {
        self.artifacts.deck(id)
    }

    /// Lists local slide decks.
    pub fn list_decks(&self) -> Result<Vec<SlideDeckSummary>> {
        self.artifacts.list_decks()
    }

    /// Inserts one slide and records a new deck snapshot.
    pub fn insert_slide(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        title: &str,
        body: &str,
        after: Option<usize>,
        now: DateTime<Utc>,
    ) -> Result<SlideDeck> {
        self.artifacts
            .insert_slide(sandbox, id, title, body, after, now)
    }

    /// Deletes a one-based slide number and records a new deck snapshot.
    pub fn delete_slide(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        slide: usize,
        now: DateTime<Utc>,
    ) -> Result<SlideDeck> {
        self.artifacts.delete_slide(sandbox, id, slide, now)
    }

    /// Applies a safe built-in slide theme.
    pub fn set_deck_theme(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        theme: &str,
        now: DateTime<Utc>,
    ) -> Result<SlideDeck> {
        self.artifacts.set_deck_theme(sandbox, id, theme, now)
    }

    /// Restores one retained slide deck snapshot.
    pub fn restore_deck(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        snapshot: u64,
        now: DateTime<Utc>,
    ) -> Result<SlideDeck> {
        self.artifacts.restore_deck(sandbox, id, snapshot, now)
    }

    /// Generates a local SVG preview for a single slide.
    pub fn screenshot_slide(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        slide: usize,
    ) -> Result<SlideRender> {
        self.artifacts.screenshot_slide(sandbox, id, slide)
    }

    /// Returns slide overflow diagnostics.
    pub fn check_slide_overflow(&self, id: &str) -> Result<Vec<SlideOverflow>> {
        self.artifacts.check_overflow(id)
    }

    /// Returns structural and text-budget diagnostics before presentation export.
    pub fn check_deck(&self, id: &str) -> Result<SlideCheck> {
        self.artifacts.check_deck(id)
    }

    /// Generates a local SVG thumbnail grid.
    pub fn thumbnail_grid(&self, sandbox: &WriteSandbox, id: &str) -> Result<SlideRender> {
        self.artifacts.thumbnail_grid(sandbox, id)
    }

    /// Builds a PPTX through an explicitly configured guarded converter command.
    pub async fn build_pptx(&self, sandbox: &WriteSandbox, id: &str) -> Result<PresentationBuild> {
        let check = self.artifacts.check_deck(id)?;
        if !check.valid {
            return Err(EduMindError::Tool(
                "slide_build_pptx requires a structurally valid deck without text overflow"
                    .to_owned(),
            ));
        }
        let destination = self
            .output_root
            .join("slides")
            .join(id)
            .join("presentation.pptx");
        let result = run_converter(
            ConverterRequest {
                tool_name: "slide_engine",
                config: &self.slide_engine,
                caps: &self.execution,
                input_name: "deck.json",
                input: self.artifacts.deck_json(id)?,
                format: "pptx",
                destination: &destination,
            },
            sandbox,
        )
        .await?;
        Ok(PresentationBuild {
            deck_id: id.to_owned(),
            output_path: result.output_path,
            stdout: result.stdout,
            stderr: result.stderr,
            stdout_truncated: result.stdout_truncated,
            stderr_truncated: result.stderr_truncated,
        })
    }

    /// Generates a deterministic offline image artifact.
    pub fn generate_image(
        &self,
        sandbox: &WriteSandbox,
        prompt: &str,
        name: Option<&str>,
    ) -> Result<ImageArtifact> {
        self.images.generate(sandbox, prompt, name)
    }

    /// Searches local image artifacts by filename.
    pub fn search_images(&self, query: &str) -> Result<Vec<ImageSearchResult>> {
        self.images.search(query)
    }

    /// Downloads one bounded HTTPS image into the local output root.
    pub async fn download_image(
        &self,
        sandbox: &WriteSandbox,
        source: &str,
        name: Option<&str>,
    ) -> Result<ImageArtifact> {
        self.images.download(sandbox, source, name).await
    }

    /// Normalizes a raster input through local copy or an optional guarded converter.
    pub async fn ensure_raster_image(
        &self,
        sandbox: &WriteSandbox,
        source: &Path,
    ) -> Result<ImageArtifact> {
        self.images.ensure_raster(sandbox, source).await
    }

    /// Compiles one standalone LaTeX source file into OUTPUT/latex.
    pub async fn compile_latex(
        &self,
        sandbox: &WriteSandbox,
        source: &Path,
    ) -> Result<LatexCompilation> {
        compile_latex(
            source,
            &self.output_root,
            &self.latex,
            &self.execution,
            sandbox,
        )
        .await
    }

    /// Extracts a bounded local PDF text layer.
    pub async fn extract_pdf(&self, source: &Path) -> Result<PdfTextExtraction> {
        extract_pdf(source, &self.execution).await
    }

    /// Derives deterministic local PDF text metadata.
    pub async fn analyze_pdf(&self, source: &Path) -> Result<PdfAnalysis> {
        analyze_pdf(source, &self.execution).await
    }

    /// Executes a configured NotebookLM MCP operation.
    pub async fn notebooklm_call(&self, operation: &str, arguments: Value) -> Result<Value> {
        self.notebooklm.call(operation, arguments).await
    }

    /// Retrieves preferred NotebookLM integration health.
    pub async fn notebooklm_health(&self) -> Result<Value> {
        self.notebooklm.health().await
    }
}

fn normalized_format(format: &str) -> Result<String> {
    let format = format.trim().to_ascii_lowercase();
    if format.is_empty()
        || !format
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return Err(EduMindError::Tool(
            "artifact formats must be non-empty alphanumeric values".to_owned(),
        ));
    }
    Ok(format)
}
