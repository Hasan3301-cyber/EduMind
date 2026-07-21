use std::{collections::BTreeMap, time::Duration};

use chrono::{DateTime, Utc};
use rusqlite::{Row, params};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::{config::types::HermesConfig, infra::Result};

use super::MemoryStore;

/// One confidence insight generated from observed skill outcomes in durable memories.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HermesSkillInsight {
    pub skill: String,
    pub confidence: f64,
    pub observations: usize,
    pub successes: usize,
    pub message: String,
}

/// Persisted output from one bounded Hermes learning cycle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HermesCycle {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub memories_considered: usize,
    #[serde(default)]
    pub insights: Vec<HermesSkillInsight>,
}

/// Local learning loop that refines skill confidence from explicit memory outcomes.
#[derive(Clone)]
pub struct HermesLearningLoop {
    store: MemoryStore,
    config: HermesConfig,
}

impl HermesLearningLoop {
    /// Creates a bounded Hermes loop using the configured local memory database.
    pub fn new(store: MemoryStore, config: HermesConfig) -> Result<Self> {
        if config.max_cycles_per_day == 0 || config.cooldown_secs == 0 {
            return Err(crate::infra::EduMindError::ConfigValidation(
                "Hermes requires non-zero max_cycles_per_day and cooldown_secs".to_owned(),
            ));
        }
        Ok(Self { store, config })
    }

    /// Lists persisted cycles in most-recent-first order.
    pub fn cycles(&self, limit: usize) -> Result<Vec<HermesCycle>> {
        self.store.list_hermes_cycles(limit)
    }

    /// Runs one cycle when enabled and outside the configured cooldown/daily bounds.
    pub fn run_once(&self, now: DateTime<Utc>) -> Result<Option<HermesCycle>> {
        if !self.config.enabled {
            return Ok(None);
        }
        let cycles = self.store.list_hermes_cycles(10_000)?;
        if cycles
            .iter()
            .filter(|cycle| cycle.completed_at.date_naive() == now.date_naive())
            .count()
            >= usize::from(self.config.max_cycles_per_day)
        {
            return Ok(None);
        }
        let cooldown =
            chrono::Duration::seconds(i64::try_from(self.config.cooldown_secs).unwrap_or(i64::MAX));
        if cycles
            .first()
            .is_some_and(|last| now.signed_duration_since(last.completed_at) < cooldown)
        {
            return Ok(None);
        }

        let records = self.store.list()?;
        let cycle = HermesCycle {
            id: Uuid::new_v4(),
            started_at: now,
            completed_at: now,
            memories_considered: records.len(),
            insights: build_skill_insights(&records),
        };
        self.store.save_hermes_cycle(&cycle)?;
        Ok(Some(cycle))
    }

    /// Starts a low-frequency background loop only when Hermes is explicitly enabled.
    #[must_use]
    pub fn spawn(&self) -> Option<JoinHandle<()>> {
        if !self.config.enabled {
            return None;
        }
        let loop_service = self.clone();
        let interval = Duration::from_secs(self.config.cooldown_secs);
        Some(tokio::spawn(async move {
            loop {
                let _ = loop_service.run_once(Utc::now());
                tokio::time::sleep(interval).await;
            }
        }))
    }
}

impl MemoryStore {
    /// Persists one completed Hermes cycle as an append-only local record.
    pub fn save_hermes_cycle(&self, cycle: &HermesCycle) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO hermes_cycles (id, cycle_json, created_at) VALUES (?1, ?2, ?3)",
            params![
                cycle.id.to_string(),
                serde_json::to_string(cycle)?,
                cycle.completed_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Loads Hermes cycle history in deterministic most-recent-first order.
    pub fn list_hermes_cycles(&self, limit: usize) -> Result<Vec<HermesCycle>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT cycle_json FROM hermes_cycles
             ORDER BY created_at DESC, id ASC
             LIMIT ?1",
        )?;
        let mut rows = statement.query(params![i64::try_from(limit).unwrap_or(i64::MAX)])?;
        let mut cycles = Vec::new();
        while let Some(row) = rows.next()? {
            cycles.push(cycle_from_row(row)?);
        }
        Ok(cycles)
    }
}

