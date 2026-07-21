use std::collections::BTreeSet;

use chrono::{DateTime, NaiveTime, Utc};
use rusqlite::{Connection, Row, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    infra::{EduMindError, Result},
    memory::{HybridMemory, MemoryId, MemoryRecord, MemoryStore, NewMemory},
};

const PLANNER_DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// The two canonical editable Student OS page identifiers.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StudentPage {
    StudentOs,
    StudentPlanner,
}

impl StudentPage {
    /// Normalizes supported aliases to `student-os` or `student-planner`.
    pub fn normalize(value: &str) -> Result<Self> {
        let normalized = value
            .trim()
            .to_ascii_lowercase()
            .chars()
            .map(|character| {
                if character == '_' || character.is_ascii_whitespace() {
                    '-'
                } else {
                    character
                }
            })
            .collect::<String>()
            .split('-')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        match normalized.as_str() {
            "student-os" | "studentos" | "os" => Ok(Self::StudentOs),
            "student-planner" | "studentplanner" | "planner" => Ok(Self::StudentPlanner),
            _ => Err(EduMindError::Student(format!(
                "unsupported student page `{value}`; expected student-os or student-planner"
            ))),
        }
    }

    /// Returns the canonical persisted page identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StudentOs => "student-os",
            Self::StudentPlanner => "student-planner",
        }
    }
}

/// One canonical key/value record, including tombstones needed for offline sync.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudentPageRecord {
    pub key: String,
    pub value: Value,
    pub updated_at: DateTime<Utc>,
    pub source: String,
    pub deleted: bool,
}

/// A client-provided record mutation whose timestamp drives last-write-wins behavior.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudentPageRecordInput {
    pub key: String,
    pub value: Value,
    pub updated_at: DateTime<Utc>,
}

impl StudentPageRecordInput {
    /// Creates a timestamped state-record input.
    #[must_use]
    pub fn new(key: impl Into<String>, value: Value, updated_at: DateTime<Utc>) -> Self {
        Self {
            key: key.into(),
            value,
            updated_at,
        }
    }
}

/// A complete page snapshot. Records include tombstones so offline clients converge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudentPageSnapshot {
    pub page: StudentPage,
    pub records: Vec<StudentPageRecord>,
    pub count: usize,
    pub updated_at: Option<DateTime<Utc>>,
}

/// A typed, deterministic seven-day projection of canonical Student Planner records.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannerSchedule {
    pub days: Vec<PlannerScheduleDay>,
    pub ignored_records: usize,
    pub updated_at: Option<DateTime<Utc>>,
}

/// One day in a canonical planner schedule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannerScheduleDay {
    pub day: String,
    pub entries: Vec<PlannerScheduleEntry>,
}

/// A validated planner block suitable for routine-generation workflows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannerScheduleEntry {
    pub id: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub source: String,
}

/// Result of an individual last-write-wins mutation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudentPageMutation {
    pub record: StudentPageRecord,
    pub applied: bool,
}

/// Content-free audit event emitted for page mutations and snapshots.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudentPageEvent {
    pub id: String,
    pub page: StudentPage,
    pub event_type: String,
    pub key: Option<String>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

/// Compact state overview suitable for agents that only need page health and provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudentPageSummary {
    pub page: StudentPage,
    pub active_records: usize,
    pub tombstones: usize,
    pub updated_at: Option<DateTime<Utc>>,
    pub sources: Vec<String>,
}

/// Canonical persistence and LWW merge service for Student OS and Student Planner pages.
#[derive(Clone)]
pub struct StudentPageStore {
    store: MemoryStore,
}

impl StudentPageStore {
    /// Creates a state service over the shared local memory database.
    #[must_use]
    pub fn new(store: MemoryStore) -> Self {
        Self { store }
    }

    /// Returns the underlying persistent store handle.
    #[must_use]
    pub fn store_handle(&self) -> &MemoryStore {
        &self.store
    }

    /// Loads every record for a canonical page, including tombstones for deterministic sync.
    pub fn load(&self, page: impl AsRef<str>) -> Result<StudentPageSnapshot> {
        let page = StudentPage::normalize(page.as_ref())?;
        let connection = self.store.connection()?;
        read_snapshot(&connection, page)
    }

