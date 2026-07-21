use std::{collections::BTreeSet, fmt};

use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, Row, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    infra::{EduMindError, Result},
    memory::MemoryStore,
};

const DEFAULT_DECK: &str = "default";
const DEFAULT_EASE_FACTOR: f64 = 2.5;
const MIN_EASE_FACTOR: f64 = 1.3;
const MAX_INTERVAL_DAYS: u32 = 36_500;

/// Stable identifier for one spaced-repetition card.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SrsCardId(pub Uuid);

impl SrsCardId {
    /// Creates a new random card identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    fn parse(value: &str) -> Result<Self> {
        Uuid::parse_str(value).map(Self).map_err(|error| {
            EduMindError::InvalidStoredData(format!("invalid SRS card id `{value}`: {error}"))
        })
    }
}

impl Default for SrsCardId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SrsCardId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// A durable flashcard with SM-2-style scheduling fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SrsCard {
    pub id: SrsCardId,
    pub front: String,
    pub back: String,
    pub deck: String,
    pub due_at: DateTime<Utc>,
    pub interval_days: u32,
    pub ease_factor: f64,
    pub repetitions: u32,
    pub lapses: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_reviewed_at: Option<DateTime<Utc>>,
}

/// Non-mutating consequence of applying one review grade at a specific time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SrsReviewPreview {
    pub card_id: SrsCardId,
    pub rating: u8,
    pub due_at: DateTime<Utc>,
    pub interval_days: u32,
    pub ease_factor: f64,
    pub repetitions: u32,
    pub lapses: u32,
}

/// Input for a new card. Empty decks normalize to `default`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewSrsCard {
    pub front: String,
    pub back: String,
    #[serde(default = "default_deck")]
    pub deck: String,
}

impl NewSrsCard {
    /// Creates a card input in the default deck.
    #[must_use]
    pub fn new(front: impl Into<String>, back: impl Into<String>) -> Self {
        Self {
            front: front.into(),
            back: back.into(),
            deck: default_deck(),
        }
    }
}

/// Summary counts for an SRS deck or the complete collection.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SrsStats {
    pub deck: Option<String>,
    pub total_cards: usize,
    pub due_cards: usize,
    pub new_cards: usize,
    pub learning_cards: usize,
    pub mature_cards: usize,
    pub next_due_at: Option<DateTime<Utc>>,
}

/// Local-first spaced-repetition service backed by the shared memory database.
#[derive(Clone)]
pub struct SrsService {
    store: MemoryStore,
}

impl SrsService {
    /// Creates an SRS service on the provided memory database.
    #[must_use]
    pub fn new(store: MemoryStore) -> Self {
        Self { store }
    }

    /// Returns the underlying persistent store handle.
    #[must_use]
    pub fn store_handle(&self) -> &MemoryStore {
        &self.store
    }

    /// Creates a card, returning an existing identical deck/front/back card on duplicate input.
    pub fn create_card(&self, input: NewSrsCard, now: DateTime<Utc>) -> Result<SrsCard> {
        self.insert_card(input, now).map(|(card, _)| card)
    }

