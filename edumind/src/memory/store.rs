use std::{
    fmt, fs,
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    infra::{EduMindError, Result, SqliteMigration, apply_sqlite_migrations},
    memory::Embedding,
};

const RECORD_COLUMNS: &str =
    "id, module_id, content, content_type, metadata_json, created_at, updated_at";
const JOINED_RECORD_COLUMNS: &str =
    "e.id, e.module_id, e.content, e.content_type, e.metadata_json, e.created_at, e.updated_at";

/// Stable identifier for a persisted memory record.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MemoryId(pub Uuid);

impl MemoryId {
    /// Creates a new random memory identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parses a memory identifier supplied by a gateway or agent request.
    pub fn parse(value: &str) -> Result<Self> {
        Uuid::parse_str(value).map(Self).map_err(|error| {
            EduMindError::InvalidStoredData(format!("invalid memory id `{value}`: {error}"))
        })
    }
}

impl Default for MemoryId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for MemoryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Durable local memory record indexed by lexical and vector retrieval.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: MemoryId,
    pub module_id: String,
    pub content: String,
    pub content_type: String,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input used when creating or replacing the content of a memory record.
#[derive(Clone, Debug, PartialEq)]
pub struct NewMemory {
    pub module_id: String,
    pub content: String,
    pub content_type: String,
    pub metadata: Value,
}

impl NewMemory {
    /// Creates a memory input with an empty JSON-object metadata payload.
    #[must_use]
    pub fn new(
        module_id: impl Into<String>,
        content: impl Into<String>,
        content_type: impl Into<String>,
    ) -> Self {
        Self {
            module_id: module_id.into(),
            content: content.into(),
            content_type: content_type.into(),
            metadata: Value::Object(serde_json::Map::new()),
        }
    }
}

/// A lexical full-text search result with normalized BM25 relevance.
#[derive(Clone, Debug, PartialEq)]
pub struct LexicalSearchHit {
    pub record: MemoryRecord,
    pub score: f32,
}

/// A vector embedding persisted alongside its source memory record.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredEmbedding {
    pub memory_id: MemoryId,
    pub embedding: Embedding,
    pub updated_at: DateTime<Utc>,
}

/// SQLite-backed local memory store with FTS5 and persisted embedding values.
#[derive(Clone)]
pub struct MemoryStore {
    connection: Arc<Mutex<Connection>>,
}

impl MemoryStore {
    /// Opens or creates a persistent memory database and applies idempotent migrations.
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

