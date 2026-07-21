use std::path::PathBuf;

use thiserror::Error;

/// The application-wide error type for recoverable EduMind failures.
#[derive(Debug, Error)]
pub enum EduMindError {
    #[error("agent operation failed: {0}")]
    Agent(String),
    #[error("blocking task failed: {0}")]
    BlockingTask(String),
    #[error("failed to read configuration {path}: {source}")]
    ConfigIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse configuration {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("configuration validation failed: {0}")]
    ConfigValidation(String),
    #[error("configuration watch failed: {0}")]
    ConfigWatch(String),
    #[error("database operation failed: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("failed to migrate {database} to schema v{version} ({name}): {source}")]
    DatabaseMigration {
        database: String,
        version: u32,
        name: String,
        #[source]
        source: rusqlite::Error,
    },
    #[error("gateway operation failed: {0}")]
    Gateway(String),
    #[error("channel operation failed: {0}")]
    Channel(String),
    #[error("collaboration operation failed: {0}")]
    Collaboration(String),
    #[error("environment variable `{0}` is required by the configuration")]
    MissingEnvironmentVariable(String),
    #[error("the home directory is unavailable while expanding `~`")]
    HomeDirectoryUnavailable,
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid embedding: {0}")]
    InvalidEmbedding(String),
    #[error("invalid memory query: {0}")]
    InvalidMemoryQuery(String),
    #[error("invalid data stored in memory: {0}")]
    InvalidStoredData(String),
    #[error("JSON operation failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("memory store lock failed: {0}")]
    MemoryLock(String),
    #[error("guarded process failed: {0}")]
    Process(String),
    #[error("routing operation failed: {0}")]
    Routing(String),
    #[error("research operation failed: {0}")]
    Research(String),
    #[error("scheduler operation failed: {0}")]
    Scheduler(String),
    #[error("study operation failed: {0}")]
    Study(String),
    #[error("student page operation failed: {0}")]
    Student(String),
    #[error("security policy failed: {0}")]
    Security(String),
    #[error("failed to prepare storage path {path}: {source}")]
    StorageIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("tool operation failed: {0}")]
    Tool(String),
}

/// Convenience result type used by EduMind runtime modules.
pub type Result<T> = std::result::Result<T, EduMindError>;
