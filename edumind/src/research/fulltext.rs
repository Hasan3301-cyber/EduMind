use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use chrono::{DateTime, SecondsFormat, Utc};
use edumind_core::{Citation, EvidenceSpan, GroundedAnswer, PaperMetadata, ResearchProjectId};
use reqwest::{Client, Url};
use rusqlite::{Connection, Row, params};
use serde::{Deserialize, Serialize};

use crate::{
    config::types::OcrConfig,
    infra::{EduMindError, Result, SqliteMigration, apply_sqlite_migrations, run_blocking},
    memory::TextEmbedder,
};

use super::{OcrMode, ProjectStore, analysis::truncate_chars, ocr_pdf_bytes};

/// Largest PDF accepted from a local path or HTTPS download.
pub const MAX_PDF_BYTES: usize = 25 * 1024 * 1024;
/// Default character budget for each indexed full-text chunk.
pub const DEFAULT_CHUNK_TARGET: usize = 1_200;
/// Default character overlap between adjacent indexed chunks.
pub const DEFAULT_CHUNK_OVERLAP: usize = 200;

const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(45);
const PDF_HEADER_SCAN_BYTES: usize = 1_024;

/// Public metadata for one PDF ingested into a research project's full-text corpus.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FullTextDocument {
    pub project_id: ResearchProjectId,
    pub paper_id: String,
    pub title: String,
    pub source: String,
    #[serde(default)]
    pub ocr: bool,
    pub char_count: usize,
    pub chunk_count: usize,
    pub extracted_at: DateTime<Utc>,
}

/// Internal document representation that includes the persisted full text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredFullText {
    pub document: FullTextDocument,
    pub full_text: String,
}

/// Validated input used to replace one paper's indexed full text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewFullTextDocument {
    pub project_id: ResearchProjectId,
    pub paper_id: String,
    pub title: String,
    pub source: String,
    pub ocr: bool,
    pub full_text: String,
    pub extracted_at: DateTime<Utc>,
}

/// One chunk before it is scoped to a particular persisted document.
#[derive(Clone, Debug, PartialEq)]
pub struct EmbeddedTextChunk {
    pub index: usize,
    pub text: String,
    pub embedding: Vec<f32>,
}

/// One persisted full-text chunk, including its embedding for local RAG retrieval.
#[derive(Clone, Debug, PartialEq)]
pub struct FullTextChunk {
    pub project_id: ResearchProjectId,
    pub paper_id: String,
    pub title: String,
    pub chunk_index: usize,
    pub text: String,
    pub embedding: Vec<f32>,
}

/// One cited passage selected from an ingested paper body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeepPassage {
    pub paper_id: String,
    pub title: String,
    pub chunk_index: usize,
    pub score: f64,
    pub text: String,
}

/// A deterministic answer grounded in passages from an ingested project corpus.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeepAnswer {
    pub project_id: ResearchProjectId,
    pub question: String,
    pub answer: String,
    #[serde(default)]
    pub passages: Vec<DeepPassage>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub grounded_answer: Option<GroundedAnswer>,
}

/// Request payload for ingesting a local PDF, HTTPS PDF, or a paper's known open-access URL.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct FullTextIngestRequest {
    #[serde(default, alias = "paperId")]
    pub paper_id: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub ocr: OcrMode,
}

/// SQLite-backed full-text corpus stored independently from project metadata.
#[derive(Clone)]
pub struct FullTextStore {
    connection: Arc<Mutex<Connection>>,
}

/// Coordinates bounded PDF intake, extraction, chunk embedding, and deep project Q&A.
#[derive(Clone)]
pub struct FullTextResearchService {
    projects: ProjectStore,
    store: FullTextStore,
    embedder: Arc<dyn TextEmbedder>,
    client: Client,
    ocr_config: OcrConfig,
}

impl FullTextResearchService {
    /// Creates a full-text service with HTTPS-only downloads and bounded request timeouts.
    pub fn new(
        projects: ProjectStore,
        store: FullTextStore,
        embedder: Arc<dyn TextEmbedder>,
    ) -> Result<Self> {
        Self::with_ocr_config(projects, store, embedder, OcrConfig::default())
    }