    /// Creates an isolated in-memory store for deterministic tests and ephemeral sessions.
    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut connection: Connection) -> Result<Self> {
        apply_sqlite_migrations(
            &mut connection,
            "memory database",
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA secure_delete = ON; PRAGMA busy_timeout = 5000;",
            &[SqliteMigration::new(
                1,
                "initial memory schema",
                "
            CREATE TABLE IF NOT EXISTS memory_entries (
                id TEXT PRIMARY KEY NOT NULL,
                module_id TEXT NOT NULL,
                content TEXT NOT NULL,
                content_type TEXT NOT NULL,
                metadata_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
                memory_id UNINDEXED,
                content,
                content_type,
                module_id
            );

            CREATE TABLE IF NOT EXISTS memory_embeddings (
                memory_id TEXT PRIMARY KEY NOT NULL REFERENCES memory_entries(id) ON DELETE CASCADE,
                model TEXT NOT NULL,
                dimensions INTEGER NOT NULL,
                vector_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS collaboration_sessions (
                id TEXT PRIMARY KEY NOT NULL,
                module_id TEXT NOT NULL,
                owner_id TEXT NOT NULL,
                state_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS collaboration_members (
                session_id TEXT NOT NULL REFERENCES collaboration_sessions(id) ON DELETE CASCADE,
                member_id TEXT NOT NULL,
                joined_at TEXT NOT NULL,
                PRIMARY KEY (session_id, member_id)
            );

            CREATE TABLE IF NOT EXISTS collaboration_events (
                id TEXT PRIMARY KEY NOT NULL,
                session_id TEXT NOT NULL REFERENCES collaboration_sessions(id) ON DELETE CASCADE,
                actor_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS srs_cards (
                id TEXT PRIMARY KEY NOT NULL,
                front TEXT NOT NULL,
                back TEXT NOT NULL,
                deck TEXT NOT NULL,
                due_at TEXT NOT NULL,
                interval_days INTEGER NOT NULL,
                ease_factor REAL NOT NULL,
                repetitions INTEGER NOT NULL,
                lapses INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_reviewed_at TEXT,
                UNIQUE (deck, front, back)
            );

            CREATE TABLE IF NOT EXISTS srs_reviews (
                id TEXT PRIMARY KEY NOT NULL,
                card_id TEXT NOT NULL REFERENCES srs_cards(id) ON DELETE CASCADE,
                rating INTEGER NOT NULL,
                previous_interval_days INTEGER NOT NULL,
                next_interval_days INTEGER NOT NULL,
                reviewed_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS student_page_state (
                page TEXT NOT NULL,
                key TEXT NOT NULL,
                value_json TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                source TEXT NOT NULL,
                deleted INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (page, key)
            );

            CREATE TABLE IF NOT EXISTS student_page_events (
                id TEXT PRIMARY KEY NOT NULL,
                page TEXT NOT NULL,
                event_type TEXT NOT NULL,
                key TEXT,
                metadata_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS student_page_indexes (
                page TEXT PRIMARY KEY NOT NULL,
                memory_id TEXT NOT NULL REFERENCES memory_entries(id) ON DELETE CASCADE,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS research_pipeline_runs (
                id TEXT PRIMARY KEY NOT NULL,
                topic TEXT NOT NULL,
                status TEXT NOT NULL,
                run_json TEXT NOT NULL,
                context_json TEXT NOT NULL,
                error TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                completed_at TEXT
            );

            CREATE TABLE IF NOT EXISTS research_pipeline_events (
                id TEXT PRIMARY KEY NOT NULL,
                run_id TEXT NOT NULL REFERENCES research_pipeline_runs(id) ON DELETE CASCADE,
                event_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS run_checkpoints (
                run_id TEXT NOT NULL,
                stage TEXT NOT NULL,
                attempt INTEGER NOT NULL,
                checkpoint_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (run_id, stage, attempt)
            );

            CREATE TABLE IF NOT EXISTS run_budgets (
                run_id TEXT PRIMARY KEY NOT NULL,
                budget_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS run_verifications (
                id TEXT PRIMARY KEY NOT NULL,
                run_id TEXT NOT NULL,
                stage TEXT NOT NULL,
                verification_json TEXT NOT NULL,
                verified_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS run_timeline_events (
                id TEXT PRIMARY KEY NOT NULL,
                run_id TEXT NOT NULL,
                event_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS hermes_cycles (
                id TEXT PRIMARY KEY NOT NULL,
                cycle_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS learning_refreshes (
                day TEXT PRIMARY KEY NOT NULL,
                generated_at TEXT NOT NULL,
                available_minutes INTEGER NOT NULL,
                module_memory_records INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS learning_refresh_metadata (
                day TEXT PRIMARY KEY NOT NULL REFERENCES learning_refreshes(day) ON DELETE CASCADE,
                planner_conflicts_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS learning_daily_snapshots (
                day TEXT NOT NULL REFERENCES learning_refreshes(day) ON DELETE CASCADE,
                concept_id TEXT NOT NULL,
                snapshot_json TEXT NOT NULL,
                recommendation_json TEXT NOT NULL,
                PRIMARY KEY (day, concept_id)
            );

            CREATE TABLE IF NOT EXISTS memory_private_envelopes (
                memory_id TEXT PRIMARY KEY NOT NULL REFERENCES memory_entries(id) ON DELETE CASCADE,
                classification TEXT NOT NULL,
                envelope_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS memory_secure_deletions (
                id TEXT PRIMARY KEY NOT NULL,
                memory_id TEXT NOT NULL,
                classification TEXT NOT NULL,
                reason TEXT NOT NULL,
                deleted_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS local_telemetry_events (
                id TEXT PRIMARY KEY NOT NULL,
                event_name TEXT NOT NULL,
                module_id TEXT NOT NULL,
                outcome TEXT NOT NULL,
                metadata_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_memory_entries_module_id ON memory_entries(module_id);
            CREATE INDEX IF NOT EXISTS idx_memory_entries_updated_at ON memory_entries(updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_collaboration_sessions_updated_at ON collaboration_sessions(updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_collaboration_events_session_created_at ON collaboration_events(session_id, created_at ASC);
            CREATE INDEX IF NOT EXISTS idx_srs_cards_due_at ON srs_cards(due_at ASC);
            CREATE INDEX IF NOT EXISTS idx_srs_cards_deck_due_at ON srs_cards(deck, due_at ASC);
            CREATE INDEX IF NOT EXISTS idx_srs_reviews_card_reviewed_at ON srs_reviews(card_id, reviewed_at DESC);
            CREATE INDEX IF NOT EXISTS idx_student_page_state_page_updated_at ON student_page_state(page, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_student_page_events_page_created_at ON student_page_events(page, created_at ASC);
            CREATE INDEX IF NOT EXISTS idx_research_pipeline_runs_updated_at ON research_pipeline_runs(updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_research_pipeline_events_run_created_at ON research_pipeline_events(run_id, created_at ASC);
            CREATE INDEX IF NOT EXISTS idx_run_checkpoints_run_created_at ON run_checkpoints(run_id, created_at ASC);
            CREATE INDEX IF NOT EXISTS idx_run_verifications_run_verified_at ON run_verifications(run_id, verified_at ASC);
            CREATE INDEX IF NOT EXISTS idx_run_timeline_events_run_created_at ON run_timeline_events(run_id, created_at ASC);
            CREATE INDEX IF NOT EXISTS idx_hermes_cycles_created_at ON hermes_cycles(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_learning_daily_snapshots_day ON learning_daily_snapshots(day, concept_id);
            CREATE INDEX IF NOT EXISTS idx_memory_secure_deletions_memory_id ON memory_secure_deletions(memory_id, deleted_at DESC);
            CREATE INDEX IF NOT EXISTS idx_local_telemetry_events_created_at ON local_telemetry_events(created_at DESC);
            ",
            )],
        )?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    /// Inserts a new record and indexes it in FTS5.
    pub fn store(&self, input: NewMemory, now: DateTime<Utc>) -> Result<MemoryRecord> {
        validate_memory_input(&input)?;
        let record = MemoryRecord {
            id: MemoryId::new(),
            module_id: input.module_id,
            content: input.content,
            content_type: input.content_type,
            metadata: input.metadata,
            created_at: now,
            updated_at: now,
        };
        let metadata = serde_json::to_string(&record.metadata)?;
        let timestamp = format_timestamp(now);
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO memory_entries (
                id, module_id, content, content_type, metadata_json, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.id.to_string(),
                record.module_id,
                record.content,
                record.content_type,
                metadata,
                timestamp,
                timestamp,
            ],
        )?;
        insert_fts_record(&connection, &record)?;
        Ok(record)
    }

    /// Fetches a record by ID.
    pub fn get(&self, memory_id: MemoryId) -> Result<Option<MemoryRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {RECORD_COLUMNS} FROM memory_entries WHERE id = ?1"
        ))?;
        let mut rows = statement.query(params![memory_id.to_string()])?;
        rows.next()?.map(record_from_row).transpose()
    }

    /// Lists all stored records in reverse update order.
    pub fn list(&self) -> Result<Vec<MemoryRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {RECORD_COLUMNS} FROM memory_entries ORDER BY updated_at DESC, id ASC"
        ))?;
        let mut rows = statement.query([])?;
        let mut records = Vec::new();
        while let Some(row) = rows.next()? {
            records.push(record_from_row(row)?);
        }
        Ok(records)
    }

    /// Replaces a record's searchable fields and refreshes the FTS5 copy.
    pub fn update(
        &self,
        memory_id: MemoryId,
        input: NewMemory,
        now: DateTime<Utc>,
    ) -> Result<Option<MemoryRecord>> {
        validate_memory_input(&input)?;
        let metadata = serde_json::to_string(&input.metadata)?;
        let timestamp = format_timestamp(now);
        let connection = self.connection()?;
        let updated = connection.execute(
            "UPDATE memory_entries
             SET module_id = ?1, content = ?2, content_type = ?3, metadata_json = ?4, updated_at = ?5
             WHERE id = ?6",
            params![
                input.module_id,
                input.content,
                input.content_type,
                metadata,
                timestamp,
                memory_id.to_string(),
            ],
        )?;
        if updated == 0 {
            return Ok(None);
        }
        let record = read_record_by_id(&connection, memory_id)?.ok_or_else(|| {
            EduMindError::InvalidStoredData(format!("record {memory_id} disappeared after update"))
        })?;
        connection.execute(
            "DELETE FROM memory_fts WHERE memory_id = ?1",
            params![memory_id.to_string()],
        )?;
        insert_fts_record(&connection, &record)?;
        Ok(Some(record))
    }

    /// Deletes a record, its FTS entry, and its embedding.
    pub fn delete(&self, memory_id: MemoryId) -> Result<bool> {
        let connection = self.connection()?;
        connection.execute(
            "DELETE FROM memory_fts WHERE memory_id = ?1",
            params![memory_id.to_string()],
        )?;
        Ok(connection.execute(
            "DELETE FROM memory_entries WHERE id = ?1",
            params![memory_id.to_string()],
        )? > 0)
    }

    /// Searches FTS5 using sanitized AND semantics and normalized BM25 scores.
    pub fn search_lexical(&self, query: &str, limit: usize) -> Result<Vec<LexicalSearchHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let query = to_fts_query(query);
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {JOINED_RECORD_COLUMNS}, bm25(memory_fts) AS lexical_rank
             FROM memory_fts
             JOIN memory_entries AS e ON e.id = memory_fts.memory_id
             WHERE memory_fts MATCH ?1
             ORDER BY lexical_rank ASC, e.id ASC
             LIMIT ?2"
        ))?;
        let mut rows = statement.query(params![query, limit as i64])?;
        let mut hits = Vec::new();
        while let Some(row) = rows.next()? {
            let rank: f64 = row.get("lexical_rank")?;
            hits.push(LexicalSearchHit {
                record: record_from_row(row)?,
                score: (1.0 / (1.0 + rank.abs())) as f32,
            });
        }
        Ok(hits)
    }

    /// Persists an embedding for an existing memory record.
    pub fn upsert_embedding(
        &self,
        memory_id: MemoryId,
        embedding: &Embedding,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let vector_json = serde_json::to_string(&embedding.values)?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO memory_embeddings (memory_id, model, dimensions, vector_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(memory_id) DO UPDATE SET
                 model = excluded.model,
                 dimensions = excluded.dimensions,
                 vector_json = excluded.vector_json,
                 updated_at = excluded.updated_at",
            params![
                memory_id.to_string(),
                embedding.model,
                embedding.dimensions() as i64,
                vector_json,
                format_timestamp(now),
            ],
        )?;
        Ok(())
    }

    /// Loads one stored embedding by memory ID.
    pub fn get_embedding(&self, memory_id: MemoryId) -> Result<Option<StoredEmbedding>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT memory_id, model, vector_json, updated_at
             FROM memory_embeddings WHERE memory_id = ?1",
        )?;
        let mut rows = statement.query(params![memory_id.to_string()])?;
        rows.next()?.map(stored_embedding_from_row).transpose()
    }

    /// Loads all persisted embeddings for exact-vector index reconstruction.
    pub fn list_embeddings(&self) -> Result<Vec<StoredEmbedding>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT memory_id, model, vector_json, updated_at
             FROM memory_embeddings ORDER BY memory_id ASC",
        )?;
        let mut rows = statement.query([])?;
        let mut embeddings = Vec::new();
        while let Some(row) = rows.next()? {
            embeddings.push(stored_embedding_from_row(row)?);
        }
        Ok(embeddings)
    }

    pub(crate) fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|error| EduMindError::MemoryLock(error.to_string()))
    }
}

fn insert_fts_record(connection: &Connection, record: &MemoryRecord) -> Result<()> {
    connection.execute(
        "INSERT INTO memory_fts (memory_id, content, content_type, module_id)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            record.id.to_string(),
            record.content,
            record.content_type,
            record.module_id,
        ],
    )?;
    Ok(())
}

fn read_record_by_id(connection: &Connection, memory_id: MemoryId) -> Result<Option<MemoryRecord>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {RECORD_COLUMNS} FROM memory_entries WHERE id = ?1"
    ))?;
    let mut rows = statement.query(params![memory_id.to_string()])?;
    rows.next()?.map(record_from_row).transpose()
}

fn record_from_row(row: &Row<'_>) -> Result<MemoryRecord> {
    let id = MemoryId::parse(&row.get::<_, String>("id")?)?;
    let metadata = serde_json::from_str(&row.get::<_, String>("metadata_json")?)?;
    Ok(MemoryRecord {
        id,
        module_id: row.get("module_id")?,
        content: row.get("content")?,
        content_type: row.get("content_type")?,
        metadata,
        created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
        updated_at: parse_timestamp(&row.get::<_, String>("updated_at")?)?,
    })
}

fn stored_embedding_from_row(row: &Row<'_>) -> Result<StoredEmbedding> {
    let memory_id = MemoryId::parse(&row.get::<_, String>("memory_id")?)?;
    let values = serde_json::from_str(&row.get::<_, String>("vector_json")?)?;
    Ok(StoredEmbedding {
        memory_id,
        embedding: Embedding::new(row.get::<_, String>("model")?, values)?,
        updated_at: parse_timestamp(&row.get::<_, String>("updated_at")?)?,
    })
}

fn validate_memory_input(input: &NewMemory) -> Result<()> {
    if input.module_id.trim().is_empty() {
        return Err(EduMindError::InvalidStoredData(
            "memory module_id must not be empty".to_owned(),
        ));
    }
    if input.content.trim().is_empty() {
        return Err(EduMindError::InvalidStoredData(
            "memory content must not be empty".to_owned(),
        ));
    }
    if input.content_type.trim().is_empty() {
        return Err(EduMindError::InvalidStoredData(
            "memory content_type must not be empty".to_owned(),
        ));
    }
    Ok(())
}

fn to_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| {
            term.chars()
                .filter(|character| character.is_alphanumeric() || *character == '_')
                .collect::<String>()
        })
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{term}\""))
        .collect::<Vec<_>>()
        .join(" AND ")
}