    /// Deterministically extracts definition-style flashcards from notes and persists new cards.
    pub fn generate_from_notes(
        &self,
        notes: &str,
        deck: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<Vec<SrsCard>> {
        let deck = normalize_deck(deck.into());
        if notes.trim().is_empty() {
            return Err(EduMindError::Study(
                "cannot generate SRS cards from empty notes".to_owned(),
            ));
        }
        let mut created = Vec::new();
        for mut candidate in extract_cards_from_notes(notes) {
            candidate.deck = deck.clone();
            let (card, inserted) = self.insert_card(candidate, now)?;
            if inserted {
                created.push(card);
            }
        }
        Ok(created)
    }

    /// Lists cards due at or before `now`, ordered deterministically by due time and ID.
    pub fn due(
        &self,
        deck: Option<&str>,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<SrsCard>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let now = format_schedule_timestamp(now);
        let connection = self.store.connection()?;
        let mut cards = Vec::new();
        match deck {
            Some(deck) => {
                let deck = normalize_deck(deck.to_owned());
                let mut statement = connection.prepare(
                    "SELECT id, front, back, deck, due_at, interval_days, ease_factor,
                            repetitions, lapses, created_at, updated_at, last_reviewed_at
                     FROM srs_cards
                     WHERE deck = ?1 AND due_at <= ?2
                     ORDER BY due_at ASC, id ASC
                     LIMIT ?3",
                )?;
                let mut rows = statement.query(params![deck, now, limit])?;
                while let Some(row) = rows.next()? {
                    cards.push(card_from_row(row)?);
                }
            }
            None => {
                let mut statement = connection.prepare(
                    "SELECT id, front, back, deck, due_at, interval_days, ease_factor,
                            repetitions, lapses, created_at, updated_at, last_reviewed_at
                     FROM srs_cards
                     WHERE due_at <= ?1
                     ORDER BY due_at ASC, id ASC
                     LIMIT ?2",
                )?;
                let mut rows = statement.query(params![now, limit])?;
                while let Some(row) = rows.next()? {
                    cards.push(card_from_row(row)?);
                }
            }
        }
        Ok(cards)
    }

    /// Previews a 0–5 review without changing the card or its history.
    pub fn preview_review(
        &self,
        card_id: SrsCardId,
        rating: u8,
        now: DateTime<Utc>,
    ) -> Result<Option<SrsReviewPreview>> {
        validate_review_rating(rating)?;
        self.get(card_id)?
            .map(|card| review_preview_for_card(&card, rating, now))
            .transpose()
    }

    /// Records a 0–5 review and updates the next due date with an SM-2-style interval.
    pub fn review(
        &self,
        card_id: SrsCardId,
        rating: u8,
        now: DateTime<Utc>,
    ) -> Result<Option<SrsCard>> {
        validate_review_rating(rating)?;
        let Some(card) = self.get(card_id)? else {
            return Ok(None);
        };
        let preview = review_preview_for_card(&card, rating, now)?;
        let timestamp = format_schedule_timestamp(now);
        let due_timestamp = format_schedule_timestamp(preview.due_at);
        let mut connection = self.store.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE srs_cards
             SET due_at = ?1, interval_days = ?2, ease_factor = ?3, repetitions = ?4,
                 lapses = ?5, updated_at = ?6, last_reviewed_at = ?7
             WHERE id = ?8",
            params![
                due_timestamp,
                i64::from(preview.interval_days),
                preview.ease_factor,
                i64::from(preview.repetitions),
                i64::from(preview.lapses),
                timestamp,
                timestamp,
                card_id.to_string(),
            ],
        )?;
        transaction.execute(
            "INSERT INTO srs_reviews (
                id, card_id, rating, previous_interval_days, next_interval_days, reviewed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                Uuid::new_v4().to_string(),
                card_id.to_string(),
                i64::from(rating),
                i64::from(card.interval_days),
                i64::from(preview.interval_days),
                timestamp,
            ],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get(card_id)
    }

    /// Gets one card by ID.
    pub fn get(&self, card_id: SrsCardId) -> Result<Option<SrsCard>> {
        let connection = self.store.connection()?;
        read_card_by_id(&connection, card_id)
    }

    /// Returns deck-level scheduling statistics as of `now`.
    pub fn stats(&self, deck: Option<&str>, now: DateTime<Utc>) -> Result<SrsStats> {
        let deck = deck.map(|deck| normalize_deck(deck.to_owned()));
        let cards = self.list_cards(deck.as_deref())?;
        let mut stats = SrsStats {
            deck,
            total_cards: cards.len(),
            ..SrsStats::default()
        };
        for card in cards {
            if card.due_at <= now {
                stats.due_cards += 1;
            }
            if card.repetitions == 0 {
                stats.new_cards += 1;
            } else if card.interval_days >= 21 {
                stats.mature_cards += 1;
            } else {
                stats.learning_cards += 1;
            }
            if stats.next_due_at.is_none_or(|due_at| card.due_at < due_at) {
                stats.next_due_at = Some(card.due_at);
            }
        }
        Ok(stats)
    }

