use std::collections::BTreeMap;

use chrono::{DateTime, SecondsFormat, Utc};
use edumind_core::{
    LearningSignal, MasterySnapshot, StudyRecommendation, rank_recommendations, score_mastery,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{
    infra::{EduMindError, Result},
    memory::MemoryStore,
    student::{PlannerSchedule, PlannerScheduleEntry, StudentPageStore},
    study::{SrsCard, SrsService},
};

/// Persisted, deterministic study analytics assembled without a model call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LearningInsights {
    pub generated_at: DateTime<Utc>,
    pub available_minutes: u16,
    pub module_memory_records: usize,
    #[serde(default)]
    pub planner_conflicts: Vec<PlannerConflict>,
    #[serde(default)]
    pub mastery: Vec<MasterySnapshot>,
    #[serde(default)]
    pub recommendations: Vec<StudyRecommendation>,
}

/// One deterministic overlap detected in the canonical Student Planner schedule.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlannerConflict {
    pub day: String,
    pub first_entry_id: String,
    pub first_title: String,
    pub second_entry_id: String,
    pub second_title: String,
}

impl LearningInsights {
    /// Returns an empty local-first insight snapshot before the first refresh.
    #[must_use]
    pub fn empty(now: DateTime<Utc>) -> Self {
        Self {
            generated_at: now,
            available_minutes: 180,
            module_memory_records: 0,
            planner_conflicts: Vec::new(),
            mastery: Vec::new(),
            recommendations: Vec::new(),
        }
    }
}

/// Builds daily mastery and next-best-action insights from canonical local services.
#[derive(Clone)]
pub struct LearningEngine {
    srs: SrsService,
    student_pages: StudentPageStore,
    memory: MemoryStore,
}

impl LearningEngine {
    /// Creates an engine that reads canonical SRS, planner, and module-memory state.
    #[must_use]
    pub fn new(srs: SrsService, student_pages: StudentPageStore) -> Self {
        Self {
            memory: srs.store_handle().clone(),
            srs,
            student_pages,
        }
    }

    /// Refreshes the current daily snapshot using only deterministic offline inputs.
    pub fn refresh(&self, now: DateTime<Utc>) -> Result<LearningInsights> {
        let cards = self.srs.cards(None)?;
        let module_memory_records = self.memory.list()?.len();
        let schedule = self.student_pages.planner_schedule()?;
        let signals = cards
            .iter()
            .map(|card| signal_from_card(card, now))
            .collect::<Vec<_>>();
        let mastery = score_mastery(&signals);
        let recommendations = rank_recommendations(&mastery);
        let insights = LearningInsights {
            generated_at: now,
            available_minutes: available_minutes(&schedule),
            module_memory_records,
            planner_conflicts: planner_conflicts(&schedule),
            mastery,
            recommendations,
        };
        self.persist(&insights)?;
        Ok(insights)
    }

