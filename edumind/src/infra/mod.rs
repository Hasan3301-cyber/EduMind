//! Shared infrastructure primitives used by all EduMind subsystems.

pub mod blocking;
pub mod error;
pub mod sqlite;
pub mod telemetry;

pub use blocking::run_blocking;
pub use error::{EduMindError, Result};
pub use sqlite::{SqliteMigration, apply_sqlite_migrations};
pub use telemetry::{LocalTelemetry, TelemetryEvent, TelemetryInput};