    /// Projects planner records into a validated, fixed-order weekly schedule for Module 4.
    pub fn planner_schedule(&self) -> Result<PlannerSchedule> {
        Ok(planner_schedule_from_snapshot(
            &self.load(StudentPage::StudentPlanner.as_str())?,
        ))
    }

    /// Applies a full page snapshot with timestamp-aware tombstones for keys omitted by the client.
    pub fn save_all(
        &self,
        page: impl AsRef<str>,
        records: Vec<StudentPageRecordInput>,
        source: impl Into<String>,
        snapshot_updated_at: DateTime<Utc>,
    ) -> Result<StudentPageSnapshot> {
        let page = StudentPage::normalize(page.as_ref())?;
        let source = normalize_source(source.into())?;
        validate_record_inputs(&records)?;
        let incoming_keys = records
            .iter()
            .map(|record| normalize_key(&record.key))
            .collect::<Result<BTreeSet<_>>>()?;
        let mut connection = self.store.connection()?;
        let transaction = connection.transaction()?;
        let existing_active_keys = active_keys(&transaction, page)?;
        let mut applied_mutations = 0;
        for record in &records {
            let key = normalize_key(&record.key)?;
            if apply_record(
                &transaction,
                page,
                &key,
                &record.value,
                record.updated_at,
                &source,
                false,
            )? {
                applied_mutations += 1;
                append_event(
                    &transaction,
                    page,
                    "record_upserted",
                    Some(&key),
                    json!({"source": source.clone(), "updated_at": record.updated_at}),
                    record.updated_at,
                )?;
            }
        }
        for key in existing_active_keys.difference(&incoming_keys) {
            if apply_record(
                &transaction,
                page,
                key,
                &Value::Null,
                snapshot_updated_at,
                &source,
                true,
            )? {
                applied_mutations += 1;
                append_event(
                    &transaction,
                    page,
                    "record_deleted",
                    Some(key),
                    json!({"source": source.clone(), "updated_at": snapshot_updated_at}),
                    snapshot_updated_at,
                )?;
            }
        }
        append_event(
            &transaction,
            page,
            "snapshot_saved",
            None,
            json!({
                "source": source,
                "submitted_records": records.len(),
                "applied_mutations": applied_mutations,
                "snapshot_updated_at": snapshot_updated_at,
            }),
            snapshot_updated_at,
        )?;
        transaction.commit()?;
        drop(connection);
        self.load(page.as_str())
    }

    /// Upserts one record only when its timestamp is at least as new as the stored record.
    pub fn upsert_record(
        &self,
        page: impl AsRef<str>,
        input: StudentPageRecordInput,
        source: impl Into<String>,
    ) -> Result<StudentPageMutation> {
        let page = StudentPage::normalize(page.as_ref())?;
        let source = normalize_source(source.into())?;
        let key = normalize_key(&input.key)?;
        let mut connection = self.store.connection()?;
        let transaction = connection.transaction()?;
        let applied = apply_record(
            &transaction,
            page,
            &key,
            &input.value,
            input.updated_at,
            &source,
            false,
        )?;
        if applied {
            append_event(
                &transaction,
                page,
                "record_upserted",
                Some(&key),
                json!({"source": source, "updated_at": input.updated_at}),
                input.updated_at,
            )?;
        }
        transaction.commit()?;
        drop(connection);
        let record = self.read_record(page, &key)?.ok_or_else(|| {
            EduMindError::InvalidStoredData(format!(
                "student page record `{key}` disappeared after upsert"
            ))
        })?;
        Ok(StudentPageMutation { record, applied })
    }

    /// Writes a timestamp-aware tombstone rather than physically deleting a page record.
    pub fn delete_record(
        &self,
        page: impl AsRef<str>,
        key: impl AsRef<str>,
        updated_at: DateTime<Utc>,
        source: impl Into<String>,
    ) -> Result<StudentPageMutation> {
        let page = StudentPage::normalize(page.as_ref())?;
        let source = normalize_source(source.into())?;
        let key = normalize_key(key.as_ref())?;
        let mut connection = self.store.connection()?;
        let transaction = connection.transaction()?;
        let applied = apply_record(
            &transaction,
            page,
            &key,
            &Value::Null,
            updated_at,
            &source,
            true,
        )?;
        if applied {
            append_event(
                &transaction,
                page,
                "record_deleted",
                Some(&key),
                json!({"source": source, "updated_at": updated_at}),
                updated_at,
            )?;
        }
        transaction.commit()?;
        drop(connection);
        let record = self.read_record(page, &key)?.ok_or_else(|| {
            EduMindError::InvalidStoredData(format!(
                "student page record `{key}` disappeared after delete"
            ))
        })?;
        Ok(StudentPageMutation { record, applied })
    }