    /// Lists durable cards in stable due-date order for offline learning analytics.
    pub fn cards(&self, deck: Option<&str>) -> Result<Vec<SrsCard>> {
        let deck = deck.map(|value| normalize_deck(value.to_owned()));
        self.list_cards(deck.as_deref())
    }

    fn insert_card(&self, input: NewSrsCard, now: DateTime<Utc>) -> Result<(SrsCard, bool)> {
        let input = normalize_card_input(input)?;
        let card = SrsCard {
            id: SrsCardId::new(),
            front: input.front,
            back: input.back,
            deck: input.deck,
            due_at: now,
            interval_days: 0,
            ease_factor: DEFAULT_EASE_FACTOR,
            repetitions: 0,
            lapses: 0,
            created_at: now,
            updated_at: now,
            last_reviewed_at: None,
        };
        let timestamp = format_schedule_timestamp(now);
        let connection = self.store.connection()?;
        let inserted = connection.execute(
            "INSERT INTO srs_cards (
                id, front, back, deck, due_at, interval_days, ease_factor, repetitions, lapses,
                created_at, updated_at, last_reviewed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL)
             ON CONFLICT(deck, front, back) DO NOTHING",
            params![
                card.id.to_string(),
                card.front,
                card.back,
                card.deck,
                timestamp,
                0_i64,
                card.ease_factor,
                0_i64,
                0_i64,
                timestamp,
                timestamp,
            ],
        )?;
        if inserted > 0 {
            return Ok((card, true));
        }
        read_card_by_fingerprint(&connection, &card.deck, &card.front, &card.back)?.map_or_else(
            || {
                Err(EduMindError::InvalidStoredData(
                    "duplicate SRS card was not readable after insert conflict".to_owned(),
                ))
            },
            |existing| Ok((existing, false)),
        )
    }

    fn list_cards(&self, deck: Option<&str>) -> Result<Vec<SrsCard>> {
        let connection = self.store.connection()?;
        let mut cards = Vec::new();
        match deck {
            Some(deck) => {
                let mut statement = connection.prepare(
                    "SELECT id, front, back, deck, due_at, interval_days, ease_factor,
                            repetitions, lapses, created_at, updated_at, last_reviewed_at
                     FROM srs_cards WHERE deck = ?1 ORDER BY due_at ASC, id ASC",
                )?;
                let mut rows = statement.query(params![deck])?;
                while let Some(row) = rows.next()? {
                    cards.push(card_from_row(row)?);
                }
            }
            None => {
                let mut statement = connection.prepare(
                    "SELECT id, front, back, deck, due_at, interval_days, ease_factor,
                            repetitions, lapses, created_at, updated_at, last_reviewed_at
                     FROM srs_cards ORDER BY due_at ASC, id ASC",
                )?;
                let mut rows = statement.query([])?;
                while let Some(row) = rows.next()? {
                    cards.push(card_from_row(row)?);
                }
            }
        }
        Ok(cards)
    }
}

/// Extracts definition-style cards from stable note patterns without calling a model.
#[must_use]
pub fn extract_cards_from_notes(notes: &str) -> Vec<NewSrsCard> {
    let mut cards = Vec::new();
    let mut fingerprints = BTreeSet::new();
    for fragment in notes.lines().flat_map(|line| line.split_terminator('.')) {
        let fragment = compact_whitespace(fragment);
        let Some((subject, explanation)) = split_definition(&fragment) else {
            continue;
        };
        if subject.len() < 2 || subject.len() > 120 || explanation.len() < 3 {
            continue;
        }
        let front = format!("What is {subject}?");
        let fingerprint = format!(
            "{}\u{1f}{}",
            front.to_ascii_lowercase(),
            explanation.to_ascii_lowercase()
        );
        if fingerprints.insert(fingerprint) {
            cards.push(NewSrsCard::new(front, explanation));
        }
        if cards.len() == 20 {
            break;
        }
    }
    cards
}

fn split_definition(fragment: &str) -> Option<(String, String)> {
    for delimiter in [":", " — ", " - "] {
        if let Some((subject, explanation)) = fragment.split_once(delimiter) {
            return definition_parts(subject, explanation);
        }
    }
    let lowercase = fragment.to_ascii_lowercase();
    for marker in [" is ", " are ", " means "] {
        if let Some(index) = lowercase.find(marker) {
            let subject = &fragment[..index];
            let explanation = &fragment[index + marker.len()..];
            return definition_parts(subject, explanation);
        }
    }
    None
}

