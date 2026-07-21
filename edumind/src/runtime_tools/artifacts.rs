use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    infra::{EduMindError, Result},
    security::WriteSandbox,
};

const MAX_HISTORY: usize = 24;
const TITLE_LIMIT: usize = 180;
const SLIDE_TITLE_LIMIT: usize = 84;
const SLIDE_BODY_LIMIT: usize = 600;

/// Persisted local HTML document with bounded revision history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentArtifact {
    pub id: String,
    pub title: String,
    pub content: String,
    pub version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub history: Vec<DocumentVersion>,
}

/// One restorable document revision.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentVersion {
    pub version: u64,
    pub title: String,
    pub content: String,
    pub updated_at: DateTime<Utc>,
}

/// Lightweight document metadata returned by list operations.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DocumentSummary {
    pub id: String,
    pub title: String,
    pub version: u64,
    pub updated_at: DateTime<Utc>,
}

/// A local document conversion artifact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DocumentConversion {
    pub id: String,
    pub format: String,
    pub output_path: String,
    pub external_converter: bool,
}

/// One editable slide in a local presentation deck.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Slide {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub notes: String,
}

/// Persisted local HTML presentation deck with bounded snapshots.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlideDeck {
    pub id: String,
    pub title: String,
    pub theme: String,
    pub slides: Vec<Slide>,
    pub version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub history: Vec<SlideDeckSnapshot>,
}

/// A restorable presentation snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlideDeckSnapshot {
    pub version: u64,
    pub title: String,
    pub theme: String,
    pub slides: Vec<Slide>,
    pub updated_at: DateTime<Utc>,
}

/// Lightweight presentation metadata returned by list operations.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SlideDeckSummary {
    pub id: String,
    pub title: String,
    pub theme: String,
    pub slide_count: usize,
    pub version: u64,
    pub updated_at: DateTime<Utc>,
}

/// A detected slide text overflow.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SlideOverflow {
    pub slide: usize,
    pub field: String,
    pub characters: usize,
    pub limit: usize,
}

/// Structural and overflow diagnostics for a local slide deck.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SlideCheck {
    pub valid: bool,
    pub issues: Vec<String>,
    pub overflow: Vec<SlideOverflow>,
}

/// Location of a generated SVG slide preview or thumbnail grid.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SlideRender {
    pub deck_id: String,
    pub output_path: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct ArtifactManifest {
    documents: BTreeMap<String, DocumentArtifact>,
    decks: BTreeMap<String, SlideDeck>,
}

/// File-backed local documents and HTML slide decks stored below the configured OUTPUT root.
#[derive(Clone)]
pub struct RuntimeArtifactStore {
    root: PathBuf,
    state: Arc<Mutex<ArtifactManifest>>,
}