    /// Loads the newest persisted daily snapshot without recomputing it.
    pub fn latest(&self) -> Result<Option<LearningInsights>> {
        let connection = self.memory.connection()?;
        let refresh = connection
            .query_row(
                "SELECT day, generated_at, available_minutes, module_memory_records
                 FROM learning_refreshes
                 ORDER BY day DESC
                 LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((day, generated_at, available_minutes, module_memory_records)) = refresh else {
            return Ok(None);
        };
        let planner_conflicts = match connection
            .query_row(
                "SELECT planner_conflicts_json
                 FROM learning_refresh_metadata
                 WHERE day = ?1",
                params![&day],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            Some(value) => serde_json::from_str(&value)?,
            None => Vec::new(),
        };
        let mut statement = connection.prepare(
            "SELECT snapshot_json, recommendation_json
             FROM learning_daily_snapshots
             WHERE day = ?1
             ORDER BY concept_id ASC",
        )?;
        let mut rows = statement.query(params![day])?;
        let mut mastery = Vec::new();
        let mut recommendations = Vec::new();
        while let Some(row) = rows.next()? {
            let snapshot: String = row.get(0)?;
            let recommendation: String = row.get(1)?;
            mastery.push(serde_json::from_str(&snapshot)?);
            recommendations.push(serde_json::from_str(&recommendation)?);
        }
        recommendations.sort_by(|left: &StudyRecommendation, right: &StudyRecommendation| {
            right
                .priority_score
                .cmp(&left.priority_score)
                .then_with(|| left.concept_id.cmp(&right.concept_id))
        });
        let generated_at = DateTime::parse_from_rfc3339(&generated_at)
            .map_err(|error| {
                EduMindError::InvalidStoredData(format!(
                    "invalid learning refresh timestamp: {}",
                    error
                ))
            })?
            .with_timezone(&Utc);
        Ok(Some(LearningInsights {
            generated_at,
            available_minutes: u16::try_from(available_minutes).unwrap_or(u16::MAX),
            module_memory_records: usize::try_from(module_memory_records).unwrap_or(usize::MAX),
            planner_conflicts,
            mastery,
            recommendations,
        }))
    }

    fn persist(&self, insights: &LearningInsights) -> Result<()> {
        let day = insights.generated_at.date_naive().to_string();
        let recommendations = insights
            .recommendations
            .iter()
            .map(|recommendation| (recommendation.concept_id.clone(), recommendation))
            .collect::<BTreeMap<_, _>>();
        let mut connection = self.memory.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO learning_refreshes (
                day, generated_at, available_minutes, module_memory_records
             ) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(day) DO UPDATE SET
                generated_at = excluded.generated_at,
                available_minutes = excluded.available_minutes,
                module_memory_records = excluded.module_memory_records",
            params![
                day,
                format_timestamp(insights.generated_at),
                i64::from(insights.available_minutes),
                i64::try_from(insights.module_memory_records).unwrap_or(i64::MAX),
            ],
        )?;
        transaction.execute(
            "INSERT INTO learning_refresh_metadata (day, planner_conflicts_json)
             VALUES (?1, ?2)
             ON CONFLICT(day) DO UPDATE SET
                planner_conflicts_json = excluded.planner_conflicts_json",
            params![&day, serde_json::to_string(&insights.planner_conflicts)?,],
        )?;
        transaction.execute(
            "DELETE FROM learning_daily_snapshots WHERE day = ?1",
            params![insights.generated_at.date_naive().to_string()],
        )?;
        for snapshot in &insights.mastery {
            let recommendation = recommendations.get(&snapshot.concept_id).ok_or_else(|| {
                EduMindError::Study("learning snapshot is missing a recommendation".to_owned())
            })?;
            transaction.execute(
                "INSERT INTO learning_daily_snapshots (
                    day, concept_id, snapshot_json, recommendation_json
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    insights.generated_at.date_naive().to_string(),
                    snapshot.concept_id,
                    serde_json::to_string(snapshot)?,
                    serde_json::to_string(recommendation)?,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

fn signal_from_card(card: &SrsCard, now: DateTime<Utc>) -> LearningSignal {
    let last_activity = card.last_reviewed_at.unwrap_or(card.created_at);
    let elapsed_days = now.signed_duration_since(last_activity).num_days().max(0);
    let days_since_review = u32::try_from(elapsed_days).unwrap_or(u32::MAX);
    LearningSignal {
        concept_id: concept_id(card),
        correct: card.repetitions.saturating_sub(card.lapses),
        attempts: card.repetitions.saturating_add(card.lapses),
        days_since_review,
    }
}

fn concept_id(card: &SrsCard) -> String {
    let label = card
        .front
        .split_whitespace()
        .take(6)
        .collect::<Vec<_>>()
        .join(" ");
    format!("{}: {}", card.deck, label)
}

fn available_minutes(schedule: &PlannerSchedule) -> u16 {
    let days = u32::try_from(schedule.days.len().max(1)).unwrap_or(u32::MAX);
    let scheduled = schedule
        .days
        .iter()
        .flat_map(|day| &day.entries)
        .map(|entry| block_minutes(&entry.start, &entry.end))
        .sum::<u32>();
    let average_scheduled = scheduled / days;
    u16::try_from(180_u32.saturating_sub(average_scheduled.min(150)).max(30)).unwrap_or(30)
}

fn planner_conflicts(schedule: &PlannerSchedule) -> Vec<PlannerConflict> {
    let mut conflicts = Vec::new();
    for day in &schedule.days {
        for (index, first) in day.entries.iter().enumerate() {
            let Some((first_start, first_end)) = planner_window(first) else {
                continue;
            };
            for second in day.entries.iter().skip(index + 1) {
                let Some((second_start, second_end)) = planner_window(second) else {
                    continue;
                };
                if second_start >= first_end {
                    break;
                }
                if first_start < second_end && second_start < first_end {
                    conflicts.push(PlannerConflict {
                        day: day.day.clone(),
                        first_entry_id: first.id.clone(),
                        first_title: first.title.clone(),
                        second_entry_id: second.id.clone(),
                        second_title: second.title.clone(),
                    });
                }
            }
        }
    }
    conflicts
}

fn planner_window(entry: &PlannerScheduleEntry) -> Option<(u32, u32)> {
    Some((
        parse_clock_minutes(&entry.start)?,
        parse_clock_minutes(&entry.end)?,
    ))
}

fn block_minutes(start: &str, end: &str) -> u32 {
    let Some(start) = parse_clock_minutes(start) else {
        return 0;
    };
    let Some(end) = parse_clock_minutes(end) else {
        return 0;
    };
    end.saturating_sub(start)
}

fn parse_clock_minutes(value: &str) -> Option<u32> {
    let value = value.rsplit_once('T').map_or(value, |(_, time)| time);
    let value = value.split('+').next().unwrap_or(value);
    let (hours, minutes) = value.split_once(':')?;
    let hours = hours.parse::<u32>().ok()?;
    let minutes = minutes
        .chars()
        .take(2)
        .collect::<String>()
        .parse::<u32>()
        .ok()?;
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

fn format_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::json;

    use crate::{
        memory::{MemoryStore, NewMemory},
        student::{StudentPageRecordInput, StudentPageStore},
        study::{LearningEngine, NewSrsCard, SrsService},
    };

    #[test]
    fn refresh_is_deterministic_and_prioritizes_overdue_cards() {
        let store = MemoryStore::in_memory().unwrap();
        let srs = SrsService::new(store.clone());
        let pages = StudentPageStore::new(store.clone());
        let engine = LearningEngine::new(srs.clone(), pages);
        let now = Utc.with_ymd_and_hms(2026, 7, 19, 8, 0, 0).unwrap();
        srs.create_card(
            NewSrsCard::new("Calculus limits", "Derivative foundations"),
            now - Duration::days(25),
        )
        .unwrap();
        let history = srs
            .create_card(NewSrsCard::new("History timeline", "Context"), now)
            .unwrap();
        srs.review(history.id, 5, now).unwrap();
        store
            .store(
                NewMemory::new("class-notes", "Calculus unit outline", "note"),
                now,
            )
            .unwrap();

        let insights = engine.refresh(now).unwrap();
        let restored = engine.latest().unwrap().unwrap();

        assert_eq!(insights, restored);
        assert!(insights.recommendations[0].concept_id.contains("Calculus"));
        assert_eq!(insights.module_memory_records, 1);
    }

    #[test]
    fn refresh_reports_overlapping_canonical_planner_blocks() {
        let store = MemoryStore::in_memory().unwrap();
        let srs = SrsService::new(store.clone());
        let pages = StudentPageStore::new(store);
        let engine = LearningEngine::new(srs, pages.clone());
        let now = Utc.with_ymd_and_hms(2026, 7, 19, 8, 0, 0).unwrap();
        pages
            .save_all(
                "planner",
                vec![
                    StudentPageRecordInput::new(
                        "calculus",
                        json!({
                            "kind": "schedule-block",
                            "day": "Monday",
                            "title": "Calculus",
                            "start": "09:00",
                            "end": "10:30"
                        }),
                        now,
                    ),
                    StudentPageRecordInput::new(
                        "lab",
                        json!({
                            "kind": "schedule-block",
                            "day": "Monday",
                            "title": "Physics lab",
                            "start": "10:00",
                            "end": "11:00"
                        }),
                        now,
                    ),
                ],
                "desktop",
                now,
            )
            .unwrap();

        let insights = engine.refresh(now).unwrap();

        assert_eq!(insights.planner_conflicts.len(), 1);
        assert_eq!(insights.planner_conflicts[0].day, "Monday");
        assert_eq!(insights.planner_conflicts[0].first_title, "Calculus");
        assert_eq!(insights.planner_conflicts[0].second_title, "Physics lab");
    }
}
