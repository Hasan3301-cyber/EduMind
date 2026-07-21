use rusqlite::{Connection, TransactionBehavior};

use super::{EduMindError, Result};

/// One ordered SQLite schema migration applied exactly once per database file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SqliteMigration<'a> {
    pub version: u32,
    pub name: &'a str,
    pub sql: &'a str,
}

impl<'a> SqliteMigration<'a> {
    /// Declares an immutable schema migration.
    #[must_use]
    pub const fn new(version: u32, name: &'a str, sql: &'a str) -> Self {
        Self { version, name, sql }
    }
}

/// Applies ordered migrations in one immediate transaction and records `PRAGMA user_version`.
///
/// The setup statements run before the migration transaction because connection-level settings
/// such as WAL mode cannot be changed while a transaction is active. Existing unversioned
/// databases safely adopt version one when its migration uses idempotent schema statements.
pub fn apply_sqlite_migrations(
    connection: &mut Connection,
    database_name: &str,
    setup_sql: &str,
    migrations: &[SqliteMigration<'_>],
) -> Result<u32> {
    if migrations.is_empty() {
        return Err(EduMindError::InvalidStoredData(format!(
            "{database_name} has no declared schema migrations"
        )));
    }

    for (index, migration) in migrations.iter().enumerate() {
        let expected = u32::try_from(index + 1).map_err(|error| {
            EduMindError::InvalidStoredData(format!(
                "{database_name} migration index is invalid: {error}"
            ))
        })?;
        if migration.version != expected {
            return Err(EduMindError::InvalidStoredData(format!(
                "{database_name} migration `{}` has version {}, expected {expected}",
                migration.name, migration.version
            )));
        }
    }

    connection.execute_batch(setup_sql)?;
    let stored_version: i64 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let stored_version = u32::try_from(stored_version).map_err(|error| {
        EduMindError::InvalidStoredData(format!(
            "{database_name} has an invalid schema version: {error}"
        ))
    })?;
    let latest_version = migrations.last().map_or(0, |migration| migration.version);
    if stored_version > latest_version {
        return Err(EduMindError::InvalidStoredData(format!(
            "{database_name} schema version {stored_version} is newer than this application supports ({latest_version})"
        )));
    }
    if stored_version == latest_version {
        return Ok(stored_version);
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for migration in migrations
        .iter()
        .filter(|migration| migration.version > stored_version)
    {
        transaction.execute_batch(migration.sql).map_err(|source| {
            EduMindError::DatabaseMigration {
                database: database_name.to_owned(),
                version: migration.version,
                name: migration.name.to_owned(),
                source,
            }
        })?;
        transaction
            .pragma_update(None, "user_version", migration.version)
            .map_err(|source| EduMindError::DatabaseMigration {
                database: database_name.to_owned(),
                version: migration.version,
                name: migration.name.to_owned(),
                source,
            })?;
    }
    transaction.commit()?;
    Ok(latest_version)
}