    /// Creates a full-text service with explicit OCR behavior for scanned PDFs.
    pub fn with_ocr_config(
        projects: ProjectStore,
        store: FullTextStore,
        embedder: Arc<dyn TextEmbedder>,
        ocr_config: OcrConfig,
    ) -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
            .timeout(DOWNLOAD_TIMEOUT)
            .user_agent("EduMind research full-text ingestion")
            .build()?;
        Ok(Self {
            projects,
            store,
            embedder,
            client,
            ocr_config,
        })
    }

    /// Returns the durable full-text store used by this service.
    #[must_use]
    pub fn store(&self) -> FullTextStore {
        self.store.clone()
    }

    /// Ingests one project's paper PDF and replaces its prior text/chunk index if present.
    pub async fn ingest(
        &self,
        project_id: ResearchProjectId,
        request: FullTextIngestRequest,
        now: DateTime<Utc>,
    ) -> Result<Option<FullTextDocument>> {
        let Some(project) = self.projects.get(project_id)? else {
            return Ok(None);
        };
        let (paper, source, title) = resolve_ingest_target(&project.papers, &request)?;
        let bytes = self.read_source(&source).await?;
        let (full_text, ocr) = self.extract_best_text(bytes, request.ocr).await?;
        let chunks = embed_chunks(self.embedder.as_ref(), &full_text).await?;
        let input = NewFullTextDocument {
            project_id,
            paper_id: paper.id.clone(),
            title,
            source,
            ocr,
            full_text,
            extracted_at: now,
        };
        self.store.store_document(&input, &chunks).map(Some)
    }

    /// Answers a question strictly from semantically ranked passages in an ingested corpus.
    pub async fn deep_ask(
        &self,
        project_id: ResearchProjectId,
        question: impl Into<String>,
        limit: usize,
    ) -> Result<Option<DeepAnswer>> {
        let question = question.into().trim().to_owned();
        if question.is_empty() {
            return Err(EduMindError::Research(
                "deep project questions require non-empty text".to_owned(),
            ));
        }
        if self.projects.get(project_id)?.is_none() {
            return Ok(None);
        }
        let documents = self.store.list_documents(project_id)?;
        if documents.is_empty() {
            let answer =
                "This project has no ingested PDFs, so it has no full-text evidence to answer from."
                    .to_owned();
            return Ok(Some(DeepAnswer {
                project_id,
                question,
                grounded_answer: Some(GroundedAnswer::insufficient_evidence(answer.clone())),
                answer,
                passages: Vec::new(),
                warnings: vec![
                    "Ingest a local PDF or an HTTPS open-access PDF before using deep Q&A."
                        .to_owned(),
                ],
            }));
        }
        let passages = search_passages(
            &self.store,
            self.embedder.as_ref(),
            project_id,
            &question,
            limit.clamp(1, 20),
        )
        .await?;
        let answer = if passages.is_empty() {
            format!(
                "No indexed full-text passages matched `{question}` strongly enough to produce a grounded answer."
            )
        } else {
            let evidence = passages
                .iter()
                .map(|passage| {
                    format!(
                        "[{}#{}] {}",
                        passage.paper_id,
                        passage.chunk_index,
                        truncate_chars(&passage.text, 360)
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("Full-text evidence for `{question}`: {evidence}")
        };
        let grounded_answer = grounded_answer_from_passages(&answer, &passages)?;
        Ok(Some(DeepAnswer {
            project_id,
            question,
            answer,
            passages,
            warnings: Vec::new(),
            grounded_answer: Some(grounded_answer),
        }))
    }

    async fn read_source(&self, source: &str) -> Result<Vec<u8>> {
        let normalized = source.trim();
        let lower = normalized.to_ascii_lowercase();
        if lower.starts_with("https://") {
            return self.download_pdf(normalized).await;
        }
        if lower.contains("://") {
            return Err(EduMindError::Research(
                "full-text downloads must use HTTPS URLs".to_owned(),
            ));
        }
        let path = PathBuf::from(normalized);
        run_blocking(move || read_local_pdf(&path)).await
    }

    async fn extract_best_text(&self, bytes: Vec<u8>, mode: OcrMode) -> Result<(String, bool)> {
        let text_layer_bytes = bytes.clone();
        let text_layer = run_blocking(move || extract_pdf_text(&text_layer_bytes)).await;
        match mode {
            OcrMode::Off => text_layer.map(|text| (text, false)),
            OcrMode::Force => ocr_pdf_bytes(&self.ocr_config, bytes)
                .await
                .map(|text| (text, true)),
            OcrMode::Auto => {
                let text_layer_chars = text_layer.as_ref().map_or(0, |text| text.chars().count());
                if text_layer_chars >= self.ocr_config.min_text_chars || !self.ocr_config.enabled {
                    return text_layer.map(|text| (text, false));
                }
                match ocr_pdf_bytes(&self.ocr_config, bytes).await {
                    Ok(ocr_text) => match text_layer {
                        Ok(text_layer) if text_layer.chars().count() >= ocr_text.chars().count() => {
                            Ok((text_layer, false))
                        }
                        _ => Ok((ocr_text, true)),
                    },
                    Err(ocr_error) => text_layer.map(|text| (text, false)).map_err(|text_error| {
                        EduMindError::Research(format!(
                            "PDF has no usable text layer ({text_error}) and OCR could not recover it ({ocr_error})"
                        ))
                    }),
                }
            }
        }
    }

    async fn download_pdf(&self, source: &str) -> Result<Vec<u8>> {
        let url = Url::parse(source).map_err(|error| {
            EduMindError::Research(format!(
                "invalid full-text download URL `{source}`: {error}"
            ))
        })?;
        if url.scheme() != "https" {
            return Err(EduMindError::Research(
                "full-text downloads must use HTTPS URLs".to_owned(),
            ));
        }
        let mut response = self.client.get(url).send().await?.error_for_status()?;
        if let Some(length) = response.content_length() {
            let max_bytes = u64::try_from(MAX_PDF_BYTES).map_err(|error| {
                EduMindError::Research(format!("PDF size cap conversion failed: {error}"))
            })?;
            if length > max_bytes {
                return Err(EduMindError::Research(format!(
                    "downloaded PDF exceeds the {} MiB size cap",
                    MAX_PDF_BYTES / (1024 * 1024)
                )));
            }
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            let next_length = bytes.len().checked_add(chunk.len()).ok_or_else(|| {
                EduMindError::Research(
                    "downloaded PDF size overflowed the configured cap".to_owned(),
                )
            })?;
            if next_length > MAX_PDF_BYTES {
                return Err(EduMindError::Research(format!(
                    "downloaded PDF exceeds the {} MiB size cap",
                    MAX_PDF_BYTES / (1024 * 1024)
                )));
            }
            bytes.extend_from_slice(&chunk);
        }
        validate_pdf_bytes(&bytes)?;
        Ok(bytes)
    }
}

fn grounded_answer_from_passages(answer: &str, passages: &[DeepPassage]) -> Result<GroundedAnswer> {
    let supported = passages
        .iter()
        .filter(|passage| !passage.text.trim().is_empty())
        .collect::<Vec<_>>();
    if supported.is_empty() {
        return Ok(GroundedAnswer::insufficient_evidence(answer));
    }
    let citations = supported
        .iter()
        .map(|passage| Citation {
            source_id: format!("{}#{}", passage.paper_id, passage.chunk_index),
            title: Some(passage.title.clone()),
            locator: Some(format!("chunk {}", passage.chunk_index)),
        })
        .collect::<Vec<_>>();
    let evidence = supported
        .iter()
        .map(|passage| {
            EvidenceSpan::new(
                format!("{}#{}", passage.paper_id, passage.chunk_index),
                0,
                passage.text.len(),
                passage.text.clone(),
            )
        })
        .collect::<Vec<_>>();
    GroundedAnswer::grounded(answer, citations, evidence, 0.75)
        .map_err(|error| EduMindError::Research(format!("invalid full-text evidence: {}", error)))
}

impl FullTextStore {
    /// Opens or creates the dedicated full-text database and applies its schema.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|source| EduMindError::StorageIo {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        Self::from_connection(Connection::open(path)?)
    }

    /// Creates an isolated in-memory full-text corpus for deterministic tests.
    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    /// Returns whether a particular project paper already has indexed full text.
    pub fn has_document(&self, project_id: ResearchProjectId, paper_id: &str) -> Result<bool> {
        let connection = self.connection()?;
        let exists: i64 = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM fulltext_docs WHERE project_id = ?1 AND paper_id = ?2
             )",
            params![project_id.to_string(), paper_id],
            |row| row.get(0),
        )?;
        Ok(exists != 0)
    }

    /// Replaces a paper's complete text and chunk index atomically.
    pub fn store_document(
        &self,
        input: &NewFullTextDocument,
        chunks: &[EmbeddedTextChunk],
    ) -> Result<FullTextDocument> {
        validate_document_input(input, chunks)?;
        let document = FullTextDocument {
            project_id: input.project_id,
            paper_id: input.paper_id.clone(),
            title: input.title.clone(),
            source: input.source.clone(),
            ocr: input.ocr,
            char_count: input.full_text.chars().count(),
            chunk_count: chunks.len(),
            extracted_at: input.extracted_at,
        };
        let char_count = database_integer(document.char_count, "full-text character count")?;
        let chunk_count = database_integer(document.chunk_count, "full-text chunk count")?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO fulltext_docs (
                project_id, paper_id, title, source, ocr, char_count, chunk_count, full_text, extracted_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(project_id, paper_id) DO UPDATE SET
                title = excluded.title,
                source = excluded.source,
                ocr = excluded.ocr,
                char_count = excluded.char_count,
                chunk_count = excluded.chunk_count,
                full_text = excluded.full_text,
                extracted_at = excluded.extracted_at",
            params![
                document.project_id.to_string(),
                &document.paper_id,
                &document.title,
                &document.source,
                i64::from(document.ocr),
                char_count,
                chunk_count,
                &input.full_text,
                format_timestamp(document.extracted_at),
            ],
        )?;
        transaction.execute(
            "DELETE FROM fulltext_chunks WHERE project_id = ?1 AND paper_id = ?2",
            params![document.project_id.to_string(), &document.paper_id],
        )?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO fulltext_chunks (
                    project_id, paper_id, chunk_index, text, embedding
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for chunk in chunks {
                statement.execute(params![
                    document.project_id.to_string(),
                    &document.paper_id,
                    database_integer(chunk.index, "full-text chunk index")?,
                    &chunk.text,
                    encode_embedding(&chunk.embedding)?,
                ])?;
            }
        }
        transaction.commit()?;
        Ok(document)
    }

    /// Lists document metadata without returning potentially large paper bodies.
    pub fn list_documents(&self, project_id: ResearchProjectId) -> Result<Vec<FullTextDocument>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT paper_id, title, source, ocr, char_count, chunk_count, extracted_at
             FROM fulltext_docs
             WHERE project_id = ?1
             ORDER BY extracted_at DESC, paper_id ASC",
        )?;
        let mut rows = statement.query(params![project_id.to_string()])?;
        let mut documents = Vec::new();
        while let Some(row) = rows.next()? {
            documents.push(document_from_row(row, project_id)?);
        }
        Ok(documents)
    }

    /// Returns full text only for internal deep reading and supervision workflows.
    pub fn full_texts(&self, project_id: ResearchProjectId) -> Result<Vec<StoredFullText>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT paper_id, title, source, ocr, char_count, chunk_count, extracted_at, full_text
             FROM fulltext_docs
             WHERE project_id = ?1
             ORDER BY extracted_at DESC, paper_id ASC",
        )?;
        let mut rows = statement.query(params![project_id.to_string()])?;
        let mut documents = Vec::new();
        while let Some(row) = rows.next()? {
            let document = document_from_row(row, project_id)?;
            let full_text: String = row.get(7)?;
            documents.push(StoredFullText {
                document,
                full_text,
            });
        }
        Ok(documents)
    }

    /// Returns all indexed chunks for one project in stable paper/chunk order.
    pub fn all_chunks(&self, project_id: ResearchProjectId) -> Result<Vec<FullTextChunk>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT chunks.paper_id, docs.title, chunks.chunk_index, chunks.text, chunks.embedding
             FROM fulltext_chunks AS chunks
             JOIN fulltext_docs AS docs
                ON docs.project_id = chunks.project_id AND docs.paper_id = chunks.paper_id
             WHERE chunks.project_id = ?1
             ORDER BY chunks.paper_id ASC, chunks.chunk_index ASC",
        )?;
        let mut rows = statement.query(params![project_id.to_string()])?;
        let mut chunks = Vec::new();
        while let Some(row) = rows.next()? {
            chunks.push(chunk_from_row(row, project_id)?);
        }
        Ok(chunks)
    }

    fn from_connection(mut connection: Connection) -> Result<Self> {
        let mut schema = "
            CREATE TABLE IF NOT EXISTS fulltext_docs (
                project_id TEXT NOT NULL,
                paper_id TEXT NOT NULL,
                title TEXT NOT NULL,
                source TEXT NOT NULL,
                ocr INTEGER NOT NULL DEFAULT 0,
                char_count INTEGER NOT NULL,
                chunk_count INTEGER NOT NULL,
                full_text TEXT NOT NULL,
                extracted_at TEXT NOT NULL,
                PRIMARY KEY(project_id, paper_id)
            );

            CREATE TABLE IF NOT EXISTS fulltext_chunks (
                project_id TEXT NOT NULL,
                paper_id TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                text TEXT NOT NULL,
                embedding BLOB NOT NULL,
                PRIMARY KEY(project_id, paper_id, chunk_index),
                FOREIGN KEY(project_id, paper_id)
                    REFERENCES fulltext_docs(project_id, paper_id)
                    ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_fulltext_docs_project_extracted
                ON fulltext_docs(project_id, extracted_at DESC);
            CREATE INDEX IF NOT EXISTS idx_fulltext_chunks_project_paper
                ON fulltext_chunks(project_id, paper_id, chunk_index);
            "
        .to_owned();
        if fulltext_docs_needs_ocr_column(&connection)? {
            schema.push_str("ALTER TABLE fulltext_docs ADD COLUMN ocr INTEGER NOT NULL DEFAULT 0;");
        }
        apply_sqlite_migrations(
            &mut connection,
            "research full-text database",
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;",
            &[SqliteMigration::new(
                1,
                "initial research full-text schema",
                &schema,
            )],
        )?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection.lock().map_err(|error| {
            EduMindError::Research(format!("full-text store lock failed: {error}"))
        })
    }
}

