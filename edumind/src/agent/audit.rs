use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::infra::{EduMindError, Result};

type ToolRateLimitKey = (String, String);
type ToolCallHistory = VecDeque<DateTime<Utc>>;
type ToolCallBuckets = HashMap<ToolRateLimitKey, ToolCallHistory>;

/// Content-free outcome recorded for a tool invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAuditOutcome {
    Denied,
    RateLimited,
    Succeeded,
    Failed,
}

/// Bounded audit record that deliberately excludes tool arguments and outputs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolAuditEntry {
    pub at: DateTime<Utc>,
    pub agent_id: String,
    pub session_key: String,
    pub tool_name: String,
    pub outcome: ToolAuditOutcome,
    pub duration_ms: Option<u64>,
    pub detail: String,
}

impl ToolAuditEntry {
    /// Constructs a content-free audit entry.
    #[must_use]
    pub fn new(
        at: DateTime<Utc>,
        agent_id: impl Into<String>,
        session_key: impl Into<String>,
        tool_name: impl Into<String>,
        outcome: ToolAuditOutcome,
        duration_ms: Option<u64>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            at,
            agent_id: agent_id.into(),
            session_key: session_key.into(),
            tool_name: tool_name.into(),
            outcome,
            duration_ms,
            detail: detail.into(),
        }
    }
}

/// In-memory bounded ring buffer of content-free tool audit records.
#[derive(Clone)]
pub struct ToolAuditLog {
    capacity: usize,
    entries: Arc<Mutex<VecDeque<ToolAuditEntry>>>,
}

impl ToolAuditLog {
    /// Creates an audit log with a positive retained-entry capacity.
    pub fn new(capacity: usize) -> Result<Self> {
        if capacity == 0 {
            return Err(EduMindError::Tool(
                "tool audit capacity must be greater than zero".to_owned(),
            ));
        }
        Ok(Self {
            capacity,
            entries: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
        })
    }

    /// Appends an entry and evicts the oldest record when full.
    pub fn record(&self, entry: ToolAuditEntry) -> Result<()> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|error| EduMindError::Tool(format!("tool audit lock failed: {error}")))?;
        if entries.len() == self.capacity {
            entries.pop_front();
        }
        entries.push_back(entry);
        Ok(())
    }

    /// Returns retained records from oldest to newest.
    pub fn entries(&self) -> Result<Vec<ToolAuditEntry>> {
        self.entries
            .lock()
            .map(|entries| entries.iter().cloned().collect())
            .map_err(|error| EduMindError::Tool(format!("tool audit lock failed: {error}")))
    }
}

/// Per-agent, per-tool fixed-window rate limiter.
#[derive(Clone)]
pub struct ToolRateLimiter {
    limit_per_minute: u32,
    calls: Arc<Mutex<ToolCallBuckets>>,
}

impl ToolRateLimiter {
    /// Creates a limiter with a strictly positive per-minute call limit.
    pub fn new(limit_per_minute: u32) -> Result<Self> {
        if limit_per_minute == 0 {
            return Err(EduMindError::Tool(
                "tool rate limit must be greater than zero".to_owned(),
            ));
        }
        Ok(Self {
            limit_per_minute,
            calls: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Reserves one invocation slot when the agent/tool pair remains within its limit.
    pub fn try_acquire(&self, agent_id: &str, tool_name: &str, now: DateTime<Utc>) -> Result<bool> {
        let mut calls = self
            .calls
            .lock()
            .map_err(|error| EduMindError::Tool(format!("tool rate-limit lock failed: {error}")))?;
        let history = calls
            .entry((agent_id.to_owned(), tool_name.to_owned()))
            .or_default();
        let cutoff = now - Duration::minutes(1);
        history.retain(|timestamp| *timestamp > cutoff);
        if history.len() >= usize::try_from(self.limit_per_minute).unwrap_or(usize::MAX) {
            return Ok(false);
        }
        history.push_back(now);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::{ToolAuditEntry, ToolAuditLog, ToolAuditOutcome, ToolRateLimiter};

    #[test]
    fn audit_log_is_bounded_and_content_free() {
        let log = ToolAuditLog::new(2).unwrap();
        let now = Utc::now();
        for index in 0..3 {
            log.record(ToolAuditEntry::new(
                now + Duration::seconds(index),
                "master",
                "desktop:student",
                "memory_search",
                ToolAuditOutcome::Succeeded,
                Some(1),
                "completed",
            ))
            .unwrap();
        }

        let entries = log.entries().unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].at, now + Duration::seconds(1));
    }

    #[test]
    fn rate_limiter_resets_after_one_minute() {
        let limiter = ToolRateLimiter::new(2).unwrap();
        let now = Utc::now();

        assert!(limiter.try_acquire("master", "read", now).unwrap());
        assert!(
            limiter
                .try_acquire("master", "read", now + Duration::seconds(1))
                .unwrap()
        );
        assert!(
            !limiter
                .try_acquire("master", "read", now + Duration::seconds(2))
                .unwrap()
        );
        assert!(
            limiter
                .try_acquire("master", "read", now + Duration::seconds(60))
                .unwrap()
        );
    }
}