impl RuntimeArtifactStore {
    /// Opens an existing manifest or initializes a lazy empty store without creating output paths.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let manifest_path = root.join("runtime-artifacts.json");
        let state = match fs::read(&manifest_path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                EduMindError::InvalidStoredData(format!(
                    "runtime artifact manifest {} is invalid: {error}",
                    manifest_path.display()
                ))
            })?,
            Err(error) if error.kind() == ErrorKind::NotFound => ArtifactManifest::default(),
            Err(source) => {
                return Err(EduMindError::StorageIo {
                    path: manifest_path,
                    source,
                });
            }
        };
        Ok(Self {
            root,
            state: Arc::new(Mutex::new(state)),
        })
    }

    /// Returns the root used for all generated document, deck, and image artifacts.
    #[must_use]
    pub fn output_root(&self) -> &Path {
        &self.root
    }

    /// Creates a sanitized local HTML document and its initial restore point.
    pub fn create_document(
        &self,
        sandbox: &WriteSandbox,
        title: impl Into<String>,
        content: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<DocumentArtifact> {
        let title = normalize_title(title.into(), "document")?;
        let id = Uuid::new_v4().to_string();
        let mut document = DocumentArtifact {
            id: id.clone(),
            title,
            content: content.into(),
            version: 1,
            created_at: now,
            updated_at: now,
            history: Vec::new(),
        };
        document.history.push(document_version(&document));

        let mut state = self.lock()?;
        let mut next = state.clone();
        next.documents.insert(id.clone(), document.clone());
        self.write_document_and_manifest(sandbox, &document, &next)?;
        *state = next;
        Ok(document)
    }

    /// Loads a document and all retained restore points by stable ID.
    pub fn document(&self, id: &str) -> Result<Option<DocumentArtifact>> {
        Ok(self.lock()?.documents.get(id).cloned())
    }

    /// Lists local document metadata without returning every document body.
    pub fn list_documents(&self) -> Result<Vec<DocumentSummary>> {
        Ok(self
            .lock()?
            .documents
            .values()
            .map(|document| DocumentSummary {
                id: document.id.clone(),
                title: document.title.clone(),
                version: document.version,
                updated_at: document.updated_at,
            })
            .collect())
    }

    /// Changes a document title and/or content while retaining its new revision as a restore point.
    pub fn modify_document(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        title: Option<String>,
        content: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<DocumentArtifact> {
        if title.is_none() && content.is_none() {
            return Err(EduMindError::Tool(
                "doc_modify requires a title or content change".to_owned(),
            ));
        }
        let mut state = self.lock()?;
        let mut next = state.clone();
        let document = next
            .documents
            .get_mut(id)
            .ok_or_else(|| EduMindError::Tool(format!("document `{id}` does not exist")))?;
        if let Some(title) = title {
            document.title = normalize_title(title, "document")?;
        }
        if let Some(content) = content {
            document.content = content;
        }
        advance_document(document, now)?;
        let updated = document.clone();
        self.write_document_and_manifest(sandbox, &updated, &next)?;
        *state = next;
        Ok(updated)
    }

    /// Restores a prior document state as a new current revision.
    pub fn restore_document(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        version: u64,
        now: DateTime<Utc>,
    ) -> Result<DocumentArtifact> {
        let mut state = self.lock()?;
        let mut next = state.clone();
        let document = next
            .documents
            .get_mut(id)
            .ok_or_else(|| EduMindError::Tool(format!("document `{id}` does not exist")))?;
        let snapshot = document
            .history
            .iter()
            .find(|snapshot| snapshot.version == version)
            .cloned()
            .ok_or_else(|| {
                EduMindError::Tool(format!(
                    "document `{id}` has no retained version `{version}`"
                ))
            })?;
        document.title = snapshot.title;
        document.content = snapshot.content;
        advance_document(document, now)?;
        let restored = document.clone();
        self.write_document_and_manifest(sandbox, &restored, &next)?;
        *state = next;
        Ok(restored)
    }

    /// Builds a local HTML, plain-text, or Markdown document artifact without requiring helpers.
    pub fn convert_document(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        format: &str,
    ) -> Result<DocumentConversion> {
        let document = self.required_document(id)?;
        let format = format.trim().to_ascii_lowercase();
        let (destination, bytes) = match format.as_str() {
            "html" => (
                self.document_html_path(id),
                render_document_html(&document).into_bytes(),
            ),
            "txt" => (
                self.root.join("documents").join(format!("{id}.txt")),
                document.content.into_bytes(),
            ),
            "md" | "markdown" => (
                self.root.join("documents").join(format!("{id}.md")),
                format!("# {}\n\n{}\n", document.title, document.content).into_bytes(),
            ),
            _ => {
                return Err(EduMindError::Tool(
                    "doc_convert supports html, txt, and markdown locally; configure tools.document_engine for other formats"
                        .to_owned(),
                ));
            }
        };
        let output_path = sandbox.write(destination, &bytes)?.display().to_string();
        Ok(DocumentConversion {
            id: id.to_owned(),
            format,
            output_path,
            external_converter: false,
        })
    }

    /// Returns rendered HTML for a document so optional converters can stage it safely.
    pub fn document_html(&self, id: &str) -> Result<Vec<u8>> {
        Ok(render_document_html(&self.required_document(id)?).into_bytes())
    }

    /// Creates a local HTML slide deck containing an editable first slide.
    pub fn create_deck(
        &self,
        sandbox: &WriteSandbox,
        title: impl Into<String>,
        body: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<SlideDeck> {
        let title = normalize_title(title.into(), "slide deck")?;
        let id = Uuid::new_v4().to_string();
        let mut deck = SlideDeck {
            id: id.clone(),
            title: title.clone(),
            theme: "midnight".to_owned(),
            slides: vec![Slide {
                id: Uuid::new_v4().to_string(),
                title,
                body: body.into(),
                notes: String::new(),
            }],
            version: 1,
            created_at: now,
            updated_at: now,
            history: Vec::new(),
        };
        deck.history.push(deck_snapshot(&deck));

        let mut state = self.lock()?;
        let mut next = state.clone();
        next.decks.insert(id, deck.clone());
        self.write_deck_and_manifest(sandbox, &deck, &next)?;
        *state = next;
        Ok(deck)
    }

    /// Loads one local slide deck by stable ID.
    pub fn deck(&self, id: &str) -> Result<Option<SlideDeck>> {
        Ok(self.lock()?.decks.get(id).cloned())
    }

    /// Lists deck metadata for presentation selection UI.
    pub fn list_decks(&self) -> Result<Vec<SlideDeckSummary>> {
        Ok(self
            .lock()?
            .decks
            .values()
            .map(|deck| SlideDeckSummary {
                id: deck.id.clone(),
                title: deck.title.clone(),
                theme: deck.theme.clone(),
                slide_count: deck.slides.len(),
                version: deck.version,
                updated_at: deck.updated_at,
            })
            .collect())
    }

    /// Inserts an editable slide after a one-based slide position, or appends when omitted.
    pub fn insert_slide(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        title: impl Into<String>,
        body: impl Into<String>,
        after: Option<usize>,
        now: DateTime<Utc>,
    ) -> Result<SlideDeck> {
        let title = normalize_title(title.into(), "slide")?;
        self.mutate_deck(sandbox, id, now, move |deck| {
            let position = after.unwrap_or(deck.slides.len()).min(deck.slides.len());
            deck.slides.insert(
                position,
                Slide {
                    id: Uuid::new_v4().to_string(),
                    title,
                    body: body.into(),
                    notes: String::new(),
                },
            );
            Ok(())
        })
    }

    /// Deletes one one-based slide while preserving at least one editable slide in every deck.
    pub fn delete_slide(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        slide: usize,
        now: DateTime<Utc>,
    ) -> Result<SlideDeck> {
        self.mutate_deck(sandbox, id, now, move |deck| {
            if deck.slides.len() <= 1 {
                return Err(EduMindError::Tool(
                    "a slide deck must retain at least one slide".to_owned(),
                ));
            }
            let index = slide
                .checked_sub(1)
                .filter(|index| *index < deck.slides.len())
                .ok_or_else(|| {
                    EduMindError::Tool(format!(
                        "slide `{slide}` is outside this deck's 1-{} range",
                        deck.slides.len()
                    ))
                })?;
            deck.slides.remove(index);
            Ok(())
        })
    }

    /// Applies one of the built-in safe CSS themes to a local deck.
    pub fn set_deck_theme(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        theme: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<SlideDeck> {
        let theme = normalize_theme(theme.into())?;
        self.mutate_deck(sandbox, id, now, move |deck| {
            deck.theme = theme;
            Ok(())
        })
    }

    /// Restores a retained deck snapshot as a new revision.
    pub fn restore_deck(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        snapshot: u64,
        now: DateTime<Utc>,
    ) -> Result<SlideDeck> {
        self.mutate_deck(sandbox, id, now, move |deck| {
            let restored = deck
                .history
                .iter()
                .find(|candidate| candidate.version == snapshot)
                .cloned()
                .ok_or_else(|| {
                    EduMindError::Tool(format!(
                        "slide deck `{id}` has no retained snapshot `{snapshot}`"
                    ))
                })?;
            deck.title = restored.title;
            deck.theme = restored.theme;
            deck.slides = restored.slides;
            Ok(())
        })
    }

    /// Returns deterministic overflow diagnostics for all slides in a deck.
    pub fn check_overflow(&self, id: &str) -> Result<Vec<SlideOverflow>> {
        Ok(collect_overflow(&self.required_deck(id)?))
    }

    /// Validates deck structure and text budgets before external export.
    pub fn check_deck(&self, id: &str) -> Result<SlideCheck> {
        let deck = self.required_deck(id)?;
        let mut issues = Vec::new();
        if deck.title.trim().is_empty() {
            issues.push("deck title is empty".to_owned());
        }
        if deck.slides.is_empty() {
            issues.push("deck has no slides".to_owned());
        }
        let mut slide_ids = HashSet::new();
        for (index, slide) in deck.slides.iter().enumerate() {
            if slide.title.trim().is_empty() {
                issues.push(format!("slide {} has an empty title", index + 1));
            }
            if !slide_ids.insert(&slide.id) {
                issues.push(format!("slide {} reuses a slide ID", index + 1));
            }
        }
        let overflow = collect_overflow(&deck);
        Ok(SlideCheck {
            valid: issues.is_empty() && overflow.is_empty(),
            issues,
            overflow,
        })
    }

    /// Renders a single slide as a local SVG preview without launching a browser process.
    pub fn screenshot_slide(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        slide: usize,
    ) -> Result<SlideRender> {
        let deck = self.required_deck(id)?;
        let slide_data = deck
            .slides
            .get(
                slide
                    .checked_sub(1)
                    .ok_or_else(|| EduMindError::Tool("slide numbers begin at 1".to_owned()))?,
            )
            .ok_or_else(|| {
                EduMindError::Tool(format!("slide `{slide}` does not exist in deck `{id}`"))
            })?;
        let destination = self
            .root
            .join("slides")
            .join(id)
            .join(format!("slide-{slide}.svg"));
        let output_path = sandbox
            .write(
                destination,
                render_slide_svg(&deck, slide, slide_data).as_bytes(),
            )?
            .display()
            .to_string();
        Ok(SlideRender {
            deck_id: id.to_owned(),
            output_path,
        })
    }

    /// Renders a local SVG thumbnail grid for a deck.
    pub fn thumbnail_grid(&self, sandbox: &WriteSandbox, id: &str) -> Result<SlideRender> {
        let deck = self.required_deck(id)?;
        let destination = self.root.join("slides").join(id).join("thumbnails.svg");
        let output_path = sandbox
            .write(destination, render_thumbnail_grid(&deck).as_bytes())?
            .display()
            .to_string();
        Ok(SlideRender {
            deck_id: id.to_owned(),
            output_path,
        })
    }

    /// Returns JSON ready for a configured external PPTX converter.
    pub fn deck_json(&self, id: &str) -> Result<Vec<u8>> {
        serde_json::to_vec_pretty(&self.required_deck(id)?).map_err(EduMindError::from)
    }

    fn mutate_deck<F>(
        &self,
        sandbox: &WriteSandbox,
        id: &str,
        now: DateTime<Utc>,
        mutate: F,
    ) -> Result<SlideDeck>
    where
        F: FnOnce(&mut SlideDeck) -> Result<()>,
    {
        let mut state = self.lock()?;
        let mut next = state.clone();
        let deck = next
            .decks
            .get_mut(id)
            .ok_or_else(|| EduMindError::Tool(format!("slide deck `{id}` does not exist")))?;
        mutate(deck)?;
        advance_deck(deck, now)?;
        let updated = deck.clone();
        self.write_deck_and_manifest(sandbox, &updated, &next)?;
        *state = next;
        Ok(updated)
    }

    fn required_document(&self, id: &str) -> Result<DocumentArtifact> {
        self.document(id)?
            .ok_or_else(|| EduMindError::Tool(format!("document `{id}` does not exist")))
    }

    fn required_deck(&self, id: &str) -> Result<SlideDeck> {
        self.deck(id)?
            .ok_or_else(|| EduMindError::Tool(format!("slide deck `{id}` does not exist")))
    }

    fn write_document_and_manifest(
        &self,
        sandbox: &WriteSandbox,
        document: &DocumentArtifact,
        manifest: &ArtifactManifest,
    ) -> Result<()> {
        sandbox.write(
            self.document_html_path(&document.id),
            render_document_html(document).as_bytes(),
        )?;
        self.persist(sandbox, manifest)
    }

    fn write_deck_and_manifest(
        &self,
        sandbox: &WriteSandbox,
        deck: &SlideDeck,
        manifest: &ArtifactManifest,
    ) -> Result<()> {
        sandbox.write(
            self.deck_html_path(&deck.id),
            render_deck_html(deck).as_bytes(),
        )?;
        self.persist(sandbox, manifest)
    }

    fn persist(&self, sandbox: &WriteSandbox, manifest: &ArtifactManifest) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(manifest)?;
        sandbox.write(self.root.join("runtime-artifacts.json"), &bytes)?;
        Ok(())
    }

    fn document_html_path(&self, id: &str) -> PathBuf {
        self.root.join("documents").join(format!("{id}.html"))
    }

    fn deck_html_path(&self, id: &str) -> PathBuf {
        self.root.join("slides").join(id).join("index.html")
    }

    fn lock(&self) -> Result<MutexGuard<'_, ArtifactManifest>> {
        self.state.lock().map_err(|error| {
            EduMindError::Tool(format!("runtime artifact store lock failed: {error}"))
        })
    }
}

fn normalize_title(value: String, kind: &str) -> Result<String> {
    let value = value.trim().to_owned();
    if value.is_empty() || value.chars().count() > TITLE_LIMIT {
        return Err(EduMindError::Tool(format!(
            "{kind} titles must contain 1-{TITLE_LIMIT} characters"
        )));
    }
    Ok(value)
}

fn normalize_theme(value: String) -> Result<String> {
    let theme = value.trim().to_ascii_lowercase();
    if matches!(theme.as_str(), "midnight" | "paper" | "ocean" | "forest") {
        Ok(theme)
    } else {
        Err(EduMindError::Tool(
            "slide themes must be one of midnight, paper, ocean, or forest".to_owned(),
        ))
    }
}

fn advance_document(document: &mut DocumentArtifact, now: DateTime<Utc>) -> Result<()> {
    document.version = document
        .version
        .checked_add(1)
        .ok_or_else(|| EduMindError::Tool("document revision counter overflowed".to_owned()))?;
    document.updated_at = now;
    document.history.push(document_version(document));
    trim_history(&mut document.history);
    Ok(())
}

fn document_version(document: &DocumentArtifact) -> DocumentVersion {
    DocumentVersion {
        version: document.version,
        title: document.title.clone(),
        content: document.content.clone(),
        updated_at: document.updated_at,
    }
}

fn advance_deck(deck: &mut SlideDeck, now: DateTime<Utc>) -> Result<()> {
    deck.version = deck
        .version
        .checked_add(1)
        .ok_or_else(|| EduMindError::Tool("slide deck revision counter overflowed".to_owned()))?;
    deck.updated_at = now;
    deck.history.push(deck_snapshot(deck));
    trim_history(&mut deck.history);
    Ok(())
}

fn deck_snapshot(deck: &SlideDeck) -> SlideDeckSnapshot {
    SlideDeckSnapshot {
        version: deck.version,
        title: deck.title.clone(),
        theme: deck.theme.clone(),
        slides: deck.slides.clone(),
        updated_at: deck.updated_at,
    }
}

fn trim_history<T>(history: &mut Vec<T>) {
    if history.len() > MAX_HISTORY {
        let excess = history.len() - MAX_HISTORY;
        history.drain(..excess);
    }
}

fn collect_overflow(deck: &SlideDeck) -> Vec<SlideOverflow> {
    let mut overflow = Vec::new();
    for (index, slide) in deck.slides.iter().enumerate() {
        let title_characters = slide.title.chars().count();
        if title_characters > SLIDE_TITLE_LIMIT {
            overflow.push(SlideOverflow {
                slide: index + 1,
                field: "title".to_owned(),
                characters: title_characters,
                limit: SLIDE_TITLE_LIMIT,
            });
        }
        let body_characters = slide.body.chars().count();
        if body_characters > SLIDE_BODY_LIMIT {
            overflow.push(SlideOverflow {
                slide: index + 1,
                field: "body".to_owned(),
                characters: body_characters,
                limit: SLIDE_BODY_LIMIT,
            });
        }
    }
    overflow
}

fn render_document_html(document: &DocumentArtifact) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><style>body{{margin:0;background:#f7f7fb;color:#1c1b25;font-family:system-ui,sans-serif}}main{{max-width:920px;margin:48px auto;padding:48px;background:#fff;border-radius:18px;box-shadow:0 12px 40px #1c1b2520}}h1{{margin-top:0}}pre{{white-space:pre-wrap;font:inherit;line-height:1.65}}</style></head><body><main><h1>{}</h1><pre>{}</pre></main></body></html>",
        escape_html(&document.title),
        escape_html(&document.title),
        escape_html(&document.content)
    )
}

fn render_deck_html(deck: &SlideDeck) -> String {
    let (background, foreground, accent) = theme_colors(&deck.theme);
    let slides = deck
        .slides
        .iter()
        .enumerate()
        .map(|(index, slide)| {
            format!(
                "<section class=\"slide\"><span>{}/{}</span><h2>{}</h2><p>{}</p></section>",
                index + 1,
                deck.slides.len(),
                escape_html(&slide.title),
                escape_html(&slide.body)
            )
        })
        .collect::<String>();
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><style>body{{margin:0;background:{};color:{};font-family:system-ui,sans-serif}}.slide{{box-sizing:border-box;min-height:100vh;padding:12vh 14vw;border-bottom:1px solid {}44}}span{{color:{};font-weight:700}}h2{{font-size:clamp(2.4rem,6vw,5rem);max-width:16ch}}p{{font-size:clamp(1.1rem,2vw,1.6rem);line-height:1.65;max-width:58ch;white-space:pre-wrap}}</style></head><body>{}</body></html>",
        escape_html(&deck.title),
        background,
        foreground,
        accent,
        accent,
        slides
    )
}

