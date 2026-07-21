use std::fmt;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::infra::{EduMindError, Result};

use super::store::{MemoryStore, format_timestamp, parse_timestamp};

/// Stable identifier for a persisted collaboration session.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CollaborationSessionId(pub Uuid);

impl CollaborationSessionId {
    /// Creates a random collaboration session identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    fn parse(value: &str) -> Result<Self> {
        Uuid::parse_str(value).map(Self).map_err(|error| {
            EduMindError::InvalidStoredData(format!(
                "invalid collaboration session id `{value}`: {error}"
            ))
        })
    }
}

impl Default for CollaborationSessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for CollaborationSessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Stable identifier for a collaboration event.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CollaborationEventId(pub Uuid);

impl CollaborationEventId {
    fn new() -> Self {
        Self(Uuid::new_v4())
    }

    fn parse(value: &str) -> Result<Self> {
        Uuid::parse_str(value).map(Self).map_err(|error| {
            EduMindError::InvalidStoredData(format!(
                "invalid collaboration event id `{value}`: {error}"
            ))
        })
    }
}

impl fmt::Display for CollaborationEventId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Persistent state for a small multi-user collaboration room.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollaborationSession {
    pub id: CollaborationSessionId,
    pub module_id: String,
    pub owner_id: String,
    pub state: Value,
    pub members: Vec<CollaborationMember>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A member admitted to a collaboration session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CollaborationMember {
    pub member_id: String,
    pub joined_at: DateTime<Utc>,
}

/// Input for creating a collaboration session and owner membership.
#[derive(Clone, Debug, PartialEq)]
pub struct NewCollaborationSession {
    pub module_id: String,
    pub owner_id: String,
    pub state: Value,
}

impl NewCollaborationSession {
    /// Creates a session input with an empty object state.
    #[must_use]
    pub fn new(module_id: impl Into<String>, owner_id: impl Into<String>) -> Self {
        Self {
            module_id: module_id.into(),
            owner_id: owner_id.into(),
            state: Value::Object(serde_json::Map::new()),
        }
    }
}

/// Input for adding an immutable event to a collaboration session.
#[derive(Clone, Debug, PartialEq)]
pub struct NewCollaborationEvent {
    pub actor_id: String,
    pub event_type: String,
    pub payload: Value,
}

impl NewCollaborationEvent {
    /// Creates an event with an empty object payload.
    #[must_use]
    pub fn new(actor_id: impl Into<String>, event_type: impl Into<String>) -> Self {
        Self {
            actor_id: actor_id.into(),
            event_type: event_type.into(),
            payload: Value::Object(serde_json::Map::new()),
        }
    }
}

/// Immutable activity recorded against a collaboration session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollaborationEvent {
    pub id: CollaborationEventId,
    pub session_id: CollaborationSessionId,
    pub actor_id: String,
    pub event_type: String,
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

impl MemoryStore {
    /// Creates a collaboration session and joins its owner atomically.
    pub fn create_collaboration_session(
        &self,
        input: NewCollaborationSession,
        now: DateTime<Utc>,
    ) -> Result<CollaborationSession> {
        validate_session_input(&input)?;
        let session = CollaborationSession {
            id: CollaborationSessionId::new(),
            module_id: input.module_id,
            owner_id: input.owner_id.clone(),
            state: input.state,
            members: vec![CollaborationMember {
                member_id: input.owner_id,
                joined_at: now,
            }],
            created_at: now,
            updated_at: now,
        };
        let timestamp = format_timestamp(now);
        let state_json = serde_json::to_string(&session.state)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO collaboration_sessions (
                id, module_id, owner_id, state_json, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session.id.to_string(),
                session.module_id,
                session.owner_id,
                state_json,
                timestamp,
                timestamp,
            ],
        )?;
        transaction.execute(
            "INSERT INTO collaboration_members (session_id, member_id, joined_at)
             VALUES (?1, ?2, ?3)",
            params![session.id.to_string(), session.owner_id, timestamp],
        )?;
        transaction.commit()?;
        Ok(session)
    }

    /// Retrieves a collaboration session with its members in join order.
    pub fn get_collaboration_session(
        &self,
        session_id: CollaborationSessionId,
    ) -> Result<Option<CollaborationSession>> {
        let connection = self.connection()?;
        read_session_by_id(&connection, session_id)
    }

