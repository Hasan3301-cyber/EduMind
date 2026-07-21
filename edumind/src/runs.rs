use chrono::{DateTime, SecondsFormat, Utc};
use edumind_core::{PipelineRunId, RunBudget, RunCheckpoint, RunTimelineEvent, RunVerification};
use rusqlite::{OptionalExtension, Row, params};

use crate::{
    infra::{EduMindError, Result},
    memory::MemoryStore,
};

/// SQLite-backed durable state for resumable agent runs.
#[derive(Clone)]
pub struct RunStore {
    memory: MemoryStore,
}

impl RunStore {
    /// Creates a repository backed by the shared local-first memory database.
    #[must_use]
    pub fn new(memory: MemoryStore) -> Self {
        Self { memory }
    }

    /// Persists a checkpoint once for a run, stage, and attempt tuple.
    pub fn create_checkpoint(&self, checkpoint: &RunCheckpoint) -> Result<RunCheckpoint> {
        validate_stage(&checkpoint.stage, "checkpoint")?;
        let encoded = serde_json::to_string(checkpoint)?;
        let connection = self.memory.connection()?;
        connection.execute(
            "INSERT INTO run_checkpoints (
                run_id, stage, attempt, checkpoint_json, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(run_id, stage, attempt) DO NOTHING",
            params![
                checkpoint.run_id.0.to_string(),
                checkpoint.stage,
                checkpoint.attempt,
                encoded,
                format_timestamp(checkpoint.created_at),
            ],
        )?;
        connection
            .query_row(
                "SELECT checkpoint_json FROM run_checkpoints
                 WHERE run_id = ?1 AND stage = ?2 AND attempt = ?3",
                params![
                    checkpoint.run_id.0.to_string(),
                    checkpoint.stage,
                    checkpoint.attempt,
                ],
                checkpoint_from_row,
            )
            .map_err(EduMindError::from)
    }

    /// Lists persisted checkpoints in deterministic creation order.
    pub fn checkpoints(&self, run_id: PipelineRunId) -> Result<Vec<RunCheckpoint>> {
        let connection = self.memory.connection()?;
        let mut statement = connection.prepare(
            "SELECT checkpoint_json FROM run_checkpoints
             WHERE run_id = ?1
             ORDER BY created_at ASC, stage ASC, attempt ASC",
        )?;
        let mut rows = statement.query(params![run_id.0.to_string()])?;
        let mut checkpoints = Vec::new();
        while let Some(row) = rows.next()? {
            checkpoints.push(checkpoint_from_row(row)?);
        }
        Ok(checkpoints)
    }

    /// Stores the authoritative budget snapshot for one run.
    pub fn set_budget(
        &self,
        run_id: PipelineRunId,
        budget: &RunBudget,
        updated_at: DateTime<Utc>,
    ) -> Result<()> {
        let connection = self.memory.connection()?;
        connection.execute(
            "INSERT INTO run_budgets (run_id, budget_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(run_id) DO UPDATE SET
                budget_json = excluded.budget_json,
                updated_at = excluded.updated_at",
            params![
                run_id.0.to_string(),
                serde_json::to_string(budget)?,
                format_timestamp(updated_at),
            ],
        )?;
        Ok(())
    }

    /// Retrieves the budget snapshot for one run.
    pub fn budget(&self, run_id: PipelineRunId) -> Result<Option<RunBudget>> {
        let connection = self.memory.connection()?;
        connection
            .query_row(
                "SELECT budget_json FROM run_budgets WHERE run_id = ?1",
                params![run_id.0.to_string()],
                budget_from_row,
            )
            .optional()
            .map_err(EduMindError::from)
    }