/// Splits text into overlapping, UTF-8-safe character windows.
#[must_use]
pub fn chunk_text(text: &str, target: usize, overlap: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() || target == 0 {
        return Vec::new();
    }
    let overlap = overlap.min(target.saturating_sub(1));
    let stride = target - overlap;
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let end = advance_character_boundary(text, start, target);
        let chunk = text[start..end].trim();
        if !chunk.is_empty() {
            chunks.push(chunk.to_owned());
        }
        if end == text.len() {
            break;
        }
        start = advance_character_boundary(text, start, stride);
    }
    chunks
}

/// Extracts a PDF text layer after enforcing the configured in-memory size limit.
pub fn extract_pdf_text(bytes: &[u8]) -> Result<String> {
    validate_pdf_bytes(bytes)?;
    let extracted = pdf_extract::extract_text_from_mem(bytes)
        .map_err(|error| EduMindError::Research(format!("PDF text extraction failed: {error}")))?;
    let normalized = normalize_extracted_text(&extracted);
    if normalized.is_empty() {
        return Err(EduMindError::Research(
            "PDF contains no extractable text layer".to_owned(),
        ));
    }
    Ok(normalized)
}

/// Embeds default-sized full-text chunks using the configured local or remote provider.
pub async fn embed_chunks(
    embedder: &dyn TextEmbedder,
    text: &str,
) -> Result<Vec<EmbeddedTextChunk>> {
    embed_text_chunks(embedder, text, DEFAULT_CHUNK_TARGET, DEFAULT_CHUNK_OVERLAP).await
}

