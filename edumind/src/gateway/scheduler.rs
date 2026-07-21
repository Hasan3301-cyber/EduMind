use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};

use crate::{
    config::types::{JobsConfig, ScheduledJobConfig},
    infra::{EduMindError, Result},
};

/// One agent message injected by a configured recurring job.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScheduledJobInvocation {
    pub job_id: String,
    pub agent_id: String,
    pub session_key: String,
    pub message: String,
    pub triggered_at: DateTime<Utc>,
    pub startup: bool,
}

/// Adapter from scheduler invocations into the agent runtime.
#[async_trait]
pub trait ScheduledJobHandler: Send + Sync {
    /// Executes an injected agent message.
    async fn run(&self, invocation: ScheduledJobInvocation) -> Result<()>;
}

/// In-memory scheduler for the recurring jobs defined in active configuration.
#[derive(Clone, Debug)]
pub struct Scheduler {
    inner: Arc<Mutex<SchedulerInner>>,
}

#[derive(Debug)]
struct SchedulerInner {
    config: JobsConfig,
    next_runs: BTreeMap<String, DateTime<Utc>>,
    startup_dispatched: BTreeSet<String>,
}

impl Scheduler {
    /// Builds scheduler state relative to a caller-provided clock value.
    pub fn new(config: JobsConfig, now: DateTime<Utc>) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            inner: Arc::new(Mutex::new(SchedulerInner {
                next_runs: initialize_next_runs(&config, now)?,
                config,
                startup_dispatched: BTreeSet::new(),
            })),
        })
    }

    /// Replaces job configuration without re-running startup jobs that keep their ID.
    pub fn replace_config(&self, config: JobsConfig, now: DateTime<Utc>) -> Result<()> {
        config.validate()?;
        let next_runs = initialize_next_runs(&config, now)?;
        let active_job_ids = config
            .schedules
            .iter()
            .filter(|job| job.enabled)
            .map(|job| job.id.clone())
            .collect::<BTreeSet<_>>();
        let mut inner = self.lock()?;
        inner
            .startup_dispatched
            .retain(|job_id| active_job_ids.contains(job_id));
        inner.config = config;
        inner.next_runs = next_runs;
        Ok(())
    }

    /// Dispatches every enabled startup job once, even if called repeatedly.
    pub async fn dispatch_startup<H: ScheduledJobHandler + ?Sized>(
        &self,
        now: DateTime<Utc>,
        handler: &H,
    ) -> Result<Vec<ScheduledJobInvocation>> {
        let (invocations, max_concurrent_jobs) = {
            let mut inner = self.lock()?;
            let config = inner.config.clone();
            if !config.enabled {
                return Ok(Vec::new());
            }
            let invocations = config
                .schedules
                .iter()
                .filter(|job| job.enabled && job.run_on_startup)
                .filter(|job| inner.startup_dispatched.insert(job.id.clone()))
                .map(|job| invocation(job, now, true))
                .collect();
            (invocations, config.max_concurrent_jobs)
        };
        execute_invocations(invocations, max_concurrent_jobs, handler).await
    }

    /// Dispatches jobs that are due at `now` and advances their next run atomically.
    pub async fn dispatch_due<H: ScheduledJobHandler + ?Sized>(
        &self,
        now: DateTime<Utc>,
        handler: &H,
    ) -> Result<Vec<ScheduledJobInvocation>> {
        let (invocations, max_concurrent_jobs) = {
            let mut inner = self.lock()?;
            let config = inner.config.clone();
            if !config.enabled {
                return Ok(Vec::new());
            }
            let mut invocations = Vec::new();
            for job in config.schedules.iter().filter(|job| job.enabled) {
                let due = inner
                    .next_runs
                    .get(&job.id)
                    .is_some_and(|next_run| *next_run <= now);
                if due {
                    inner
                        .next_runs
                        .insert(job.id.clone(), next_run_after(now, job.interval_secs)?);
                    invocations.push(invocation(job, now, false));
                }
            }
            (invocations, config.max_concurrent_jobs)
        };
        execute_invocations(invocations, max_concurrent_jobs, handler).await
    }

    /// Returns the next execution time for each enabled job in stable ID order.
    pub fn next_runs(&self) -> Result<BTreeMap<String, DateTime<Utc>>> {
        Ok(self.lock()?.next_runs.clone())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, SchedulerInner>> {
        self.inner
            .lock()
            .map_err(|error| EduMindError::Scheduler(format!("scheduler lock failed: {error}")))
    }
}