    /// Lists sessions in reverse update order, including their current members.
    pub fn list_collaboration_sessions(&self) -> Result<Vec<CollaborationSession>> {
        let connection = self.connection()?;
        let mut sessions = {
            let mut statement = connection.prepare(
                "SELECT id, module_id, owner_id, state_json, created_at, updated_at
                 FROM collaboration_sessions
                 ORDER BY updated_at DESC, id ASC",
            )?;
            let mut rows = statement.query([])?;
            let mut sessions = Vec::new();
            while let Some(row) = rows.next()? {
                sessions.push(session_from_row(row)?);
            }
            sessions
        };
        for session in &mut sessions {
            session.members = read_members(&connection, session.id)?;
        }
        Ok(sessions)
    }

    /// Idempotently joins a member to a session and refreshes the session update time.
    pub fn join_collaboration_session(
        &self,
        session_id: CollaborationSessionId,
        member_id: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<Option<CollaborationSession>> {
        let member_id = member_id.into();
        validate_identity(&member_id, "member_id")?;
        let timestamp = format_timestamp(now);
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE collaboration_sessions SET updated_at = ?1 WHERE id = ?2",
            params![timestamp, session_id.to_string()],
        )?;
        if updated == 0 {
            return Ok(None);
        }
        transaction.execute(
            "INSERT INTO collaboration_members (session_id, member_id, joined_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(session_id, member_id) DO NOTHING",
            params![session_id.to_string(), member_id, timestamp],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_collaboration_session(session_id)
    }

    /// Replaces JSON-object state and records the change as an immutable event.
    pub fn update_collaboration_state(
        &self,
        session_id: CollaborationSessionId,
        actor_id: impl Into<String>,
        state: Value,
        now: DateTime<Utc>,
    ) -> Result<Option<CollaborationSession>> {
        let actor_id = actor_id.into();
        validate_identity(&actor_id, "actor_id")?;
        validate_state(&state)?;
        let state_json = serde_json::to_string(&state)?;
        let event_payload = serde_json::to_string(&json!({"state": state}))?;
        let event_id = CollaborationEventId::new();
        let timestamp = format_timestamp(now);
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE collaboration_sessions
             SET state_json = ?1, updated_at = ?2
             WHERE id = ?3",
            params![state_json, timestamp, session_id.to_string()],
        )?;
        if updated == 0 {
            return Ok(None);
        }
        transaction.execute(
            "INSERT INTO collaboration_events (
                id, session_id, actor_id, event_type, payload_json, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event_id.to_string(),
                session_id.to_string(),
                actor_id,
                "state_updated",
                event_payload,
                timestamp,
            ],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_collaboration_session(session_id)
    }

    /// Appends an immutable event and refreshes the session update time.
    pub fn append_collaboration_event(
        &self,
        session_id: CollaborationSessionId,
        input: NewCollaborationEvent,
        now: DateTime<Utc>,
    ) -> Result<Option<CollaborationEvent>> {
        validate_event_input(&input)?;
        let event = CollaborationEvent {
            id: CollaborationEventId::new(),
            session_id,
            actor_id: input.actor_id,
            event_type: input.event_type,
            payload: input.payload,
            created_at: now,
        };
        let timestamp = format_timestamp(now);
        let payload_json = serde_json::to_string(&event.payload)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE collaboration_sessions SET updated_at = ?1 WHERE id = ?2",
            params![timestamp, session_id.to_string()],
        )?;
        if updated == 0 {
            return Ok(None);
        }
        transaction.execute(
            "INSERT INTO collaboration_events (
                id, session_id, actor_id, event_type, payload_json, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event.id.to_string(),
                session_id.to_string(),
                event.actor_id,
                event.event_type,
                payload_json,
                timestamp,
            ],
        )?;
        transaction.commit()?;
        Ok(Some(event))
    }

    /// Lists events in chronological order for replay by desktop or web clients.
    pub fn list_collaboration_events(
        &self,
        session_id: CollaborationSessionId,
        limit: usize,
    ) -> Result<Vec<CollaborationEvent>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, session_id, actor_id, event_type, payload_json, created_at
             FROM collaboration_events
             WHERE session_id = ?1
             ORDER BY created_at ASC, id ASC
             LIMIT ?2",
        )?;
        let mut rows = statement.query(params![session_id.to_string(), limit])?;
        let mut events = Vec::new();
        while let Some(row) = rows.next()? {
            events.push(event_from_row(row)?);
        }
        Ok(events)
    }
}

