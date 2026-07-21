use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

use async_trait::async_trait;
use chrono::Utc;
use edumind_core::{PipelineRunId, RunBudget, RunCheckpoint, RunTimelineEvent, RunVerification};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    infra::{EduMindError, Result},
    runs::RunStore,
};

/// Shared registry of cancellation tokens keyed by durable run ID.
#[derive(Clone, Default)]
pub struct CancellationRegistry {
    tokens: Arc<Mutex<HashMap<PipelineRunId, CancellationToken>>>,
}

impl CancellationRegistry {
    /// Returns the stable cancellation token for a run, creating it if needed.
    pub fn token_for(&self, run_id: PipelineRunId) -> Result<CancellationToken> {
        let mut tokens = self.tokens.lock().map_err(|error| {
            EduMindError::Agent(format!("run cancellation registry lock failed: {error}"))
        })?;
        Ok(tokens
            .entry(run_id)
            .or_insert_with(CancellationToken::new)
            .clone())
    }

    /// Requests cancellation for a run, including a run that has not begun yet.
    pub fn cancel(&self, run_id: PipelineRunId) -> Result<()> {
        self.token_for(run_id)?.cancel();
        Ok(())
    }

    /// Releases a terminal run's token when callers no longer need cancellation state.
    pub fn remove(&self, run_id: PipelineRunId) -> Result<()> {
        self.tokens
            .lock()
            .map_err(|error| {
                EduMindError::Agent(format!("run cancellation registry lock failed: {error}"))
            })?
            .remove(&run_id);
        Ok(())
    }
}

/// One Plan -> Execute -> Verify -> Commit stage with caller-defined deterministic work.
#[async_trait]
pub trait RunStage: Send + Sync {
    /// Stable stage name used in durable checkpoints and timeline events.
    fn name(&self) -> &str;

    /// Plans a stage before any side effect is committed.
    async fn plan(&self, cancellation: CancellationToken) -> Result<Value>;

    /// Performs the bounded stage work.
    async fn execute(&self, plan: &Value, cancellation: CancellationToken) -> Result<Value>;

    /// Verifies stage output before commit.
    async fn verify(
        &self,
        plan: &Value,
        output: &Value,
        cancellation: CancellationToken,
    ) -> Result<RunVerification>;

    /// Commits verified output to its target state.
    async fn commit(
        &self,
        plan: &Value,
        output: &Value,
        cancellation: CancellationToken,
    ) -> Result<Value>;
}

/// Complete outcome returned after a stage reaches the commit boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct RunStageResult {
    pub plan: Value,
    pub output: Value,
    pub verification: RunVerification,
    pub commit: Value,
}

/// Durable executor that persists every boundary and refuses cancelled or over-budget work.
#[derive(Clone)]
pub struct PlanExecuteVerifyCommitEngine {
    store: RunStore,
    cancellations: CancellationRegistry,
}

impl PlanExecuteVerifyCommitEngine {
    /// Creates an engine over durable run storage and a shared cancellation registry.
    #[must_use]
    pub fn new(store: RunStore, cancellations: CancellationRegistry) -> Self {
        Self {
            store,
            cancellations,
        }
    }

    /// Returns the durable repository used by this engine.
    #[must_use]
    pub fn store(&self) -> RunStore {
        self.store.clone()
    }

    /// Returns the cancellation registry exposed to gateway control endpoints.
    #[must_use]
    pub fn cancellations(&self) -> CancellationRegistry {
        self.cancellations.clone()
    }

    /// Initializes a run's budget and records its first durable timeline event.
    pub fn initialize(
        &self,
        run_id: PipelineRunId,
        budget: &RunBudget,
    ) -> Result<CancellationToken> {
        let now = Utc::now();
        self.store.set_budget(run_id, budget, now)?;
        self.store.append_timeline_event(&RunTimelineEvent::new(
            run_id,
            "run_initialized",
            "Initialized run budget and recovery state.",
            now,
        ))?;
        self.cancellations.token_for(run_id)
    }