/// Returns the first HTTPS open-access or source URL suitable for automatic PDF retrieval.
#[must_use]
pub fn downloadable_url(paper: &PaperMetadata) -> Option<String> {
    [
        paper.open_access_url.as_deref(),
        paper.source_url.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .find(|candidate| is_https_url(candidate))
    .map(ToOwned::to_owned)
}

/// Performs local cosine retrieval over every indexed full-text chunk in a project.
pub async fn search_passages(
    store: &FullTextStore,
    embedder: &dyn TextEmbedder,
    project_id: ResearchProjectId,
    query: &str,
    limit: usize,
) -> Result<Vec<DeepPassage>> {
    let query = query.trim();
    if query.is_empty() {
        return Err(EduMindError::Research(
            "full-text passage search requires a non-empty query".to_owned(),
        ));
    }
    if limit == 0 {
        return Ok(Vec::new());
    }
    let query_embedding = embedder.embed(query).await?;
    let mut passages = store
        .all_chunks(project_id)?
        .into_iter()
        .filter_map(|chunk| {
            cosine_similarity(&query_embedding.values, &chunk.embedding).map(|score| DeepPassage {
                paper_id: chunk.paper_id,
                title: chunk.title,
                chunk_index: chunk.chunk_index,
                score,
                text: chunk.text,
            })
        })
        .collect::<Vec<_>>();
    passages.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.paper_id.cmp(&right.paper_id))
            .then_with(|| left.chunk_index.cmp(&right.chunk_index))
    });
    passages.truncate(limit);
    Ok(passages)
}