    /// Atomically records resource usage while refusing to cross a configured ceiling.
    pub fn consume_budget(
        &self,
        run_id: PipelineRunId,
        tool_calls: u32,
        output_bytes: u64,
        elapsed_secs: u64,
        updated_at: DateTime<Utc>,
    ) -> Result<RunBudget> {
        let connection = self.memory.connection()?;
        let mut budget = connection
            .query_row(
                "SELECT budget_json FROM run_budgets WHERE run_id = ?1",
                params![run_id.0.to_string()],
                budget_from_row,
            )
            .optional()?
            .ok_or_else(|| {
                EduMindError::Agent(format!("run budget for {} is not initialized", run_id.0))
            })?;
        if !budget.consume(tool_calls, output_bytes, elapsed_secs) {
            return Err(EduMindError::Agent(format!(
                "run budget for {} would be exceeded",
                run_id.0
            )));
        }
        connection.execute(
            "UPDATE run_budgets
             SET budget_json = ?2, updated_at = ?3
             WHERE run_id = ?1",
            params![
                run_id.0.to_string(),
                serde_json::to_string(&budget)?,
                format_timestamp(updated_at),
            ],
        )?;
        Ok(budget)
    }

    /// Appends a verification result without allowing existing records to change.
    pub fn record_verification(&self, verification: &RunVerification) -> Result<RunVerification> {
        validate_stage(&verification.stage, "verification")?;
        if verification.summary.trim().is_empty() {
            return Err(EduMindError::Agent(
                "run verification summary must not be empty".to_owned(),
            ));
        }
        let connection = self.memory.connection()?;
        connection.execute(
            "INSERT INTO run_verifications (
                id, run_id, stage, verification_json, verified_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO NOTHING",
            params![
                verification.id.to_string(),
                verification.run_id.0.to_string(),
                verification.stage,
                serde_json::to_string(verification)?,
                format_timestamp(verification.verified_at),
            ],
        )?;
        connection
            .query_row(
                "SELECT verification_json FROM run_verifications WHERE id = ?1",
                params![verification.id.to_string()],
                verification_from_row,
            )
            .map_err(EduMindError::from)
    }

    /// Lists verification records in durable chronological order.
    pub fn verifications(&self, run_id: PipelineRunId) -> Result<Vec<RunVerification>> {
        let connection = self.memory.connection()?;
        let mut statement = connection.prepare(
            "SELECT verification_json FROM run_verifications
             WHERE run_id = ?1
             ORDER BY verified_at ASC, id ASC",
        )?;
        let mut rows = statement.query(params![run_id.0.to_string()])?;
        let mut verifications = Vec::new();
        while let Some(row) = rows.next()? {
            verifications.push(verification_from_row(row)?);
        }
        Ok(verifications)
    }

    /// Appends a timeline event without allowing later mutation.
    pub fn append_timeline_event(&self, event: &RunTimelineEvent) -> Result<RunTimelineEvent> {
        if event.event_type.trim().is_empty() || event.message.trim().is_empty() {
            return Err(EduMindError::Agent(
                "run timeline event_type and message must not be empty".to_owned(),
            ));
        }
        let connection = self.memory.connection()?;
        connection.execute(
            "INSERT INTO run_timeline_events (id, run_id, event_json, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO NOTHING",
            params![
                event.id.to_string(),
                event.run_id.0.to_string(),
                serde_json::to_string(event)?,
                format_timestamp(event.at),
            ],
        )?;
        connection
            .query_row(
                "SELECT event_json FROM run_timeline_events WHERE id = ?1",
                params![event.id.to_string()],
                timeline_event_from_row,
            )
            .map_err(EduMindError::from)
    }

    /// Lists timeline events in append order for recovery and user-visible history.
    pub fn timeline(&self, run_id: PipelineRunId) -> Result<Vec<RunTimelineEvent>> {
        let connection = self.memory.connection()?;
        let mut statement = connection.prepare(
            "SELECT event_json FROM run_timeline_events
             WHERE run_id = ?1
             ORDER BY created_at ASC, id ASC",
        )?;
        let mut rows = statement.query(params![run_id.0.to_string()])?;
        let mut events = Vec::new();
        while let Some(row) = rows.next()? {
            events.push(timeline_event_from_row(row)?);
        }
        Ok(events)
    }
}