    /// Runs one stage through plan, execute, verify, and commit boundaries.
    pub async fn run_stage<S: RunStage + ?Sized>(
        &self,
        run_id: PipelineRunId,
        stage: &S,
        attempt: u32,
    ) -> Result<RunStageResult> {
        if stage.name().trim().is_empty() {
            return Err(EduMindError::Agent(
                "run stages require a stable non-empty name".to_owned(),
            ));
        }
        let token = self.cancellations.token_for(run_id)?;
        self.ensure_active(run_id, stage.name(), &token)?;
        self.store.create_checkpoint(&RunCheckpoint {
            run_id,
            stage: format!("{}:before", stage.name()),
            attempt,
            state: json!({"phase": "before"}),
            created_at: Utc::now(),
        })?;
        self.record_event(
            run_id,
            stage.name(),
            "stage_started",
            "Started planned stage.",
        )?;

        let plan = self
            .run_phase(
                run_id,
                stage.name(),
                "plan",
                &token,
                stage.plan(token.clone()),
            )
            .await?;
        let output = self
            .run_phase(
                run_id,
                stage.name(),
                "execute",
                &token,
                stage.execute(&plan, token.clone()),
            )
            .await?;
        let verification = self
            .run_phase(
                run_id,
                stage.name(),
                "verify",
                &token,
                stage.verify(&plan, &output, token.clone()),
            )
            .await?;
        if verification.run_id != run_id
            || verification.stage != stage.name()
            || !verification.passed
        {
            self.record_event(
                run_id,
                stage.name(),
                "stage_verification_failed",
                "Stage verification did not pass.",
            )?;
            return Err(EduMindError::Agent(format!(
                "stage '{}' did not produce a passing verification",
                stage.name()
            )));
        }
        self.store.record_verification(&verification)?;
        let commit = self
            .run_phase(
                run_id,
                stage.name(),
                "commit",
                &token,
                stage.commit(&plan, &output, token.clone()),
            )
            .await?;

        self.store.create_checkpoint(&RunCheckpoint {
            run_id,
            stage: format!("{}:after", stage.name()),
            attempt,
            state: json!({
                "phase": "after",
                "verified": true,
                "output_bytes": serialized_size(&output)?,
                "commit_bytes": serialized_size(&commit)?,
            }),
            created_at: Utc::now(),
        })?;
        self.record_event(
            run_id,
            stage.name(),
            "stage_committed",
            "Verified stage output was committed.",
        )?;
        Ok(RunStageResult {
            plan,
            output,
            verification,
            commit,
        })
    }

    async fn run_phase<T, F>(
        &self,
        run_id: PipelineRunId,
        stage: &str,
        phase: &str,
        token: &CancellationToken,
        future: F,
    ) -> Result<T>
    where
        T: serde::Serialize,
        F: std::future::Future<Output = Result<T>>,
    {
        self.ensure_active(run_id, stage, token)?;
        self.store.consume_budget(run_id, 1, 0, 0, Utc::now())?;
        let started = Instant::now();
        let result = tokio::select! {
            _ = token.cancelled() => {
                self.record_cancelled(run_id, stage)?;
                return Err(EduMindError::Agent(format!("run {} was cancelled before {}", run_id.0, phase)));
            }
            result = future => result,
        }?;
        self.ensure_active(run_id, stage, token)?;
        let elapsed_secs = started.elapsed().as_secs();
        let output_bytes = serialized_size(&result)?;
        self.store
            .consume_budget(run_id, 0, output_bytes, elapsed_secs, Utc::now())?;
        self.record_event(
            run_id,
            stage,
            "stage_phase_completed",
            &format!("Completed {phase} boundary."),
        )?;
        Ok(result)
    }

    fn ensure_active(
        &self,
        run_id: PipelineRunId,
        stage: &str,
        token: &CancellationToken,
    ) -> Result<()> {
        if token.is_cancelled() {
            self.record_cancelled(run_id, stage)?;
            return Err(EduMindError::Agent(format!(
                "run {} was cancelled before stage '{}'",
                run_id.0, stage
            )));
        }
        Ok(())
    }