/// Derives the dedicated full-text database path beside the configured memory database.
#[must_use]
pub fn fulltext_database_path(memory_database_path: &Path) -> PathBuf {
    memory_database_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(
            || PathBuf::from("research_fulltext.db"),
            |parent| parent.join("research_fulltext.db"),
        )
}

fn resolve_ingest_target<'a>(
    papers: &'a [PaperMetadata],
    request: &FullTextIngestRequest,
) -> Result<(&'a PaperMetadata, String, String)> {
    let requested_paper_id = request
        .paper_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let requested_paper = requested_paper_id
        .map(|paper_id| {
            papers
                .iter()
                .find(|paper| paper.id == paper_id)
                .ok_or_else(|| {
                    EduMindError::Research(format!(
                        "research project does not contain paper `{paper_id}`"
                    ))
                })
        })
        .transpose()?;
    let source = request
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| requested_paper.and_then(downloadable_url))
        .ok_or_else(|| {
            EduMindError::Research(
                "full-text ingest requires a local/HTTPS source or a paper with an HTTPS open-access URL"
                    .to_owned(),
            )
        })?;
    let paper = requested_paper
        .or_else(|| {
            papers.iter().find(|paper| {
                paper.open_access_url.as_deref() == Some(source.as_str())
                    || paper.source_url.as_deref() == Some(source.as_str())
            })
        })
        .or_else(|| (papers.len() == 1).then(|| &papers[0]))
        .ok_or_else(|| {
            EduMindError::Research(
                "full-text ingest requires `paper_id` when a project has multiple papers"
                    .to_owned(),
            )
        })?;
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| paper.title.clone());
    Ok((paper, source, title))
}