    /// Returns a compact page overview without exposing individual record values.
    pub fn summarize(&self, page: impl AsRef<str>) -> Result<StudentPageSummary> {
        let snapshot = self.load(page)?;
        let mut sources = BTreeSet::new();
        let mut tombstones = 0;
        for record in &snapshot.records {
            sources.insert(record.source.clone());
            if record.deleted {
                tombstones += 1;
            }
        }
        Ok(StudentPageSummary {
            page: snapshot.page,
            active_records: snapshot.count,
            tombstones,
            updated_at: snapshot.updated_at,
            sources: sources.into_iter().collect(),
        })
    }

    /// Lists content-free page audit events in chronological order.
    pub fn events(&self, page: impl AsRef<str>, limit: usize) -> Result<Vec<StudentPageEvent>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let page = StudentPage::normalize(page.as_ref())?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let connection = self.store.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, page, event_type, key, metadata_json, created_at
             FROM student_page_events
             WHERE page = ?1
             ORDER BY created_at ASC, rowid ASC
             LIMIT ?2",
        )?;
        let mut rows = statement.query(params![page.as_str(), limit])?;
        let mut events = Vec::new();
        while let Some(row) = rows.next()? {
            events.push(event_from_row(row)?);
        }
        Ok(events)
    }

    /// Serializes and embeds the current canonical page snapshot for agent retrieval.
    pub async fn index_snapshot(
        &self,
        memory: &HybridMemory,
        page: impl AsRef<str>,
        now: DateTime<Utc>,
    ) -> Result<MemoryRecord> {
        let page = StudentPage::normalize(page.as_ref())?;
        let snapshot = self.load(page.as_str())?;
        let content = snapshot_for_index(&snapshot)?;
        let mut input = NewMemory::new(page.as_str(), content, "student_page_snapshot");
        input.metadata = json!({
            "tags": ["student-page", page.as_str(), "planner-state"],
            "page": page.as_str(),
            "record_count": snapshot.count,
            "updated_at": snapshot.updated_at,
        });
        let record = match self.indexed_memory_id(page)? {
            Some(memory_id) => match memory.update_memory(memory_id, input.clone(), now).await? {
                Some(record) => record,
                None => memory.store_memory(input, now).await?,
            },
            None => memory.store_memory(input, now).await?,
        };
        self.set_indexed_memory_id(page, record.id, now)?;
        Ok(record)
    }

    fn read_record(&self, page: StudentPage, key: &str) -> Result<Option<StudentPageRecord>> {
        let connection = self.store.connection()?;
        read_record(&connection, page, key)
    }

    fn indexed_memory_id(&self, page: StudentPage) -> Result<Option<MemoryId>> {
        let connection = self.store.connection()?;
        let mut statement =
            connection.prepare("SELECT memory_id FROM student_page_indexes WHERE page = ?1")?;
        let mut rows = statement.query(params![page.as_str()])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let value = row.get::<_, String>("memory_id")?;
        Uuid::parse_str(&value)
            .map(MemoryId)
            .map(Some)
            .map_err(|error| {
                EduMindError::InvalidStoredData(format!(
                    "invalid student page index memory id `{value}`: {error}"
                ))
            })
    }

    fn set_indexed_memory_id(
        &self,
        page: StudentPage,
        memory_id: MemoryId,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let connection = self.store.connection()?;
        connection.execute(
            "INSERT INTO student_page_indexes (page, memory_id, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(page) DO UPDATE SET
                 memory_id = excluded.memory_id,
                 updated_at = excluded.updated_at",
            params![
                page.as_str(),
                memory_id.to_string(),
                format_lww_timestamp(now),
            ],
        )?;
        Ok(())
    }
}

/// Convenience wrapper matching the phase contract for semantic Student Page indexing.
pub async fn index_student_page_snapshot(
    store: &StudentPageStore,
    memory: &HybridMemory,
    page: impl AsRef<str>,
    now: DateTime<Utc>,
) -> Result<MemoryRecord> {
    store.index_snapshot(memory, page, now).await
}

