use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::TaskId;

/// Stable identifier for an agent pipeline execution.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PipelineRunId(pub Uuid);

impl PipelineRunId {
    /// Creates a new random pipeline run identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for PipelineRunId {
    fn default() -> Self {
        Self::new()
    }
}

/// Overall lifecycle state for a pipeline run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineRunStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Lifecycle state for an individual pipeline stage.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStageStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

/// A named unit of work within an ordered pipeline run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PipelineStage {
    pub name: String,
    pub status: PipelineStageStatus,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
}

impl PipelineStage {
    /// Creates a pending stage with no runtime metadata.
    #[must_use]
    pub fn pending(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: PipelineStageStatus::Pending,
            detail: None,
            started_at: None,
            completed_at: None,
        }
    }

    /// Starts the stage at a supplied deterministic timestamp.
    pub fn start(&mut self, now: DateTime<Utc>) {
        self.status = PipelineStageStatus::Running;
        self.started_at = Some(now);
        self.completed_at = None;
    }

    /// Completes the stage with optional human-readable detail.
    pub fn complete(&mut self, detail: Option<String>, now: DateTime<Utc>) {
        self.status = PipelineStageStatus::Completed;
        self.detail = detail;
        self.completed_at = Some(now);
    }
}

/// Durable state describing a complete multi-stage agent run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: PipelineRunId,
    pub task_id: TaskId,
    pub status: PipelineRunStatus,
    #[serde(default)]
    pub stages: Vec<PipelineStage>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PipelineRun {
    /// Creates a pending run linked to a task.
    #[must_use]
    pub fn new(task_id: TaskId, now: DateTime<Utc>) -> Self {
        Self {
            id: PipelineRunId::new(),
            task_id,
            status: PipelineRunStatus::Pending,
            stages: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Adds a pending stage and returns its index.
    pub fn add_stage(&mut self, stage: PipelineStage, now: DateTime<Utc>) -> usize {
        self.stages.push(stage);
        self.updated_at = now;
        self.stages.len() - 1
    }
}

/// A durable, idempotent checkpoint for one run stage attempt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunCheckpoint {
    pub run_id: PipelineRunId,
    pub stage: String,
    #[serde(default)]
    pub attempt: u32,
    #[serde(default)]
    pub state: Value,
    pub created_at: DateTime<Utc>,
}

impl RunCheckpoint {
    /// Creates a checkpoint with an empty JSON state payload.
    #[must_use]
    pub fn new(
        run_id: PipelineRunId,
        stage: impl Into<String>,
        attempt: u32,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            run_id,
            stage: stage.into(),
            attempt,
            state: Value::Null,
            created_at,
        }
    }
}

/// Configured and consumed execution limits for a run.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunBudget {
    #[serde(default)]
    pub max_tool_calls: Option<u32>,
    #[serde(default)]
    pub max_output_bytes: Option<u64>,
    #[serde(default)]
    pub max_elapsed_secs: Option<u64>,
    #[serde(default)]
    pub tool_calls_used: u32,
    #[serde(default)]
    pub output_bytes_used: u64,
    #[serde(default)]
    pub elapsed_secs_used: u64,
}

impl RunBudget {
    /// Returns whether the requested resources fit within configured budget ceilings.
    #[must_use]
    pub fn can_consume(&self, tool_calls: u32, output_bytes: u64, elapsed_secs: u64) -> bool {
        let Some(next_tool_calls) = self.tool_calls_used.checked_add(tool_calls) else {
            return false;
        };
        let Some(next_output_bytes) = self.output_bytes_used.checked_add(output_bytes) else {
            return false;
        };
        let Some(next_elapsed_secs) = self.elapsed_secs_used.checked_add(elapsed_secs) else {
            return false;
        };
        self.max_tool_calls
            .is_none_or(|limit| next_tool_calls <= limit)
            && self
                .max_output_bytes
                .is_none_or(|limit| next_output_bytes <= limit)
            && self
                .max_elapsed_secs
                .is_none_or(|limit| next_elapsed_secs <= limit)
    }

