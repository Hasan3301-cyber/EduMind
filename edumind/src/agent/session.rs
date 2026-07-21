use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    config::types::SessionConfig,
    infra::{EduMindError, Result, SqliteMigration, apply_sqlite_migrations},
};

/// Message roles persisted in an agent conversation session.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

impl ChatRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "system" => Ok(Self::System),
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            "tool" => Ok(Self::Tool),
            _ => Err(EduMindError::InvalidStoredData(format!(
                "invalid chat message role `{value}`"
            ))),
        }
    }
}

/// Durable session identity bound to one agent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub session_key: String,
    pub agent_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Durable individual chat message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionMessage {
    pub id: Uuid,
    pub role: ChatRole,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

/// SQLite-backed conversation manager with bounded, context-aware histories.
#[derive(Clone)]
pub struct SessionManager {
    connection: Arc<Mutex<Connection>>,
    max_messages: usize,
    max_age_minutes: u32,
}

impl SessionManager {
    /// Opens a durable session database and applies its idempotent schema migration.
    pub fn open(path: impl AsRef<Path>, config: &SessionConfig) -> Result<Self> {
        validate_limits(config)?;
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
        Self::from_connection(Connection::open(path)?, config)
    }

    /// Creates an isolated session manager for deterministic tests and temporary workflows.
    pub fn in_memory(config: &SessionConfig) -> Result<Self> {
        validate_limits(config)?;
        Self::from_connection(Connection::open_in_memory()?, config)
    }

    fn from_connection(mut connection: Connection, config: &SessionConfig) -> Result<Self> {
        apply_sqlite_migrations(
            &mut connection,
            "agent session database",
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;",
            &[SqliteMigration::new(
                1,
                "initial agent session schema",
                "
                CREATE TABLE IF NOT EXISTS agent_sessions (
                    session_key TEXT PRIMARY KEY NOT NULL,
                    agent_id TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS agent_session_messages (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_key TEXT NOT NULL REFERENCES agent_sessions(session_key) ON DELETE CASCADE,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_agent_session_messages_key_created
                    ON agent_session_messages(session_key, created_at ASC);
                CREATE INDEX IF NOT EXISTS idx_agent_sessions_updated_at
                    ON agent_sessions(updated_at DESC);
                ",
            )],
        )?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            max_messages: config.max_messages,
            max_age_minutes: config.max_age_minutes,
        })
    }

    /// Creates a session or returns its existing identity, which cannot change agents.
    pub fn get_or_create(
        &self,
        session_key: &str,
        agent_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Session> {
        validate_identifier("session key", session_key)?;
        validate_identifier("agent ID", agent_id)?;
        let connection = self.connection()?;
        self.prune_expired(&connection, now)?;
        let stored = connection
            .query_row(
                "SELECT session_key, agent_id, created_at, updated_at
                 FROM agent_sessions WHERE session_key = ?1",
                params![session_key],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        if let Some((stored_key, stored_agent, created_at, updated_at)) = stored {
            if stored_agent != agent_id {
                return Err(EduMindError::Agent(format!(
                    "session `{session_key}` is already bound to agent `{stored_agent}`"
                )));
            }
            return session_from_row(stored_key, stored_agent, created_at, updated_at);
        }
        let timestamp = format_timestamp(now);
        connection.execute(
            "INSERT INTO agent_sessions (session_key, agent_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![session_key, agent_id, timestamp, timestamp],
        )?;
        Ok(Session {
            session_key: session_key.to_owned(),
            agent_id: agent_id.to_owned(),
            created_at: now,
            updated_at: now,
        })
    }

    /// Appends a message and evicts the oldest history beyond the configured bound.
    pub fn append_message(
        &self,
        session_key: &str,
        role: ChatRole,
        content: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<SessionMessage> {
        validate_identifier("session key", session_key)?;
        let content = content.into();
        if content.trim().is_empty() {
            return Err(EduMindError::Agent(
                "session messages must not be empty".to_owned(),
            ));
        }
        let message = SessionMessage {
            id: Uuid::new_v4(),
            role,
            content,
            created_at: now,
        };
        let mut connection = self.connection()?;
        self.prune_expired(&connection, now)?;
        let transaction = connection.transaction()?;
        let exists = transaction
            .query_row(
                "SELECT 1 FROM agent_sessions WHERE session_key = ?1",
                params![session_key],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(EduMindError::Agent(format!(
                "session `{session_key}` does not exist"
            )));
        }
        let timestamp = format_timestamp(now);
        transaction.execute(
            "INSERT INTO agent_session_messages (id, session_key, role, content, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                message.id.to_string(),
                session_key,
                message.role.as_str(),
                message.content,
                timestamp,
            ],
        )?;
        transaction.execute(
            "UPDATE agent_sessions SET updated_at = ?2 WHERE session_key = ?1",
            params![session_key, timestamp],
        )?;
        transaction.execute(
            "DELETE FROM agent_session_messages
             WHERE id IN (
                 SELECT id FROM agent_session_messages
                 WHERE session_key = ?1
                 ORDER BY created_at DESC, rowid DESC
                 LIMIT -1 OFFSET ?2
             )",
            params![session_key, self.max_messages],
        )?;
        transaction.commit()?;
        Ok(message)
    }

    /// Returns one session, after removing expired records.
    pub fn get(&self, session_key: &str, now: DateTime<Utc>) -> Result<Option<Session>> {
        validate_identifier("session key", session_key)?;
        let connection = self.connection()?;
        self.prune_expired(&connection, now)?;
        let stored = connection
            .query_row(
                "SELECT session_key, agent_id, created_at, updated_at
                 FROM agent_sessions WHERE session_key = ?1",
                params![session_key],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        stored
            .map(|(key, agent, created, updated)| session_from_row(key, agent, created, updated))
            .transpose()
    }

    /// Lists persisted sessions in most-recently-active order.
    pub fn list(&self, now: DateTime<Utc>) -> Result<Vec<Session>> {
        let connection = self.connection()?;
        self.prune_expired(&connection, now)?;
        let mut statement = connection.prepare(
            "SELECT session_key, agent_id, created_at, updated_at
             FROM agent_sessions ORDER BY updated_at DESC, session_key ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(key, agent, created, updated)| session_from_row(key, agent, created, updated))
            .collect()
    }

    /// Returns persisted messages in chronological order.
    pub fn messages(&self, session_key: &str, now: DateTime<Utc>) -> Result<Vec<SessionMessage>> {
        validate_identifier("session key", session_key)?;
        let connection = self.connection()?;
        self.prune_expired(&connection, now)?;
        let mut statement = connection.prepare(
            "SELECT id, role, content, created_at
             FROM agent_session_messages
             WHERE session_key = ?1
             ORDER BY created_at ASC, rowid ASC",
        )?;
        let rows = statement.query_map(params![session_key], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(id, role, content, created_at)| message_from_row(id, role, content, created_at))
            .collect()
    }

    /// Builds a chronological context window that keeps the newest useful messages.
    pub fn context_for_model(
        &self,
        session_key: &str,
        context_window: u32,
        now: DateTime<Utc>,
    ) -> Result<Vec<SessionMessage>> {
        let token_budget = usize::try_from(context_window).map_err(|_| {
            EduMindError::Agent("model context window cannot fit this platform".to_owned())
        })?;
        if token_budget == 0 {
            return Err(EduMindError::Agent(
                "model context window must be greater than zero".to_owned(),
            ));
        }
        let mut selected = Vec::new();
        let mut used_tokens = 0_usize;
        for mut message in self.messages(session_key, now)?.into_iter().rev() {
            let estimated_tokens = estimate_tokens(&message.content);
            if used_tokens.saturating_add(estimated_tokens) <= token_budget {
                used_tokens = used_tokens.saturating_add(estimated_tokens);
                selected.push(message);
                continue;
            }
            if selected.is_empty() {
                let character_limit = token_budget.saturating_mul(4).saturating_sub(4);
                if character_limit > 0 {
                    message.content = truncate_to_tail(&message.content, character_limit);
                    selected.push(message);
                }
            }
            break;
        }
        selected.reverse();
        Ok(selected)
    }

    /// Creates a distinct child session after confirming the parent session exists.
    pub fn spawn(
        &self,
        parent_session_key: &str,
        session_key: &str,
        agent_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Session> {
        if parent_session_key == session_key {
            return Err(EduMindError::Agent(
                "child session key must differ from its parent session key".to_owned(),
            ));
        }
        if self.get(parent_session_key, now)?.is_none() {
            return Err(EduMindError::Agent(format!(
                "parent session `{parent_session_key}` does not exist"
            )));
        }
        self.get_or_create(session_key, agent_id, now)
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|error| EduMindError::Agent(format!("session store lock failed: {error}")))
    }

    fn prune_expired(&self, connection: &Connection, now: DateTime<Utc>) -> Result<()> {
        let cutoff = now - Duration::minutes(i64::from(self.max_age_minutes));
        connection.execute(
            "DELETE FROM agent_sessions WHERE updated_at < ?1",
            params![format_timestamp(cutoff)],
        )?;
        Ok(())
    }
}

fn validate_limits(config: &SessionConfig) -> Result<()> {
    if config.max_messages == 0 || config.max_age_minutes == 0 {
        return Err(EduMindError::Agent(
            "session max_messages and max_age_minutes must be greater than zero".to_owned(),
        ));
    }
    Ok(())
}

fn validate_identifier(kind: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(EduMindError::Agent(format!("{kind} must not be empty")));
    }
    Ok(())
}

fn session_from_row(
    session_key: String,
    agent_id: String,
    created_at: String,
    updated_at: String,
) -> Result<Session> {
    Ok(Session {
        session_key,
        agent_id,
        created_at: parse_timestamp(&created_at)?,
        updated_at: parse_timestamp(&updated_at)?,
    })
}

fn message_from_row(
    id: String,
    role: String,
    content: String,
    created_at: String,
) -> Result<SessionMessage> {
    Ok(SessionMessage {
        id: Uuid::parse_str(&id).map_err(|error| {
            EduMindError::InvalidStoredData(format!("invalid session message id `{id}`: {error}"))
        })?,
        role: ChatRole::parse(&role)?,
        content,
        created_at: parse_timestamp(&created_at)?,
    })
}

fn format_timestamp(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

fn parse_timestamp(timestamp: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            EduMindError::InvalidStoredData(format!(
                "invalid session timestamp `{timestamp}`: {error}"
            ))
        })
}

fn estimate_tokens(content: &str) -> usize {
    content.chars().count().div_ceil(4).saturating_add(4)
}

fn truncate_to_tail(content: &str, limit: usize) -> String {
    let tail = content
        .chars()
        .rev()
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    if tail.len() == content.len() {
        tail
    } else {
        format!("…{tail}")
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::{ChatRole, SessionManager};
    use crate::config::types::SessionConfig;

    #[test]
    fn bounds_persisted_history_and_keeps_the_newest_messages() {
        let config = SessionConfig {
            max_messages: 2,
            max_age_minutes: 60,
        };
        let manager = SessionManager::in_memory(&config).unwrap();
        let now = Utc::now();
        manager
            .get_or_create("desktop:student", "master", now)
            .unwrap();
        manager
            .append_message("desktop:student", ChatRole::User, "first", now)
            .unwrap();
        manager
            .append_message(
                "desktop:student",
                ChatRole::Assistant,
                "second",
                now + Duration::seconds(1),
            )
            .unwrap();
        manager
            .append_message(
                "desktop:student",
                ChatRole::User,
                "third",
                now + Duration::seconds(2),
            )
            .unwrap();

        let messages = manager
            .messages("desktop:student", now + Duration::seconds(3))
            .unwrap();

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "second");
        assert_eq!(messages[1].content, "third");
    }

    #[test]
    fn prevents_rebinding_a_session_to_another_agent() {
        let manager = SessionManager::in_memory(&SessionConfig::default()).unwrap();
        let now = Utc::now();
        manager
            .get_or_create("desktop:student", "master", now)
            .unwrap();

        assert!(
            manager
                .get_or_create("desktop:student", "researcher", now)
                .is_err()
        );
    }

    #[test]
    fn context_window_prioritizes_the_newest_message() {
        let manager = SessionManager::in_memory(&SessionConfig::default()).unwrap();
        let now = Utc::now();
        manager
            .get_or_create("desktop:student", "master", now)
            .unwrap();
        manager
            .append_message("desktop:student", ChatRole::User, "old context", now)
            .unwrap();
        manager
            .append_message(
                "desktop:student",
                ChatRole::Assistant,
                "latest answer",
                now + Duration::seconds(1),
            )
            .unwrap();

        let context = manager
            .context_for_model("desktop:student", 8, now + Duration::seconds(2))
            .unwrap();

        assert_eq!(context.len(), 1);
        assert_eq!(context[0].content, "latest answer");
    }
}
