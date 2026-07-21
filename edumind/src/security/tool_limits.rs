use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, NaiveDate, Utc};

use crate::{
    agent::tools::ToolClass,
    config::types::ToolDailyLimitsConfig,
    infra::{EduMindError, Result},
};

type DailyKey = (NaiveDate, String);

/// The daily quota that denied a tool call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolDailyLimitReason {
    Total,
    Network,
    Execution,
}

impl fmt::Display for ToolDailyLimitReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Total => "total",
            Self::Network => "network",
            Self::Execution => "execution",
        };
        formatter.write_str(value)
    }
}

/// Outcome of recording a daily tool call attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolDailyLimitDecision {
    Allowed,
    Denied(ToolDailyLimitReason),
}

#[derive(Clone, Copy, Debug, Default)]
struct DailyCallCounts {
    total: u32,
    network: u32,
    execution: u32,
}

/// In-memory per-agent UTC-day quota tracker for total, network, and execution tools.
#[derive(Clone)]
pub struct ToolDailyLimiter {
    limits: ToolDailyLimitsConfig,
    calls: Arc<Mutex<HashMap<DailyKey, DailyCallCounts>>>,
}

impl ToolDailyLimiter {
    /// Creates a limiter that shares quotas across its clones.
    #[must_use]
    pub fn new(limits: ToolDailyLimitsConfig) -> Self {
        Self {
            limits,
            calls: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Reserves a quota slot for one authorized tool call.
    pub fn try_acquire(
        &self,
        agent_id: &str,
        class: ToolClass,
        now: DateTime<Utc>,
    ) -> Result<ToolDailyLimitDecision> {
        let day = now.date_naive();
        let mut calls = self.calls.lock().map_err(|error| {
            EduMindError::Security(format!("daily tool-limit lock failed: {error}"))
        })?;
        calls.retain(|(recorded_day, _), _| *recorded_day >= day);
        let counts = calls.entry((day, agent_id.to_owned())).or_default();
        if reached(counts.total, self.limits.max_total_per_day) {
            return Ok(ToolDailyLimitDecision::Denied(ToolDailyLimitReason::Total));
        }
        if class == ToolClass::Network && reached(counts.network, self.limits.max_network_per_day) {
            return Ok(ToolDailyLimitDecision::Denied(
                ToolDailyLimitReason::Network,
            ));
        }
        if class == ToolClass::Execution
            && reached(counts.execution, self.limits.max_execution_per_day)
        {
            return Ok(ToolDailyLimitDecision::Denied(
                ToolDailyLimitReason::Execution,
            ));
        }
        counts.total = counts.total.saturating_add(1);
        if class == ToolClass::Network {
            counts.network = counts.network.saturating_add(1);
        }
        if class == ToolClass::Execution {
            counts.execution = counts.execution.saturating_add(1);
        }
        Ok(ToolDailyLimitDecision::Allowed)
    }
}

fn reached(count: u32, limit: u32) -> bool {
    count >= limit
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::{ToolDailyLimitDecision, ToolDailyLimitReason, ToolDailyLimiter};
    use crate::{agent::ToolClass, config::types::ToolDailyLimitsConfig};

    #[test]
    fn enforces_total_network_and_execution_quotas_by_utc_day() {
        let limiter = ToolDailyLimiter::new(ToolDailyLimitsConfig {
            max_total_per_day: 3,
            max_network_per_day: 1,
            max_execution_per_day: 1,
        });
        let now = Utc::now();

        assert_eq!(
            limiter
                .try_acquire("master", ToolClass::Network, now)
                .unwrap(),
            ToolDailyLimitDecision::Allowed
        );
        assert_eq!(
            limiter
                .try_acquire("master", ToolClass::Network, now)
                .unwrap(),
            ToolDailyLimitDecision::Denied(ToolDailyLimitReason::Network)
        );
        assert_eq!(
            limiter
                .try_acquire("master", ToolClass::Execution, now)
                .unwrap(),
            ToolDailyLimitDecision::Allowed
        );
        assert_eq!(
            limiter.try_acquire("master", ToolClass::Read, now).unwrap(),
            ToolDailyLimitDecision::Allowed
        );
        assert_eq!(
            limiter.try_acquire("master", ToolClass::Read, now).unwrap(),
            ToolDailyLimitDecision::Denied(ToolDailyLimitReason::Total)
        );
        assert_eq!(
            limiter
                .try_acquire("master", ToolClass::Read, now + Duration::days(1))
                .unwrap(),
            ToolDailyLimitDecision::Allowed
        );
    }
}