    /// Applies resource consumption when all configured limits permit it.
    pub fn consume(&mut self, tool_calls: u32, output_bytes: u64, elapsed_secs: u64) -> bool {
        if !self.can_consume(tool_calls, output_bytes, elapsed_secs) {
            return false;
        }
        self.tool_calls_used = self.tool_calls_used.saturating_add(tool_calls);
        self.output_bytes_used = self.output_bytes_used.saturating_add(output_bytes);
        self.elapsed_secs_used = self.elapsed_secs_used.saturating_add(elapsed_secs);
        true
    }
}

/// A durable verification result for one stage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunVerification {
    pub id: Uuid,
    pub run_id: PipelineRunId,
    pub stage: String,
    pub passed: bool,
    pub summary: String,
    #[serde(default)]
    pub details: Value,
    pub verified_at: DateTime<Utc>,
}

impl RunVerification {
    /// Creates a verification with an empty JSON details payload.
    #[must_use]
    pub fn new(
        run_id: PipelineRunId,
        stage: impl Into<String>,
        passed: bool,
        summary: impl Into<String>,
        verified_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            run_id,
            stage: stage.into(),
            passed,
            summary: summary.into(),
            details: Value::Null,
            verified_at,
        }
    }
}

/// An append-only event that makes a run recoverable and explainable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunTimelineEvent {
    pub id: Uuid,
    pub run_id: PipelineRunId,
    #[serde(default)]
    pub stage: Option<String>,
    pub event_type: String,
    pub message: String,
    #[serde(default)]
    pub metadata: Value,
    pub at: DateTime<Utc>,
}

impl RunTimelineEvent {
    /// Creates an event with empty JSON metadata.
    #[must_use]
    pub fn new(
        run_id: PipelineRunId,
        event_type: impl Into<String>,
        message: impl Into<String>,
        at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            run_id,
            stage: None,
            event_type: event_type.into(),
            message: message.into(),
            metadata: Value::Null,
            at,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::{
        PipelineRun, PipelineRunId, PipelineStage, PipelineStageStatus, RunBudget, RunCheckpoint,
        RunTimelineEvent, RunVerification,
    };
    use crate::domain::TaskId;

    #[test]
    fn run_tracks_added_stages() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let mut run = PipelineRun::new(TaskId::new(), now);

        let stage_index = run.add_stage(PipelineStage::pending("extract"), now);
        run.stages[stage_index].start(now);
        run.stages[stage_index].complete(Some("2 documents".to_owned()), now);

        assert_eq!(stage_index, 0);
        assert_eq!(run.stages.len(), 1);
        assert_eq!(run.stages[0].status, PipelineStageStatus::Completed);
    }

    #[test]
    fn pipeline_run_round_trips_through_json() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let run = PipelineRun::new(TaskId::new(), now);

        let encoded = serde_json::to_string(&run).unwrap();
        let decoded: PipelineRun = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, run);
    }

    #[test]
    fn run_budget_rejects_consumption_over_a_ceiling() {
        let mut budget = RunBudget {
            max_tool_calls: Some(2),
            max_output_bytes: Some(100),
            max_elapsed_secs: Some(10),
            ..RunBudget::default()
        };

        assert!(budget.consume(1, 40, 3));
        assert!(!budget.consume(2, 40, 3));
        assert_eq!(budget.tool_calls_used, 1);
        assert_eq!(budget.output_bytes_used, 40);
        assert_eq!(budget.elapsed_secs_used, 3);
    }

    #[test]
    fn premium_run_records_round_trip_through_json() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let run_id = PipelineRunId::new();
        let checkpoint = RunCheckpoint::new(run_id, "plan", 1, now);
        let verification = RunVerification::new(run_id, "plan", true, "validated", now);
        let event = RunTimelineEvent::new(run_id, "planned", "Created a plan.", now);

        let encoded = serde_json::to_string(&(checkpoint, verification, event)).unwrap();
        let decoded: (RunCheckpoint, RunVerification, RunTimelineEvent) =
            serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.0.run_id, run_id);
        assert!(decoded.1.passed);
        assert_eq!(decoded.2.event_type, "planned");
    }
}