async fn embed_text_chunks(
    embedder: &dyn TextEmbedder,
    text: &str,
    target: usize,
    overlap: usize,
) -> Result<Vec<EmbeddedTextChunk>> {
    let chunks = chunk_text(text, target, overlap);
    if chunks.is_empty() {
        return Err(EduMindError::Research(
            "full-text chunking produced no searchable text".to_owned(),
        ));
    }
    let mut embedded = Vec::with_capacity(chunks.len());
    for (index, text) in chunks.into_iter().enumerate() {
        let embedding = embedder.embed(&text).await?;
        embedded.push(EmbeddedTextChunk {
            index,
            text,
            embedding: embedding.values,
        });
    }
    Ok(embedded)
}

fn read_local_pdf(path: &Path) -> Result<Vec<u8>> {
    let mut file = File::open(path).map_err(|source| EduMindError::StorageIo {
        path: path.to_path_buf(),
        source,
    })?;
    let maximum = u64::try_from(MAX_PDF_BYTES).map_err(|error| {
        EduMindError::Research(format!("PDF size cap conversion failed: {error}"))
    })?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| EduMindError::StorageIo {
            path: path.to_path_buf(),
            source,
        })?;
    validate_pdf_bytes(&bytes)?;
    Ok(bytes)
}

fn validate_pdf_bytes(bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_PDF_BYTES {
        return Err(EduMindError::Research(format!(
            "PDF exceeds the {} MiB size cap",
            MAX_PDF_BYTES / (1024 * 1024)
        )));
    }
    if bytes.is_empty()
        || !bytes[..bytes.len().min(PDF_HEADER_SCAN_BYTES)]
            .windows(5)
            .any(|window| window == b"%PDF-")
    {
        return Err(EduMindError::Research(
            "full-text ingest accepts PDF content only".to_owned(),
        ));
    }
    Ok(())
}

fn normalize_extracted_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn advance_character_boundary(text: &str, start: usize, character_count: usize) -> usize {
    text[start..]
        .char_indices()
        .nth(character_count)
        .map_or(text.len(), |(offset, _)| start + offset)
}

fn validate_document_input(
    input: &NewFullTextDocument,
    chunks: &[EmbeddedTextChunk],
) -> Result<()> {
    for (name, value) in [
        ("paper ID", input.paper_id.as_str()),
        ("title", input.title.as_str()),
        ("source", input.source.as_str()),
        ("full text", input.full_text.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(EduMindError::Research(format!(
                "full-text document {name} must not be empty"
            )));
        }
    }
    if chunks.is_empty() {
        return Err(EduMindError::Research(
            "full-text documents require at least one embedded chunk".to_owned(),
        ));
    }
    for (expected_index, chunk) in chunks.iter().enumerate() {
        if chunk.index != expected_index || chunk.text.trim().is_empty() {
            return Err(EduMindError::Research(
                "full-text chunks must have contiguous indexes and non-empty text".to_owned(),
            ));
        }
        if chunk.embedding.is_empty() || chunk.embedding.iter().any(|value| !value.is_finite()) {
            return Err(EduMindError::Research(
                "full-text chunks require finite non-empty embeddings".to_owned(),
            ));
        }
    }
    Ok(())
}

fn encode_embedding(embedding: &[f32]) -> Result<Vec<u8>> {
    if embedding.is_empty() || embedding.iter().any(|value| !value.is_finite()) {
        return Err(EduMindError::Research(
            "full-text embeddings must be finite and non-empty".to_owned(),
        ));
    }
    Ok(embedding
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect())
}