fn read_session_by_id(
    connection: &Connection,
    session_id: CollaborationSessionId,
) -> Result<Option<CollaborationSession>> {
    let session = {
        let mut statement = connection.prepare(
            "SELECT id, module_id, owner_id, state_json, created_at, updated_at
             FROM collaboration_sessions WHERE id = ?1",
        )?;
        let mut rows = statement.query(params![session_id.to_string()])?;
        rows.next()?.map(session_from_row).transpose()?
    };
    let Some(mut session) = session else {
        return Ok(None);
    };
    session.members = read_members(connection, session.id)?;
    Ok(Some(session))
}

fn read_members(
    connection: &Connection,
    session_id: CollaborationSessionId,
) -> Result<Vec<CollaborationMember>> {
    let mut statement = connection.prepare(
        "SELECT member_id, joined_at FROM collaboration_members
         WHERE session_id = ?1 ORDER BY joined_at ASC, member_id ASC",
    )?;
    let mut rows = statement.query(params![session_id.to_string()])?;
    let mut members = Vec::new();
    while let Some(row) = rows.next()? {
        members.push(CollaborationMember {
            member_id: row.get("member_id")?,
            joined_at: parse_timestamp(&row.get::<_, String>("joined_at")?)?,
        });
    }
    Ok(members)
}

fn session_from_row(row: &Row<'_>) -> Result<CollaborationSession> {
    Ok(CollaborationSession {
        id: CollaborationSessionId::parse(&row.get::<_, String>("id")?)?,
        module_id: row.get("module_id")?,
        owner_id: row.get("owner_id")?,
        state: serde_json::from_str(&row.get::<_, String>("state_json")?)?,
        members: Vec::new(),
        created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
        updated_at: parse_timestamp(&row.get::<_, String>("updated_at")?)?,
    })
}

fn event_from_row(row: &Row<'_>) -> Result<CollaborationEvent> {
    Ok(CollaborationEvent {
        id: CollaborationEventId::parse(&row.get::<_, String>("id")?)?,
        session_id: CollaborationSessionId::parse(&row.get::<_, String>("session_id")?)?,
        actor_id: row.get("actor_id")?,
        event_type: row.get("event_type")?,
        payload: serde_json::from_str(&row.get::<_, String>("payload_json")?)?,
        created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
    })
}

fn validate_session_input(input: &NewCollaborationSession) -> Result<()> {
    validate_identity(&input.module_id, "module_id")?;
    validate_identity(&input.owner_id, "owner_id")?;
    validate_state(&input.state)
}

fn validate_event_input(input: &NewCollaborationEvent) -> Result<()> {
    validate_identity(&input.actor_id, "actor_id")?;
    validate_identity(&input.event_type, "event_type")
}

fn validate_identity(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(EduMindError::Collaboration(format!(
            "collaboration {name} must not be empty"
        )));
    }
    Ok(())
}

fn validate_state(state: &Value) -> Result<()> {
    if !state.is_object() {
        return Err(EduMindError::Collaboration(
            "collaboration state must be a JSON object".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::json;

    use super::{NewCollaborationEvent, NewCollaborationSession};
    use crate::memory::MemoryStore;

    fn timestamp() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 15, 11, 0, 0).unwrap()
    }

    #[test]
    fn persists_session_members_state_and_events() {
        let store = MemoryStore::in_memory().unwrap();
        let mut input = NewCollaborationSession::new("student-os", "owner");
        input.state = json!({"topic": "calculus"});
        let session = store
            .create_collaboration_session(input, timestamp())
            .unwrap();

        let joined = store
            .join_collaboration_session(session.id, "peer", timestamp() + Duration::seconds(1))
            .unwrap()
            .unwrap();
        let updated = store
            .update_collaboration_state(
                session.id,
                "owner",
                json!({"topic": "integration"}),
                timestamp() + Duration::seconds(2),
            )
            .unwrap()
            .unwrap();
        let mut event = NewCollaborationEvent::new("peer", "resource_added");
        event.payload = json!({"resource_id": "note-1"});
        store
            .append_collaboration_event(session.id, event, timestamp() + Duration::seconds(3))
            .unwrap()
            .unwrap();

        let events = store.list_collaboration_events(session.id, 10).unwrap();
        assert_eq!(joined.members.len(), 2);
        assert_eq!(updated.state, json!({"topic": "integration"}));
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "state_updated");
        assert_eq!(events[1].event_type, "resource_added");
        assert_eq!(store.list_collaboration_sessions().unwrap().len(), 1);
    }
}
