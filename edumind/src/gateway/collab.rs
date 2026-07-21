use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::{
    gateway::{Broadcaster, EventFrame},
    infra::Result,
    memory::{
        CollaborationEvent, CollaborationSession, CollaborationSessionId, MemoryStore,
        NewCollaborationEvent, NewCollaborationSession,
    },
};

/// Gateway-facing collaboration facade with optional WebSocket event fan-out.
#[derive(Clone)]
pub struct CollaborationService {
    store: MemoryStore,
    broadcaster: Option<Broadcaster>,
}

impl CollaborationService {
    /// Creates a service without gateway event fan-out.
    #[must_use]
    pub fn new(store: MemoryStore) -> Self {
        Self {
            store,
            broadcaster: None,
        }
    }

    /// Creates a service that announces collaboration changes to gateway subscribers.
    #[must_use]
    pub fn with_broadcaster(store: MemoryStore, broadcaster: Broadcaster) -> Self {
        Self {
            store,
            broadcaster: Some(broadcaster),
        }
    }

    /// Creates a session and emits a creation event.
    pub fn create(
        &self,
        input: NewCollaborationSession,
        now: DateTime<Utc>,
    ) -> Result<CollaborationSession> {
        let session = self.store.create_collaboration_session(input, now)?;
        self.publish(
            "collaboration.session_created",
            json!({
                "session_id": session.id.to_string(),
                "module_id": session.module_id,
                "owner_id": session.owner_id,
            }),
        );
        Ok(session)
    }

    /// Joins a member to an existing session and emits a membership event.
    pub fn join(
        &self,
        session_id: CollaborationSessionId,
        member_id: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<Option<CollaborationSession>> {
        let member_id = member_id.into();
        let session = self
            .store
            .join_collaboration_session(session_id, member_id.clone(), now)?;
        if session.is_some() {
            self.publish(
                "collaboration.member_joined",
                json!({"session_id": session_id.to_string(), "member_id": member_id}),
            );
        }
        Ok(session)
    }

    /// Replaces session state and emits the resulting state snapshot.
    pub fn update_state(
        &self,
        session_id: CollaborationSessionId,
        actor_id: impl Into<String>,
        state: Value,
        now: DateTime<Utc>,
    ) -> Result<Option<CollaborationSession>> {
        let session = self
            .store
            .update_collaboration_state(session_id, actor_id, state, now)?;
        if let Some(session) = &session {
            self.publish(
                "collaboration.state_updated",
                json!({"session_id": session.id.to_string(), "state": session.state}),
            );
        }
        Ok(session)
    }

    /// Appends an immutable collaboration event and broadcasts it to clients.
    pub fn append_event(
        &self,
        session_id: CollaborationSessionId,
        input: NewCollaborationEvent,
        now: DateTime<Utc>,
    ) -> Result<Option<CollaborationEvent>> {
        let event = self
            .store
            .append_collaboration_event(session_id, input, now)?;
        if let Some(event) = &event {
            self.publish(
                "collaboration.event_appended",
                json!({"session_id": session_id.to_string(), "event": event}),
            );
        }
        Ok(event)
    }

    /// Gets a complete collaboration session or returns `None` for an unknown ID.
    pub fn get(&self, session_id: CollaborationSessionId) -> Result<Option<CollaborationSession>> {
        self.store.get_collaboration_session(session_id)
    }

    /// Lists active and historical sessions in reverse update order.
    pub fn list(&self) -> Result<Vec<CollaborationSession>> {
        self.store.list_collaboration_sessions()
    }

    /// Lists chronological event history for one session.
    pub fn events(
        &self,
        session_id: CollaborationSessionId,
        limit: usize,
    ) -> Result<Vec<CollaborationEvent>> {
        self.store.list_collaboration_events(session_id, limit)
    }

    fn publish(&self, event: &str, payload: Value) {
        if let Some(broadcaster) = &self.broadcaster {
            broadcaster.publish(EventFrame::new(event, payload));
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::CollaborationService;
    use crate::{
        gateway::Broadcaster,
        memory::{MemoryStore, NewCollaborationSession},
    };

    #[tokio::test]
    async fn publishes_session_creation_for_gateway_subscribers() {
        let broadcaster = Broadcaster::new(4);
        let mut receiver = broadcaster.subscribe();
        let service =
            CollaborationService::with_broadcaster(MemoryStore::in_memory().unwrap(), broadcaster);
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 12, 0, 0).unwrap();

        let session = service
            .create(NewCollaborationSession::new("student-os", "owner"), now)
            .unwrap();
        let event = receiver.recv().await.unwrap();

        assert_eq!(event.event, "collaboration.session_created");
        assert_eq!(event.payload["session_id"], session.id.to_string());
    }
}