fn decode_embedding(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(std::mem::size_of::<f32>()) {
        return Err(EduMindError::Research(
            "stored full-text embedding has an invalid byte length".to_owned(),
        ));
    }
    let embedding = bytes
        .chunks_exact(std::mem::size_of::<f32>())
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect::<Vec<_>>();
    if embedding.iter().any(|value| !value.is_finite()) {
        return Err(EduMindError::Research(
            "stored full-text embedding contains non-finite values".to_owned(),
        ));
    }
    Ok(embedding)
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f64> {
    if left.is_empty() || left.len() != right.len() {
        return None;
    }
    let (dot, left_norm, right_norm) = left.iter().zip(right).fold(
        (0.0_f64, 0.0_f64, 0.0_f64),
        |(dot, left_norm, right_norm), (left, right)| {
            let left = f64::from(*left);
            let right = f64::from(*right);
            (
                dot + left * right,
                left_norm + left * left,
                right_norm + right * right,
            )
        },
    );
    if left_norm == 0.0 || right_norm == 0.0 {
        return None;
    }
    let score = dot / (left_norm.sqrt() * right_norm.sqrt());
    score.is_finite().then_some(score)
}

fn is_https_url(value: &str) -> bool {
    Url::parse(value).is_ok_and(|url| url.scheme() == "https" && url.host_str().is_some())
}

fn database_integer(value: usize, name: &str) -> Result<i64> {
    i64::try_from(value)
        .map_err(|error| EduMindError::Research(format!("{name} exceeds SQLite range: {error}")))
}

fn document_from_row(
    row: &Row<'_>,
    project_id: ResearchProjectId,
) -> rusqlite::Result<FullTextDocument> {
    let ocr: i64 = row.get(3)?;
    let char_count: i64 = row.get(4)?;
    let chunk_count: i64 = row.get(5)?;
    let extracted_at: String = row.get(6)?;
    let char_count = usize::try_from(char_count).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })?;
    let chunk_count = usize::try_from(chunk_count).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })?;
    let extracted_at = DateTime::parse_from_rfc3339(&extracted_at)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?
        .with_timezone(&Utc);
    Ok(FullTextDocument {
        project_id,
        paper_id: row.get(0)?,
        title: row.get(1)?,
        source: row.get(2)?,
        ocr: ocr != 0,
        char_count,
        chunk_count,
        extracted_at,
    })
}

fn fulltext_docs_needs_ocr_column(connection: &Connection) -> Result<bool> {
    let mut statement = connection.prepare("PRAGMA table_info(fulltext_docs)")?;
    let mut rows = statement.query([])?;
    let mut has_columns = false;

    while let Some(row) = rows.next()? {
        has_columns = true;
        let name: String = row.get(1)?;
        if name == "ocr" {
            return Ok(false);
        }
    }

    Ok(has_columns)
}

fn chunk_from_row(row: &Row<'_>, project_id: ResearchProjectId) -> Result<FullTextChunk> {
    let chunk_index: i64 = row.get(2)?;
    let embedding: Vec<u8> = row.get(4)?;
    Ok(FullTextChunk {
        project_id,
        paper_id: row.get(0)?,
        title: row.get(1)?,
        chunk_index: usize::try_from(chunk_index).map_err(|error| {
            EduMindError::Research(format!("stored full-text chunk index is invalid: {error}"))
        })?,
        text: row.get(3)?,
        embedding: decode_embedding(&embedding)?,
    })
}

