use chrono::{DateTime, Utc};

use crate::{
    infra::Result,
    study::{LearningEngine, LearningInsights},
};

/// Bounded refresh coordinator for daily and explicit study-insight requests.
#[derive(Clone)]
pub struct LearningRefreshJob {
    engine: LearningEngine,
}

impl LearningRefreshJob {
    /// Creates a refresh job that never invokes a model provider.
    #[must_use]
    pub fn new(engine: LearningEngine) -> Self {
        Self { engine }
    }

    /// Returns today’s persisted snapshot or computes it once when it is absent.
    pub fn refresh_daily(&self, now: DateTime<Utc>) -> Result<LearningInsights> {
        if let Some(insights) = self.engine.latest()?
            && insights.generated_at.date_naive() == now.date_naive()
        {
            return Ok(insights);
        }
        self.engine.refresh(now)
    }

    /// Recomputes insights after an explicit user request using local canonical data only.
    pub fn refresh_manual(&self, now: DateTime<Utc>) -> Result<LearningInsights> {
        self.engine.refresh(now)
    }
}