fn render_slide_svg(deck: &SlideDeck, number: usize, slide: &Slide) -> String {
    let (background, foreground, accent) = theme_colors(&deck.theme);
    let body_lines = wrapped_lines(&slide.body, 52, 7);
    let body = body_lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            format!(
                "<text x=\"88\" y=\"{}\" fill=\"{}\" font-size=\"30\">{}</text>",
                310 + index * 44,
                foreground,
                escape_html(line)
            )
        })
        .collect::<String>();
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1600\" height=\"900\" viewBox=\"0 0 1600 900\"><rect width=\"1600\" height=\"900\" fill=\"{}\"/><text x=\"88\" y=\"110\" fill=\"{}\" font-family=\"Arial,sans-serif\" font-size=\"28\" font-weight=\"700\">{} · {}/{}</text><text x=\"88\" y=\"230\" fill=\"{}\" font-family=\"Arial,sans-serif\" font-size=\"72\" font-weight=\"700\">{}</text>{}</svg>",
        background,
        accent,
        escape_html(&deck.title),
        number,
        deck.slides.len(),
        foreground,
        escape_html(&slide.title),
        body
    )
}

fn render_thumbnail_grid(deck: &SlideDeck) -> String {
    let columns = 3usize;
    let rows = deck.slides.len().div_ceil(columns).max(1);
    let height = rows * 290 + 80;
    let cards = deck
        .slides
        .iter()
        .enumerate()
        .map(|(index, slide)| {
            let column = index % columns;
            let row = index / columns;
            let x = 40 + column * 400;
            let y = 40 + row * 290;
            format!(
                "<g transform=\"translate({x},{y})\"><rect width=\"360\" height=\"230\" rx=\"16\" fill=\"#111827\"/><text x=\"28\" y=\"48\" fill=\"#7dd3fc\" font-size=\"18\">{}</text><text x=\"28\" y=\"105\" fill=\"#f9fafb\" font-size=\"28\" font-weight=\"700\">{}</text></g>",
                index + 1,
                escape_html(&truncate_chars(&slide.title, 28))
            )
        })
        .collect::<String>();
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1280\" height=\"{}\" viewBox=\"0 0 1280 {}\"><rect width=\"1280\" height=\"{}\" fill=\"#f3f4f6\"/>{}</svg>",
        height, height, height, cards
    )
}