fn definition_parts(subject: &str, explanation: &str) -> Option<(String, String)> {
    let subject = compact_whitespace(subject)
        .trim_matches(['-', ':', '—'])
        .to_owned();
    let explanation = compact_whitespace(explanation)
        .trim_matches(['-', ':', '—'])
        .to_owned();
    (!subject.is_empty() && !explanation.is_empty()).then_some((subject, explanation))
}

fn validate_review_rating(rating: u8) -> Result<()> {
    if rating > 5 {
        return Err(EduMindError::Study(
            "SRS review rating must be between 0 and 5".to_owned(),
        ));
    }
    Ok(())
}

fn review_preview_for_card(
    card: &SrsCard,
    rating: u8,
    now: DateTime<Utc>,
) -> Result<SrsReviewPreview> {
    let schedule = next_schedule(card, rating)?;
    let due_at = now
        .checked_add_signed(Duration::days(i64::from(schedule.interval_days)))
        .ok_or_else(|| EduMindError::Study("SRS due timestamp overflowed".to_owned()))?;
    Ok(SrsReviewPreview {
        card_id: card.id,
        rating,
        due_at,
        interval_days: schedule.interval_days,
        ease_factor: schedule.ease_factor,
        repetitions: schedule.repetitions,
        lapses: schedule.lapses,
    })
}

fn next_schedule(card: &SrsCard, rating: u8) -> Result<SrsSchedule> {
    let quality = f64::from(rating);
    let ease_adjustment = 0.1 - (5.0 - quality) * (0.08 + (5.0 - quality) * 0.02);
    let ease_factor = (card.ease_factor + ease_adjustment).max(MIN_EASE_FACTOR);
    if rating < 3 {
        return Ok(SrsSchedule {
            interval_days: 1,
            ease_factor,
            repetitions: 0,
            lapses: card.lapses.saturating_add(1),
        });
    }
    let repetitions = card.repetitions.saturating_add(1);
    let interval_days = match repetitions {
        1 => 1,
        2 => 6,
        _ => ((f64::from(card.interval_days.max(1)) * ease_factor).round() as u32)
            .clamp(1, MAX_INTERVAL_DAYS),
    };
    Ok(SrsSchedule {
        interval_days,
        ease_factor,
        repetitions,
        lapses: card.lapses,
    })
}

#[derive(Clone, Copy, Debug)]
struct SrsSchedule {
    interval_days: u32,
    ease_factor: f64,
    repetitions: u32,
    lapses: u32,
}

fn read_card_by_id(connection: &Connection, card_id: SrsCardId) -> Result<Option<SrsCard>> {
    let mut statement = connection.prepare(
        "SELECT id, front, back, deck, due_at, interval_days, ease_factor,
                repetitions, lapses, created_at, updated_at, last_reviewed_at
         FROM srs_cards WHERE id = ?1",
    )?;
    let mut rows = statement.query(params![card_id.to_string()])?;
    rows.next()?.map(card_from_row).transpose()
}

fn read_card_by_fingerprint(
    connection: &Connection,
    deck: &str,
    front: &str,
    back: &str,
) -> Result<Option<SrsCard>> {
    let mut statement = connection.prepare(
        "SELECT id, front, back, deck, due_at, interval_days, ease_factor,
                repetitions, lapses, created_at, updated_at, last_reviewed_at
         FROM srs_cards WHERE deck = ?1 AND front = ?2 AND back = ?3",
    )?;
    let mut rows = statement.query(params![deck, front, back])?;
    rows.next()?.map(card_from_row).transpose()
}