fn read_snapshot(connection: &Connection, page: StudentPage) -> Result<StudentPageSnapshot> {
    let mut statement = connection.prepare(
        "SELECT page, key, value_json, updated_at, source, deleted
         FROM student_page_state
         WHERE page = ?1
         ORDER BY key ASC",
    )?;
    let mut rows = statement.query(params![page.as_str()])?;
    let mut records = Vec::new();
    while let Some(row) = rows.next()? {
        records.push(record_from_row(row)?);
    }
    let count = records.iter().filter(|record| !record.deleted).count();
    let updated_at = records.iter().map(|record| record.updated_at).max();
    Ok(StudentPageSnapshot {
        page,
        records,
        count,
        updated_at,
    })
}

fn planner_schedule_from_snapshot(snapshot: &StudentPageSnapshot) -> PlannerSchedule {
    let mut days = PLANNER_DAYS
        .iter()
        .map(|day| PlannerScheduleDay {
            day: (*day).to_owned(),
            entries: Vec::new(),
        })
        .collect::<Vec<_>>();
    let mut ignored_records = 0;

    for record in snapshot.records.iter().filter(|record| !record.deleted) {
        if !is_planner_schedule_candidate(record) {
            continue;
        }
        let Some((day_index, entry)) = planner_schedule_entry(record) else {
            ignored_records += 1;
            continue;
        };
        days[day_index].entries.push(entry);
    }

    for day in &mut days {
        day.entries.sort_by(|left, right| {
            left.start
                .cmp(&right.start)
                .then_with(|| left.end.cmp(&right.end))
                .then_with(|| left.title.cmp(&right.title))
                .then_with(|| left.id.cmp(&right.id))
        });
    }

    PlannerSchedule {
        days,
        ignored_records,
        updated_at: snapshot.updated_at,
    }
}

fn is_planner_schedule_candidate(record: &StudentPageRecord) -> bool {
    let Some(value) = record.value.as_object() else {
        return false;
    };
    value
        .get("kind")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("schedule-block"))
        || value.contains_key("day")
        || (value.contains_key("start") && value.contains_key("end"))
}

fn planner_schedule_entry(record: &StudentPageRecord) -> Option<(usize, PlannerScheduleEntry)> {
    let value = record.value.as_object()?;
    let day_index = planner_day_index(value.get("day")?.as_str()?)?;
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())?
        .to_owned();
    let (start_time, start) = normalized_planner_time(value.get("start")?.as_str()?)?;
    let (end_time, end) = normalized_planner_time(value.get("end")?.as_str()?)?;
    if start_time >= end_time {
        return None;
    }

    Some((
        day_index,
        PlannerScheduleEntry {
            id: record.key.clone(),
            title,
            start,
            end,
            source: record.source.clone(),
        },
    ))
}

fn planner_day_index(value: &str) -> Option<usize> {
    match value.trim().to_ascii_lowercase().as_str() {
        "monday" | "mon" => Some(0),
        "tuesday" | "tue" | "tues" => Some(1),
        "wednesday" | "wed" => Some(2),
        "thursday" | "thu" | "thur" | "thurs" => Some(3),
        "friday" | "fri" => Some(4),
        "saturday" | "sat" => Some(5),
        "sunday" | "sun" => Some(6),
        _ => None,
    }
}

fn normalized_planner_time(value: &str) -> Option<(NaiveTime, String)> {
    let time = NaiveTime::parse_from_str(value.trim(), "%H:%M").ok()?;
    Some((time, time.format("%H:%M").to_string()))
}

fn active_keys(transaction: &Transaction<'_>, page: StudentPage) -> Result<BTreeSet<String>> {
    let mut statement = transaction
        .prepare("SELECT key FROM student_page_state WHERE page = ?1 AND deleted = 0")?;
    let mut rows = statement.query(params![page.as_str()])?;
    let mut keys = BTreeSet::new();
    while let Some(row) = rows.next()? {
        keys.insert(row.get("key")?);
    }
    Ok(keys)
}

fn apply_record(
    transaction: &Transaction<'_>,
    page: StudentPage,
    key: &str,
    value: &Value,
    updated_at: DateTime<Utc>,
    source: &str,
    deleted: bool,
) -> Result<bool> {
    let changed = transaction.execute(
        "INSERT INTO student_page_state (page, key, value_json, updated_at, source, deleted)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(page, key) DO UPDATE SET
             value_json = excluded.value_json,
             updated_at = excluded.updated_at,
             source = excluded.source,
             deleted = excluded.deleted
         WHERE excluded.updated_at >= student_page_state.updated_at",
        params![
            page.as_str(),
            key,
            serde_json::to_string(value)?,
            format_lww_timestamp(updated_at),
            source,
            i64::from(deleted),
        ],
    )?;
    Ok(changed > 0)
}