fn build_skill_insights(records: &[super::MemoryRecord]) -> Vec<HermesSkillInsight> {
    let mut observations = BTreeMap::<String, (usize, usize)>::new();
    for record in records {
        let Some(success) = success_outcome(&record.metadata) else {
            continue;
        };
        for skill in skills(&record.metadata) {
            let entry = observations.entry(skill).or_insert((0, 0));
            entry.0 += 1;
            if success {
                entry.1 += 1;
            }
        }
    }
    observations
        .into_iter()
        .map(|(skill, (observation_count, successes))| {
            let confidence = (successes + 1) as f64 / (observation_count + 2) as f64;
            HermesSkillInsight {
                message: format!(
                    "{skill} has {successes}/{observation_count} successful observed outcome(s); smoothed confidence is {:.0}%.",
                    confidence * 100.0
                ),
                skill,
                confidence,
                observations: observation_count,
                successes,
            }
        })
        .collect()
}

fn skills(metadata: &serde_json::Value) -> Vec<String> {
    let mut skills = metadata
        .get("skill")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|skill| !skill.is_empty())
        .map(ToOwned::to_owned)
        .into_iter()
        .collect::<Vec<_>>();
    skills.extend(
        metadata
            .get("skills")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|skill| !skill.is_empty())
            .map(ToOwned::to_owned),
    );
    skills.sort();
    skills.dedup();
    skills
}

fn success_outcome(metadata: &serde_json::Value) -> Option<bool> {
    metadata
        .get("success")
        .and_then(serde_json::Value::as_bool)
        .or_else(|| {
            metadata
                .get("outcome")
                .and_then(serde_json::Value::as_str)
                .and_then(|outcome| match outcome.trim().to_lowercase().as_str() {
                    "success" | "passed" | "complete" => Some(true),
                    "failure" | "failed" | "incomplete" => Some(false),
                    _ => None,
                })
        })
}

fn cycle_from_row(row: &Row<'_>) -> rusqlite::Result<HermesCycle> {
    let encoded: String = row.get(0)?;
    serde_json::from_str(&encoded).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use super::HermesLearningLoop;
    use crate::{
        config::types::HermesConfig,
        memory::{MemoryStore, NewMemory},
    };

    fn timestamp() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap()
    }

    #[test]
    fn disabled_hermes_does_not_create_cycles() {
        let loop_service =
            HermesLearningLoop::new(MemoryStore::in_memory().unwrap(), HermesConfig::default())
                .unwrap();

        assert!(loop_service.run_once(timestamp()).unwrap().is_none());
        assert!(loop_service.cycles(10).unwrap().is_empty());
    }

    #[test]
    fn creates_persists_and_cooldown_gates_skill_insights() {
        let store = MemoryStore::in_memory().unwrap();
        let mut first = NewMemory::new(
            "study",
            "A successful retrieval practice session",
            "outcome",
        );
        first.metadata = json!({"skill": "retrieval", "success": true});
        store.store(first, timestamp()).unwrap();
        let mut second = NewMemory::new("study", "A failed retrieval practice session", "outcome");
        second.metadata = json!({"skill": "retrieval", "outcome": "failed"});
        store.store(second, timestamp()).unwrap();
        let loop_service = HermesLearningLoop::new(
            store,
            HermesConfig {
                enabled: true,
                max_cycles_per_day: 2,
                cooldown_secs: 60,
            },
        )
        .unwrap();

        let cycle = loop_service.run_once(timestamp()).unwrap().unwrap();
        let blocked = loop_service
            .run_once(timestamp() + chrono::Duration::seconds(30))
            .unwrap();

        assert_eq!(cycle.insights[0].skill, "retrieval");
        assert_eq!(cycle.insights[0].confidence, 0.5);
        assert!(blocked.is_none());
        assert_eq!(loop_service.cycles(10).unwrap().len(), 1);
    }
}