fn card_from_row(row: &Row<'_>) -> Result<SrsCard> {
    let interval_days = row.get::<_, i64>("interval_days")?;
    let repetitions = row.get::<_, i64>("repetitions")?;
    let lapses = row.get::<_, i64>("lapses")?;
    Ok(SrsCard {
        id: SrsCardId::parse(&row.get::<_, String>("id")?)?,
        front: row.get("front")?,
        back: row.get("back")?,
        deck: row.get("deck")?,
        due_at: parse_schedule_timestamp(&row.get::<_, String>("due_at")?)?,
        interval_days: u32::try_from(interval_days)
            .map_err(|_| invalid_card_field("interval_days"))?,
        ease_factor: row.get("ease_factor")?,
        repetitions: u32::try_from(repetitions).map_err(|_| invalid_card_field("repetitions"))?,
        lapses: u32::try_from(lapses).map_err(|_| invalid_card_field("lapses"))?,
        created_at: parse_schedule_timestamp(&row.get::<_, String>("created_at")?)?,
        updated_at: parse_schedule_timestamp(&row.get::<_, String>("updated_at")?)?,
        last_reviewed_at: row
            .get::<_, Option<String>>("last_reviewed_at")?
            .map(|value| parse_schedule_timestamp(&value))
            .transpose()?,
    })
}

fn invalid_card_field(field: &str) -> EduMindError {
    EduMindError::InvalidStoredData(format!("invalid SRS card {field}"))
}

fn normalize_card_input(input: NewSrsCard) -> Result<NewSrsCard> {
    let front = compact_whitespace(&input.front);
    let back = compact_whitespace(&input.back);
    if front.is_empty() || back.is_empty() {
        return Err(EduMindError::Study(
            "SRS cards require non-empty front and back text".to_owned(),
        ));
    }
    Ok(NewSrsCard {
        front,
        back,
        deck: normalize_deck(input.deck),
    })
}

fn normalize_deck(deck: String) -> String {
    let deck = compact_whitespace(&deck);
    if deck.is_empty() {
        default_deck()
    } else {
        deck
    }
}

fn default_deck() -> String {
    DEFAULT_DECK.to_owned()
}

fn compact_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_schedule_timestamp(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%dT%H:%M:%S%.9fZ").to_string()
}

fn parse_schedule_timestamp(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| {
            EduMindError::InvalidStoredData(format!("invalid SRS timestamp `{value}`: {error}"))
        })
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};

    use super::{NewSrsCard, SrsService, extract_cards_from_notes};
    use crate::memory::MemoryStore;

    fn timestamp() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 15, 9, 0, 0).unwrap()
    }

    #[test]
    fn generates_deterministic_definition_cards_without_duplicates() {
        let cards = extract_cards_from_notes(
            "Derivative: the instantaneous rate of change.\nDerivative: the instantaneous rate of change.\nAn integral is accumulated change.",
        );

        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].front, "What is Derivative?");
        assert_eq!(cards[1].back, "accumulated change");
    }

    #[test]
    fn schedules_reviews_and_reports_due_stats() {
        let service = SrsService::new(MemoryStore::in_memory().unwrap());
        let mut input = NewSrsCard::new("A derivative", "An instantaneous rate of change");
        input.deck = "calculus".to_owned();
        let card = service.create_card(input, timestamp()).unwrap();

        assert_eq!(
            service
                .due(Some("calculus"), timestamp(), 10)
                .unwrap()
                .len(),
            1
        );
        let reviewed = service.review(card.id, 5, timestamp()).unwrap().unwrap();
        assert_eq!(reviewed.interval_days, 1);
        assert!(
            service
                .due(Some("calculus"), timestamp(), 10)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            service
                .due(Some("calculus"), timestamp() + Duration::days(1), 10)
                .unwrap()
                .len(),
            1
        );
        let stats = service.stats(Some("calculus"), timestamp()).unwrap();
        assert_eq!(stats.total_cards, 1);
        assert_eq!(stats.learning_cards, 1);
    }

    #[test]
    fn generation_persists_only_new_cards() {
        let service = SrsService::new(MemoryStore::in_memory().unwrap());
        let notes = "Vector: a quantity with magnitude and direction.";

        assert_eq!(
            service
                .generate_from_notes(notes, "physics", timestamp())
                .unwrap()
                .len(),
            1
        );
        assert!(
            service
                .generate_from_notes(notes, "physics", timestamp())
                .unwrap()
                .is_empty()
        );
    }
}