pub(crate) fn format_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339()
}

pub(crate) fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| {
            EduMindError::InvalidStoredData(format!("invalid timestamp `{value}`: {error}"))
        })
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use crate::memory::{Embedding, MemoryStore, NewMemory};

    fn timestamp() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap()
    }

    #[test]
    fn stores_updates_searches_and_deletes_memory() {
        let store = MemoryStore::in_memory().unwrap();
        let mut input = NewMemory::new("class-notes", "Calculus derivatives revision", "note");
        input.metadata = json!({"course": "MAT101"});
        let record = store.store(input, timestamp()).unwrap();

        let hits = store.search_lexical("calculus", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.id, record.id);
        assert!(hits[0].score > 0.0);

        let updated = store
            .update(
                record.id,
                NewMemory::new("class-notes", "Integration exercises", "note"),
                timestamp(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(updated.content, "Integration exercises");
        assert!(store.search_lexical("calculus", 5).unwrap().is_empty());
        assert!(store.delete(record.id).unwrap());
        assert!(store.get(record.id).unwrap().is_none());
    }

    #[test]
    fn persists_and_recovers_embeddings() {
        let store = MemoryStore::in_memory().unwrap();
        let record = store
            .store(
                NewMemory::new("research", "Graph neural networks", "paper"),
                timestamp(),
            )
            .unwrap();
        let embedding = Embedding::new("hash-v1", vec![0.5, -0.5]).unwrap();

        store
            .upsert_embedding(record.id, &embedding, timestamp())
            .unwrap();

        assert_eq!(
            store.get_embedding(record.id).unwrap().unwrap().embedding,
            embedding
        );
        assert_eq!(store.list_embeddings().unwrap().len(), 1);
    }
}