fn append_event(
    transaction: &Transaction<'_>,
    page: StudentPage,
    event_type: &str,
    key: Option<&str>,
    metadata: Value,
    created_at: DateTime<Utc>,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO student_page_events (id, page, event_type, key, metadata_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            Uuid::new_v4().to_string(),
            page.as_str(),
            event_type,
            key,
            serde_json::to_string(&metadata)?,
            format_lww_timestamp(created_at),
        ],
    )?;
    Ok(())
}

fn read_record(
    connection: &Connection,
    page: StudentPage,
    key: &str,
) -> Result<Option<StudentPageRecord>> {
    let mut statement = connection.prepare(
        "SELECT page, key, value_json, updated_at, source, deleted
         FROM student_page_state WHERE page = ?1 AND key = ?2",
    )?;
    let mut rows = statement.query(params![page.as_str(), key])?;
    rows.next()?.map(record_from_row).transpose()
}

fn record_from_row(row: &Row<'_>) -> Result<StudentPageRecord> {
    Ok(StudentPageRecord {
        key: row.get("key")?,
        value: serde_json::from_str(&row.get::<_, String>("value_json")?)?,
        updated_at: parse_lww_timestamp(&row.get::<_, String>("updated_at")?)?,
        source: row.get("source")?,
        deleted: row.get::<_, i64>("deleted")? != 0,
    })
}

fn event_from_row(row: &Row<'_>) -> Result<StudentPageEvent> {
    Ok(StudentPageEvent {
        id: row.get("id")?,
        page: StudentPage::normalize(&row.get::<_, String>("page")?)?,
        event_type: row.get("event_type")?,
        key: row.get("key")?,
        metadata: serde_json::from_str(&row.get::<_, String>("metadata_json")?)?,
        created_at: parse_lww_timestamp(&row.get::<_, String>("created_at")?)?,
    })
}

fn validate_record_inputs(records: &[StudentPageRecordInput]) -> Result<()> {
    let mut keys = BTreeSet::new();
    for record in records {
        let key = normalize_key(&record.key)?;
        if !keys.insert(key.clone()) {
            return Err(EduMindError::Student(format!(
                "snapshot contains duplicate student page key `{key}`"
            )));
        }
    }
    Ok(())
}

fn normalize_key(value: &str) -> Result<String> {
    let key = value.trim();
    if key.is_empty() {
        return Err(EduMindError::Student(
            "student page record key must not be empty".to_owned(),
        ));
    }
    Ok(key.to_owned())
}

fn normalize_source(value: String) -> Result<String> {
    let source = value.trim();
    if source.is_empty() {
        return Err(EduMindError::Student(
            "student page source must not be empty".to_owned(),
        ));
    }
    Ok(source.to_owned())
}

fn snapshot_for_index(snapshot: &StudentPageSnapshot) -> Result<String> {
    let active_records = snapshot
        .records
        .iter()
        .filter(|record| !record.deleted)
        .map(|record| {
            json!({
                "key": record.key,
                "value": record.value,
                "updated_at": record.updated_at,
                "source": record.source,
            })
        })
        .collect::<Vec<_>>();
    let serialized = serde_json::to_string_pretty(&json!({
        "page": snapshot.page.as_str(),
        "records": active_records,
        "updated_at": snapshot.updated_at,
    }))?;
    Ok(format!(
        "Canonical {} state for study planning:\n{serialized}",
        snapshot.page.as_str()
    ))
}

fn format_lww_timestamp(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%dT%H:%M:%S%.9fZ").to_string()
}

