use std::collections::BTreeMap;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Row, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    infra::{EduMindError, Result},
    memory::MemoryStore,
};

/// Redacted operational event kept only in the local EduMind database.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelemetryEvent {
    pub id: Uuid,
    pub event_name: String,
    pub module_id: String,
    pub outcome: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    pub created_at: DateTime<Utc>,
}

/// Input accepted by local telemetry after metadata redaction.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TelemetryInput {
    pub event_name: String,
    pub module_id: String,
    pub outcome: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

/// Local-only observability storage that rejects raw content and redacts unknown values.
#[derive(Clone)]
pub struct LocalTelemetry {
    store: MemoryStore,
}

impl LocalTelemetry {
    /// Creates a telemetry service over the existing local-first database.
    #[must_use]
    pub fn new(store: MemoryStore) -> Self {
        Self { store }
    }

    /// Records one redacted event without any network export path.
    pub fn record(&self, input: TelemetryInput, now: DateTime<Utc>) -> Result<TelemetryEvent> {
        validate_identifier("event name", &input.event_name)?;
        validate_identifier("module ID", &input.module_id)?;
        validate_identifier("outcome", &input.outcome)?;
        let event = TelemetryEvent {
            id: Uuid::new_v4(),
            event_name: input.event_name,
            module_id: input.module_id,
            outcome: input.outcome,
            metadata: redact_metadata(input.metadata),
            created_at: now,
        };
        let connection = self.store.connection()?;
        connection.execute(
            "INSERT INTO local_telemetry_events (
                id, event_name, module_id, outcome, metadata_json, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event.id.to_string(),
                event.event_name,
                event.module_id,
                event.outcome,
                serde_json::to_string(&event.metadata)?,
                format_timestamp(event.created_at),
            ],
        )?;
        Ok(event)
    }

    /// Lists recent redacted operational events in reverse chronological order.
    pub fn recent(&self, limit: usize) -> Result<Vec<TelemetryEvent>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let connection = self.store.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, event_name, module_id, outcome, metadata_json, created_at
             FROM local_telemetry_events
             ORDER BY created_at DESC, id DESC
             LIMIT ?1",
        )?;
        let mut rows = statement.query(params![i64::try_from(limit).unwrap_or(i64::MAX)])?;
        let mut events = Vec::new();
        while let Some(row) = rows.next()? {
            events.push(event_from_row(row)?);
        }
        Ok(events)
    }
}

fn validate_identifier(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 80
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(EduMindError::Security(format!(
            "telemetry {} must be a short identifier",
            label
        )));
    }
    Ok(())
}

fn redact_metadata(metadata: BTreeMap<String, String>) -> BTreeMap<String, String> {
    metadata
        .into_iter()
        .filter_map(|(key, value)| {
            valid_metadata_key(&key).then(|| {
                let redacted =
                    if key.ends_with("_count") || key.ends_with("_ms") || key.ends_with("_code") {
                        value
                            .parse::<u64>()
                            .map_or("[redacted]".to_owned(), |number| number.to_string())
                    } else if matches!(value.as_str(), "ok" | "failed" | "cancelled" | "offline") {
                        value
                    } else {
                        "[redacted]".to_owned()
                    };
                (key, redacted)
            })
        })
        .collect()
}

fn valid_metadata_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 48
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

fn event_from_row(row: &Row<'_>) -> Result<TelemetryEvent> {
    let id: String = row.get(0)?;
    let created_at: String = row.get(5)?;
    Ok(TelemetryEvent {
        id: Uuid::parse_str(&id).map_err(|error| {
            EduMindError::InvalidStoredData(format!("invalid telemetry ID: {}", error))
        })?,
        event_name: row.get(1)?,
        module_id: row.get(2)?,
        outcome: row.get(3)?,
        metadata: serde_json::from_str(&row.get::<_, String>(4)?)?,
        created_at: DateTime::parse_from_rfc3339(&created_at)
            .map_err(|error| {
                EduMindError::InvalidStoredData(format!("invalid telemetry timestamp: {}", error))
            })?
            .with_timezone(&Utc),
    })
}

fn format_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{TimeZone, Utc};

    use super::{LocalTelemetry, TelemetryInput};
    use crate::memory::MemoryStore;

    #[test]
    fn local_telemetry_redacts_free_form_metadata() {
        let telemetry = LocalTelemetry::new(MemoryStore::in_memory().unwrap());
        let mut metadata = BTreeMap::new();
        metadata.insert("duration_ms".to_owned(), "42".to_owned());
        metadata.insert("status_code".to_owned(), "200".to_owned());
        metadata.insert("raw_message".to_owned(), "student@example.edu".to_owned());
        metadata.insert("status".to_owned(), "ok".to_owned());
        let now = Utc.with_ymd_and_hms(2026, 7, 19, 10, 0, 0).unwrap();

        let event = telemetry
            .record(
                TelemetryInput {
                    event_name: "study_refresh".to_owned(),
                    module_id: "study-review".to_owned(),
                    outcome: "ok".to_owned(),
                    metadata,
                },
                now,
            )
            .unwrap();

        assert_eq!(event.metadata.get("duration_ms"), Some(&"42".to_owned()));
        assert_eq!(event.metadata.get("status_code"), Some(&"200".to_owned()));
        assert_eq!(
            event.metadata.get("raw_message"),
            Some(&"[redacted]".to_owned())
        );
        assert_eq!(telemetry.recent(1).unwrap().len(), 1);
    }
}