fn validate_stage(stage: &str, record_type: &str) -> Result<()> {
    if stage.trim().is_empty() {
        return Err(EduMindError::Agent(format!(
            "run {record_type} stage must not be empty"
        )));
    }
    Ok(())
}

fn checkpoint_from_row(row: &Row<'_>) -> rusqlite::Result<RunCheckpoint> {
    decode_json(row, "run checkpoint")
}

fn budget_from_row(row: &Row<'_>) -> rusqlite::Result<RunBudget> {
    decode_json(row, "run budget")
}

fn verification_from_row(row: &Row<'_>) -> rusqlite::Result<RunVerification> {
    decode_json(row, "run verification")
}

fn timeline_event_from_row(row: &Row<'_>) -> rusqlite::Result<RunTimelineEvent> {
    decode_json(row, "run timeline event")
}

fn decode_json<T: serde::de::DeserializeOwned>(
    row: &Row<'_>,
    record_type: &str,
) -> rusqlite::Result<T> {
    let encoded: String = row.get(0)?;
    serde_json::from_str(&encoded).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(EduMindError::InvalidStoredData(format!(
                "invalid stored {record_type}: {error}"
            ))),
        )
    })
}

fn format_timestamp(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Micros, true)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};
    use edumind_core::{
        PipelineRunId, RunBudget, RunCheckpoint, RunTimelineEvent, RunVerification,
    };
    use serde_json::json;

    use super::RunStore;
    use crate::memory::MemoryStore;

    #[test]
    fn checkpoints_are_idempotent_for_one_run_stage_and_attempt() {
        let store = RunStore::new(MemoryStore::in_memory().unwrap());
        let run_id = PipelineRunId::new();
        let now = timestamp();
        let mut checkpoint = RunCheckpoint::new(run_id, "plan", 1, now);
        checkpoint.state = json!({"step": "draft"});
        let saved = store.create_checkpoint(&checkpoint).unwrap();

        let mut duplicate = checkpoint.clone();
        duplicate.state = json!({"step": "different"});
        let repeated = store.create_checkpoint(&duplicate).unwrap();

        assert_eq!(saved, checkpoint);
        assert_eq!(repeated, checkpoint);
        assert_eq!(store.checkpoints(run_id).unwrap(), vec![checkpoint]);
    }

    #[test]
    fn persists_budget_verification_and_append_only_timeline() {
        let store = RunStore::new(MemoryStore::in_memory().unwrap());
        let run_id = PipelineRunId::new();
        let now = timestamp();
        let budget = RunBudget {
            max_tool_calls: Some(2),
            max_output_bytes: Some(100),
            max_elapsed_secs: Some(20),
            ..RunBudget::default()
        };

        store.set_budget(run_id, &budget, now).unwrap();
        let consumed = store.consume_budget(run_id, 1, 40, 5, now).unwrap();
        assert_eq!(consumed.tool_calls_used, 1);
        assert!(store.consume_budget(run_id, 2, 1, 1, now).is_err());

        let verification = RunVerification::new(run_id, "verify", true, "passed", now);
        assert_eq!(
            store.record_verification(&verification).unwrap(),
            verification
        );
        assert_eq!(store.verifications(run_id).unwrap(), vec![verification]);

        let first = RunTimelineEvent::new(run_id, "planned", "Created plan.", now);
        let second = RunTimelineEvent::new(
            run_id,
            "verified",
            "Verified plan.",
            now + Duration::seconds(1),
        );
        store.append_timeline_event(&first).unwrap();
        store.append_timeline_event(&second).unwrap();
        assert_eq!(store.timeline(run_id).unwrap(), vec![first, second]);
    }

    fn timestamp() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0).unwrap()
    }
}