fn initialize_next_runs(
    config: &JobsConfig,
    now: DateTime<Utc>,
) -> Result<BTreeMap<String, DateTime<Utc>>> {
    config
        .schedules
        .iter()
        .filter(|job| job.enabled)
        .map(|job| Ok((job.id.clone(), next_run_after(now, job.interval_secs)?)))
        .collect()
}

fn next_run_after(now: DateTime<Utc>, interval_secs: u64) -> Result<DateTime<Utc>> {
    let seconds = i64::try_from(interval_secs).map_err(|_| {
        EduMindError::Scheduler("configured job interval does not fit a signed duration".to_owned())
    })?;
    now.checked_add_signed(Duration::seconds(seconds))
        .ok_or_else(|| {
            EduMindError::Scheduler("configured job interval overflows timestamp".to_owned())
        })
}

fn invocation(
    job: &ScheduledJobConfig,
    now: DateTime<Utc>,
    startup: bool,
) -> ScheduledJobInvocation {
    ScheduledJobInvocation {
        job_id: job.id.clone(),
        agent_id: job.agent_id.clone(),
        session_key: job.session_key.clone(),
        message: job.message.clone(),
        triggered_at: now,
        startup,
    }
}

async fn execute_invocations<H: ScheduledJobHandler + ?Sized>(
    invocations: Vec<ScheduledJobInvocation>,
    max_concurrent_jobs: u16,
    handler: &H,
) -> Result<Vec<ScheduledJobInvocation>> {
    let results = stream::iter(invocations)
        .map(|invocation| async move {
            handler.run(invocation.clone()).await?;
            Ok::<_, EduMindError>(invocation)
        })
        .buffer_unordered(usize::from(max_concurrent_jobs))
        .collect::<Vec<_>>()
        .await;
    let mut completed = Vec::with_capacity(results.len());
    for result in results {
        completed.push(result?);
    }
    completed.sort_by(|left, right| left.job_id.cmp(&right.job_id));
    Ok(completed)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use chrono::{Duration, TimeZone, Utc};

    use super::{ScheduledJobHandler, ScheduledJobInvocation, Scheduler};
    use crate::{
        config::types::{JobsConfig, ScheduledJobConfig},
        infra::Result,
    };

    #[derive(Default)]
    struct RecordingHandler {
        invocations: Mutex<Vec<ScheduledJobInvocation>>,
    }

    #[async_trait]
    impl ScheduledJobHandler for RecordingHandler {
        async fn run(&self, invocation: ScheduledJobInvocation) -> Result<()> {
            self.invocations.lock().unwrap().push(invocation);
            Ok(())
        }
    }

    fn config() -> JobsConfig {
        JobsConfig {
            enabled: true,
            max_concurrent_jobs: 2,
            schedules: vec![ScheduledJobConfig {
                id: "review".to_owned(),
                enabled: true,
                interval_secs: 60,
                agent_id: "master".to_owned(),
                session_key: "scheduled:review".to_owned(),
                message: "Run a review reminder.".to_owned(),
                run_on_startup: true,
            }],
        }
    }

    #[tokio::test]
    async fn startup_jobs_run_once_and_due_jobs_advance() {
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 8, 0, 0).single().unwrap();
        let scheduler = Scheduler::new(config(), now).unwrap();
        let handler = RecordingHandler::default();

        let startup = scheduler.dispatch_startup(now, &handler).await.unwrap();
        let repeated = scheduler.dispatch_startup(now, &handler).await.unwrap();
        let early = scheduler
            .dispatch_due(now + Duration::seconds(59), &handler)
            .await
            .unwrap();
        let due = scheduler
            .dispatch_due(now + Duration::seconds(60), &handler)
            .await
            .unwrap();

        assert_eq!(startup.len(), 1);
        assert!(startup[0].startup);
        assert!(repeated.is_empty());
        assert!(early.is_empty());
        assert_eq!(due.len(), 1);
        assert!(!due[0].startup);
        assert_eq!(handler.invocations.lock().unwrap().len(), 2);
    }
}