    fn record_cancelled(&self, run_id: PipelineRunId, stage: &str) -> Result<()> {
        self.record_event(
            run_id,
            stage,
            "run_cancelled",
            "Cancellation was observed before further stage work.",
        )
    }

    fn record_event(
        &self,
        run_id: PipelineRunId,
        stage: &str,
        event_type: &str,
        message: &str,
    ) -> Result<()> {
        let mut event = RunTimelineEvent::new(run_id, event_type, message, Utc::now());
        event.stage = Some(stage.to_owned());
        self.store.append_timeline_event(&event)?;
        Ok(())
    }
}

fn serialized_size(value: &impl serde::Serialize) -> Result<u64> {
    u64::try_from(serde_json::to_vec(value)?.len())
        .map_err(|error| EduMindError::Agent(format!("run output size overflowed: {error}")))
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use edumind_core::{PipelineRunId, RunBudget, RunVerification};
    use serde_json::{Value, json};
    use tokio_util::sync::CancellationToken;

    use super::{CancellationRegistry, PlanExecuteVerifyCommitEngine, RunStage, RunStageResult};
    use crate::{infra::Result, memory::MemoryStore, runs::RunStore};

    struct RunBoundStage {
        run_id: PipelineRunId,
    }

    #[async_trait::async_trait]
    impl RunStage for RunBoundStage {
        fn name(&self) -> &str {
            "test"
        }

        async fn plan(&self, _cancellation: CancellationToken) -> Result<Value> {
            Ok(json!({"plan": true}))
        }

        async fn execute(&self, _plan: &Value, _cancellation: CancellationToken) -> Result<Value> {
            Ok(json!({"output": true}))
        }

        async fn verify(
            &self,
            _plan: &Value,
            _output: &Value,
            _cancellation: CancellationToken,
        ) -> Result<RunVerification> {
            Ok(RunVerification::new(
                self.run_id,
                "test",
                true,
                "passed",
                Utc::now(),
            ))
        }

        async fn commit(
            &self,
            _plan: &Value,
            _output: &Value,
            _cancellation: CancellationToken,
        ) -> Result<Value> {
            Ok(json!({"committed": true}))
        }
    }

    fn engine() -> PlanExecuteVerifyCommitEngine {
        PlanExecuteVerifyCommitEngine::new(
            RunStore::new(MemoryStore::in_memory().unwrap()),
            CancellationRegistry::default(),
        )
    }

    #[tokio::test]
    async fn persists_stage_boundaries_and_refuses_cancelled_runs() {
        let engine = engine();
        let run_id = PipelineRunId::new();
        engine
            .initialize(
                run_id,
                &RunBudget {
                    max_tool_calls: Some(10),
                    max_output_bytes: Some(10_000),
                    max_elapsed_secs: Some(10),
                    ..RunBudget::default()
                },
            )
            .unwrap();
        let result: RunStageResult = engine
            .run_stage(run_id, &RunBoundStage { run_id }, 1)
            .await
            .unwrap();

        assert!(result.verification.passed);
        assert_eq!(engine.store().checkpoints(run_id).unwrap().len(), 2);
        assert_eq!(engine.store().verifications(run_id).unwrap().len(), 1);

        let cancelled = PipelineRunId::new();
        engine
            .initialize(
                cancelled,
                &RunBudget {
                    max_tool_calls: Some(10),
                    max_output_bytes: Some(10_000),
                    max_elapsed_secs: Some(10),
                    ..RunBudget::default()
                },
            )
            .unwrap();
        engine.cancellations().cancel(cancelled).unwrap();
        assert!(
            engine
                .run_stage(cancelled, &RunBoundStage { run_id: cancelled }, 1)
                .await
                .is_err()
        );
        assert!(
            engine
                .store()
                .timeline(cancelled)
                .unwrap()
                .iter()
                .any(|event| event.event_type == "run_cancelled")
        );
        assert!(Utc::now() < Utc::now() + Duration::seconds(1));
    }
}