fn parse_lww_timestamp(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| {
            EduMindError::InvalidStoredData(format!(
                "invalid student page timestamp `{value}`: {error}"
            ))
        })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{Duration, TimeZone, Utc};
    use serde_json::json;

    use super::{
        StudentPage, StudentPageRecordInput, StudentPageStore, index_student_page_snapshot,
    };
    use crate::memory::{HashEmbedder, HybridMemory, MemoryStore};

    fn timestamp() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 15, 13, 0, 0).unwrap()
    }

    #[test]
    fn normalizes_required_page_aliases() {
        assert_eq!(
            StudentPage::normalize("os").unwrap(),
            StudentPage::StudentOs
        );
        assert_eq!(
            StudentPage::normalize("student_planner").unwrap(),
            StudentPage::StudentPlanner
        );
        assert!(StudentPage::normalize("other").is_err());
    }

    #[test]
    fn last_write_wins_and_tombstones_converge_by_timestamp() {
        let store = StudentPageStore::new(MemoryStore::in_memory().unwrap());
        let older = timestamp();
        let newer = older + Duration::seconds(1);
        store
            .upsert_record(
                "os",
                StudentPageRecordInput::new("routine", json!({"focus": "physics"}), newer),
                "desktop",
            )
            .unwrap();
        let stale = store
            .upsert_record(
                "student-os",
                StudentPageRecordInput::new("routine", json!({"focus": "history"}), older),
                "offline",
            )
            .unwrap();
        let stale_delete = store
            .delete_record("student_os", "routine", older, "offline")
            .unwrap();
        let deleted = store
            .delete_record(
                "student-os",
                "routine",
                newer + Duration::seconds(1),
                "desktop",
            )
            .unwrap();

        assert!(!stale.applied);
        assert_eq!(stale.record.value, json!({"focus": "physics"}));
        assert!(!stale_delete.applied);
        assert!(deleted.applied);
        assert!(deleted.record.deleted);
        assert_eq!(store.load("os").unwrap().count, 0);
    }

    #[test]
    fn snapshot_saves_record_individual_and_summary_audit_events() {
        let store = StudentPageStore::new(MemoryStore::in_memory().unwrap());
        store
            .save_all(
                "planner",
                vec![StudentPageRecordInput::new(
                    "tuesday",
                    json!({"topic": "biology"}),
                    timestamp(),
                )],
                "desktop",
                timestamp(),
            )
            .unwrap();

        let events = store.events("student-planner", 10).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "record_upserted");
        assert_eq!(events[1].event_type, "snapshot_saved");
    }

    #[test]
    fn projects_planner_records_into_a_canonical_week() {
        let store = StudentPageStore::new(MemoryStore::in_memory().unwrap());
        store
            .save_all(
                "planner",
                vec![
                    StudentPageRecordInput::new(
                        "later",
                        json!({
                            "kind": "schedule-block",
                            "day": "Mon",
                            "title": "Later block",
                            "start": "11:00",
                            "end": "12:00"
                        }),
                        timestamp(),
                    ),
                    StudentPageRecordInput::new(
                        "earlier",
                        json!({
                            "day": "Monday",
                            "title": "Earlier block",
                            "start": "09:00",
                            "end": "10:00"
                        }),
                        timestamp(),
                    ),
                    StudentPageRecordInput::new(
                        "invalid",
                        json!({
                            "kind": "schedule-block",
                            "day": "Tuesday",
                            "title": "Broken block",
                            "start": "15:00",
                            "end": "14:00"
                        }),
                        timestamp(),
                    ),
                ],
                "desktop",
                timestamp(),
            )
            .unwrap();

        let schedule = store.planner_schedule().unwrap();
        assert_eq!(schedule.days.len(), 7);
        assert_eq!(schedule.days[0].day, "Monday");
        assert_eq!(
            schedule.days[0]
                .entries
                .iter()
                .map(|entry| entry.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Earlier block", "Later block"]
        );
        assert!(schedule.days[1].entries.is_empty());
        assert_eq!(schedule.ignored_records, 1);
    }

    #[tokio::test]
    async fn indexes_current_page_state_into_hybrid_memory() {
        let memory_store = MemoryStore::in_memory().unwrap();
        let pages = StudentPageStore::new(memory_store.clone());
        pages
            .upsert_record(
                "planner",
                StudentPageRecordInput::new(
                    "monday",
                    json!({"class": "calculus revision"}),
                    timestamp(),
                ),
                "desktop",
            )
            .unwrap();
        let memory =
            HybridMemory::new(memory_store, Arc::new(HashEmbedder::new(32).unwrap())).unwrap();

        let indexed = index_student_page_snapshot(&pages, &memory, "student-planner", timestamp())
            .await
            .unwrap();
        let hits = memory.search("calculus", 1).await.unwrap();

        assert_eq!(hits[0].record.id, indexed.id);
    }
}