fn format_timestamp(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Micros, true)
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc};

    use chrono::TimeZone;
    use edumind_core::{PaperMetadata, ResearchProject};
    use uuid::Uuid;

    use super::{
        FullTextIngestRequest, FullTextResearchService, FullTextStore, NewFullTextDocument,
        OcrMode, chunk_text, downloadable_url, embed_text_chunks, extract_pdf_text,
        fulltext_database_path, search_passages,
    };
    use crate::{memory::HashEmbedder, research::ProjectStore};

    #[test]
    fn chunks_unicode_text_at_character_boundaries_with_overlap() {
        let chunks = chunk_text("αβγδεζηθ", 4, 1);

        assert_eq!(chunks, vec!["αβγδ", "δεζη", "ηθ"]);
    }

    #[test]
    fn extracts_text_from_a_small_real_pdf() {
        let extracted = extract_pdf_text(&minimal_pdf("Deep retrieval evidence")).unwrap();

        assert!(extracted.contains("Deep retrieval evidence"));
    }

    #[test]
    fn chooses_only_https_downloadable_paper_urls() {
        let paper = PaperMetadata {
            open_access_url: Some("http://example.test/blocked.pdf".to_owned()),
            source_url: Some("https://example.test/allowed.pdf".to_owned()),
            ..PaperMetadata::default()
        };

        assert_eq!(
            downloadable_url(&paper).as_deref(),
            Some("https://example.test/allowed.pdf")
        );
    }

    #[tokio::test]
    async fn ingests_persists_and_retrieves_local_pdf_passages() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap();
        let projects = ProjectStore::in_memory().unwrap();
        let mut project = ResearchProject::new("retrieval", now);
        project.add_papers(
            vec![PaperMetadata {
                id: "retrieval-paper".to_owned(),
                title: "Retrieval practice trial".to_owned(),
                ..PaperMetadata::default()
            }],
            now,
        );
        projects.save(&project).unwrap();
        let store = FullTextStore::in_memory().unwrap();
        let service = FullTextResearchService::new(
            projects,
            store.clone(),
            Arc::new(HashEmbedder::new(64).unwrap()),
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!("edumind-{}.pdf", Uuid::new_v4()));
        fs::write(
            &path,
            minimal_pdf("Retrieval practice improved long-term retention in the classroom trial."),
        )
        .unwrap();

        let result = service
            .ingest(
                project.id,
                FullTextIngestRequest {
                    paper_id: Some("retrieval-paper".to_owned()),
                    source: Some(path.to_string_lossy().into_owned()),
                    title: None,
                    ocr: OcrMode::Auto,
                },
                now,
            )
            .await;
        let _ = fs::remove_file(&path);
        let document = result.unwrap().unwrap();

        assert_eq!(document.paper_id, "retrieval-paper");
        assert!(!document.ocr);
        assert!(store.has_document(project.id, "retrieval-paper").unwrap());
        assert_eq!(store.list_documents(project.id).unwrap(), vec![document]);
        let answer = service
            .deep_ask(project.id, "What outcome improved?", 3)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(answer.passages[0].paper_id, "retrieval-paper");
        assert!(answer.answer.contains("[retrieval-paper#0]"));
    }

    #[tokio::test]
    async fn forced_ocr_reports_a_clear_error_when_disabled() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap();
        let projects = ProjectStore::in_memory().unwrap();
        let mut project = ResearchProject::new("scanned study", now);
        project.add_papers(
            vec![PaperMetadata {
                id: "scanned-paper".to_owned(),
                title: "Scanned study".to_owned(),
                ..PaperMetadata::default()
            }],
            now,
        );
        projects.save(&project).unwrap();
        let service = FullTextResearchService::new(
            projects,
            FullTextStore::in_memory().unwrap(),
            Arc::new(HashEmbedder::new(64).unwrap()),
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!("edumind-force-ocr-{}.pdf", Uuid::new_v4()));
        fs::write(
            &path,
            minimal_pdf("Visible text is not used in forced OCR mode."),
        )
        .unwrap();

        let result = service
            .ingest(
                project.id,
                FullTextIngestRequest {
                    paper_id: Some("scanned-paper".to_owned()),
                    source: Some(path.to_string_lossy().into_owned()),
                    title: None,
                    ocr: OcrMode::Force,
                },
                now,
            )
            .await;
        let _ = fs::remove_file(&path);

        assert!(result.unwrap_err().to_string().contains("OCR is disabled"));
    }

    #[tokio::test]
    async fn stores_little_endian_embeddings_and_ranks_matching_passages() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap();
        let project_id = edumind_core::ResearchProjectId::new();
        let store = FullTextStore::in_memory().unwrap();
        let embedder = HashEmbedder::new(64).unwrap();
        let text = "Retrieval practice improves retention. \n\nSpacing supports later recall.";
        let chunks = embed_text_chunks(&embedder, text, 45, 5).await.unwrap();
        let document = store
            .store_document(
                &NewFullTextDocument {
                    project_id,
                    paper_id: "paper-one".to_owned(),
                    title: "Retention study".to_owned(),
                    source: "local-test.pdf".to_owned(),
                    ocr: false,
                    full_text: text.to_owned(),
                    extracted_at: now,
                },
                &chunks,
            )
            .unwrap();
        let passages = search_passages(&store, &embedder, project_id, "retention", 2)
            .await
            .unwrap();

        assert_eq!(store.full_texts(project_id).unwrap()[0].full_text, text);
        assert_eq!(document.chunk_count, chunks.len());
        assert_eq!(passages[0].paper_id, "paper-one");
        assert!(passages[0].text.contains("retention"));
    }

    #[test]
    fn derives_fulltext_database_beside_memory_database() {
        assert_eq!(
            fulltext_database_path(std::path::Path::new("data/memory.db")),
            std::path::PathBuf::from("data/research_fulltext.db")
        );
    }

    fn minimal_pdf(text: &str) -> Vec<u8> {
        let stream = format!("BT\n/F1 18 Tf\n72 720 Td\n({text}) Tj\nET");
        let objects = vec![
            "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_owned(),
            "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n".to_owned(),
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>\nendobj\n".to_owned(),
            "4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n".to_owned(),
            format!(
                "5 0 obj\n<< /Length {} >>\nstream\n{stream}\nendstream\nendobj\n",
                stream.len()
            ),
        ];
        let mut bytes = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for object in objects {
            offsets.push(bytes.len());
            bytes.extend_from_slice(object.as_bytes());
        }
        let xref_offset = bytes.len();
        bytes.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
        for offset in offsets {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n")
                .as_bytes(),
        );
        bytes
    }
}