fn theme_colors(theme: &str) -> (&'static str, &'static str, &'static str) {
    match theme {
        "paper" => ("#fcfcf8", "#1f2937", "#2563eb"),
        "ocean" => ("#082f49", "#e0f2fe", "#38bdf8"),
        "forest" => ("#052e16", "#dcfce7", "#4ade80"),
        _ => ("#111827", "#f9fafb", "#a78bfa"),
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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

fn truncate_chars(value: &str, maximum: usize) -> String {
    let mut result = value.chars().take(maximum).collect::<String>();
    if value.chars().count() > maximum {
        result.push('…');
    }
    result
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use chrono::Utc;
    use uuid::Uuid;

    use super::RuntimeArtifactStore;
    use crate::{config::types::SecurityConfig, security::WriteSandbox};

    #[test]
    fn documents_preserve_and_restore_versions() {
        let base = env::temp_dir().join(format!("edumind-artifacts-{}", Uuid::new_v4()));
        let root = base.join("OUTPUT");
        let security = SecurityConfig {
            allowed_tool_write_roots: vec![root.clone()],
            ..SecurityConfig::default()
        };
        let sandbox = WriteSandbox::from_config(&security);
        let store = RuntimeArtifactStore::open(&root).unwrap();
        let created = store
            .create_document(&sandbox, "Plan", "first", Utc::now())
            .unwrap();
        let updated = store
            .modify_document(
                &sandbox,
                &created.id,
                None,
                Some("second".to_owned()),
                Utc::now(),
            )
            .unwrap();
        let restored = store
            .restore_document(&sandbox, &updated.id, 1, Utc::now())
            .unwrap();

        assert_eq!(restored.content, "first");
        assert!(
            root.join("documents")
                .join(format!("{}.html", created.id))
                .exists()
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn slide_checks_report_long_content() {
        let base = env::temp_dir().join(format!("edumind-slides-{}", Uuid::new_v4()));
        let root = base.join("OUTPUT");
        let security = SecurityConfig {
            allowed_tool_write_roots: vec![root.clone()],
            ..SecurityConfig::default()
        };
        let sandbox = WriteSandbox::from_config(&security);
        let store = RuntimeArtifactStore::open(&root).unwrap();
        let deck = store
            .create_deck(&sandbox, "Review", "x".repeat(700), Utc::now())
            .unwrap();

        assert!(!store.check_deck(&deck.id).unwrap().valid);
        fs::remove_dir_all(base).unwrap();
    }
}
