use chrono::{DateTime, SecondsFormat, Utc};
use edumind_core::{PipelineContext, PipelineEvent, PipelineRun, PipelineRunId, PipelineRunStatus};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    infra::{EduMindError, Result},
    memory::MemoryStore,
};

/// Complete persisted state for one focused research pipeline run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResearchRunRecord {
    pub run: PipelineRun,
    pub context: PipelineContext,
    #[serde(default)]
    pub error: Option<String>,
}

/// SQLite-backed repository for research pipeline snapshots and progress events.
#[derive(Clone)]
pub struct ResearchRunStore {
    memory: MemoryStore,
}

impl ResearchRunStore {
    /// Creates a run store backed by the shared local-first memory database.
    #[must_use]
    pub fn new(memory: MemoryStore) -> Self {
        Self { memory }
    }

    /// Persists a pipeline snapshot, replacing an earlier snapshot of the same run.
    pub fn save(
        &self,
        run: &PipelineRun,
        context: &PipelineContext,
        error: Option<&str>,
    ) -> Result<()> {
        let run_json = serde_json::to_string(run)?;
        let context_json = serde_json::to_string(context)?;
        let status = serde_json::to_value(run.status)?
            .as_str()
            .unwrap_or("unknown")
            .to_owned();
        let completed_at = matches!(
            run.status,
            PipelineRunStatus::Completed | PipelineRunStatus::Failed | PipelineRunStatus::Cancelled
        )
        .then(|| format_timestamp(run.updated_at));
        let connection = self.memory.connection()?;
        connection.execute(
            "INSERT INTO research_pipeline_runs (
                id, topic, status, run_json, context_json, error, created_at, updated_at, completed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                topic = excluded.topic,
                status = excluded.status,
                run_json = excluded.run_json,
                context_json = excluded.context_json,
                error = excluded.error,
                updated_at = excluded.updated_at,
                completed_at = excluded.completed_at",
            params![
                run.id.0.to_string(),
                &context.topic,
                status,
                run_json,
                context_json,
                error,
                format_timestamp(run.created_at),
                format_timestamp(run.updated_at),
                completed_at,
            ],
        )?;
        Ok(())
    }

    /// Appends one durable progress event to a saved pipeline run.
    pub fn append_event(&self, event: &PipelineEvent) -> Result<()> {
        let connection = self.memory.connection()?;
        connection.execute(
            "INSERT INTO research_pipeline_events (id, run_id, event_json, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                Uuid::new_v4().to_string(),
                event.progress.run_id.0.to_string(),
                serde_json::to_string(event)?,
                format_timestamp(event.progress.at),
            ],
        )?;
        Ok(())
    }

    /// Gets a complete saved run snapshot by its stable pipeline run ID.
    pub fn get(&self, run_id: PipelineRunId) -> Result<Option<ResearchRunRecord>> {
        let connection = self.memory.connection()?;
        connection
            .query_row(
                "SELECT run_json, context_json, error
                 FROM research_pipeline_runs WHERE id = ?1",
                params![run_id.0.to_string()],
                research_run_from_row,
            )
            .optional()
            .map_err(EduMindError::from)
    }

    /// Lists recent runs in deterministic most-recent-first order.
    pub fn list_recent(&self, limit: usize) -> Result<Vec<ResearchRunRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let connection = self.memory.connection()?;
        let mut statement = connection.prepare(
            "SELECT run_json, context_json, error
             FROM research_pipeline_runs
             ORDER BY updated_at DESC, id ASC
             LIMIT ?1",
        )?;
        let mut rows = statement.query(params![i64::try_from(limit).unwrap_or(i64::MAX)])?;
        let mut records = Vec::new();
        while let Some(row) = rows.next()? {
            records.push(research_run_from_row(row)?);
        }
        Ok(records)
    }

    /// Lists progress events for a run in the order they were emitted.
    pub fn events(&self, run_id: PipelineRunId) -> Result<Vec<PipelineEvent>> {
        let connection = self.memory.connection()?;
        let mut statement = connection.prepare(
            "SELECT event_json FROM research_pipeline_events
             WHERE run_id = ?1 ORDER BY created_at ASC, id ASC",
        )?;
        let mut rows = statement.query(params![run_id.0.to_string()])?;
        let mut events = Vec::new();
        while let Some(row) = rows.next()? {
            let encoded: String = row.get(0)?;
            events.push(serde_json::from_str(&encoded).map_err(|error| {
                EduMindError::InvalidStoredData(format!(
                    "invalid stored research pipeline event: {error}"
                ))
            })?);
        }
        Ok(events)
    }
}

fn research_run_from_row(row: &Row<'_>) -> rusqlite::Result<ResearchRunRecord> {
    let run_json: String = row.get(0)?;
    let context_json: String = row.get(1)?;
    let error: Option<String> = row.get(2)?;
    let run = serde_json::from_str(&run_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let context = serde_json::from_str(&context_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(ResearchRunRecord {
        run,
        context,
        error,
    })
}

fn format_timestamp(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Micros, true)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use edumind_core::{PipelineContext, PipelineRun, PipelineRunStatus, ResearchRequest, TaskId};

    use super::ResearchRunStore;
    use crate::memory::MemoryStore;

    #[test]
    fn saves_and_loads_a_pipeline_snapshot() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let mut run = PipelineRun::new(TaskId::new(), now);
        run.status = PipelineRunStatus::Completed;
        let context = PipelineContext::new(run.id, ResearchRequest::new("retrieval systems"), now);
        let store = ResearchRunStore::new(MemoryStore::in_memory().unwrap());

        store.save(&run, &context, None).unwrap();
        let loaded = store.get(run.id).unwrap().unwrap();

        assert_eq!(loaded.run, run);
        assert_eq!(loaded.context, context);
        assert_eq!(loaded.error, None);
    }
}
