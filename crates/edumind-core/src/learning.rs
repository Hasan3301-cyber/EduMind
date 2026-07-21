use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A deterministic learning observation supplied by SRS, planning, or module memory.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LearningSignal {
    pub concept_id: String,
    #[serde(default)]
    pub correct: u32,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub days_since_review: u32,
}

/// The projected risk that a concept will be forgotten without further practice.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionRisk {
    #[default]
    Low,
    Moderate,
    High,
    Critical,
}

/// An offline, reproducible mastery estimate for one concept.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MasterySnapshot {
    pub concept_id: String,
    pub mastery_percent: u8,
    pub retention_risk: RetentionRisk,
    pub attempts: u32,
    pub correct: u32,
    pub days_since_review: u32,
}

/// A ranked next-best study action derived without model calls.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StudyRecommendation {
    pub concept_id: String,
    pub retention_risk: RetentionRisk,
    pub priority_score: u16,
    pub recommended_minutes: u16,
    pub rationale: String,
}

/// Scores all concepts represented by the supplied signals in stable concept order.
#[must_use]
pub fn score_mastery(signals: &[LearningSignal]) -> Vec<MasterySnapshot> {
    let mut aggregates = BTreeMap::<String, LearningAggregate>::new();
    for signal in signals {
        let concept_id = signal.concept_id.trim();
        if concept_id.is_empty() {
            continue;
        }
        let aggregate = aggregates.entry(concept_id.to_owned()).or_default();
        aggregate.attempts = aggregate
            .attempts
            .saturating_add(u64::from(signal.attempts));
        aggregate.correct = aggregate
            .correct
            .saturating_add(u64::from(signal.correct.min(signal.attempts)));
        aggregate.days_since_review = aggregate.days_since_review.max(signal.days_since_review);
    }

    aggregates
        .into_iter()
        .map(|(concept_id, aggregate)| {
            let accuracy = if aggregate.attempts == 0 {
                0
            } else {
                ((aggregate.correct.saturating_mul(100) / aggregate.attempts).min(100)) as u8
            };
            let recency_penalty = aggregate.days_since_review.saturating_mul(3).min(45) as u8;
            let mastery_percent = accuracy.saturating_sub(recency_penalty);
            let retention_risk = retention_risk(
                mastery_percent,
                aggregate.attempts,
                aggregate.days_since_review,
            );
            MasterySnapshot {
                concept_id,
                mastery_percent,
                retention_risk,
                attempts: aggregate.attempts.min(u64::from(u32::MAX)) as u32,
                correct: aggregate.correct.min(u64::from(u32::MAX)) as u32,
                days_since_review: aggregate.days_since_review,
            }
        })
        .collect()
}

/// Ranks snapshots by retention risk, low mastery, then a stable concept identifier.
#[must_use]
pub fn rank_recommendations(snapshots: &[MasterySnapshot]) -> Vec<StudyRecommendation> {
    let mut recommendations = snapshots
        .iter()
        .map(|snapshot| {
            let risk_weight = match snapshot.retention_risk {
                RetentionRisk::Low => 0,
                RetentionRisk::Moderate => 100,
                RetentionRisk::High => 200,
                RetentionRisk::Critical => 300,
            };
            let priority_score = risk_weight + u16::from(100 - snapshot.mastery_percent);
            let recommended_minutes = match snapshot.retention_risk {
                RetentionRisk::Low => 10,
                RetentionRisk::Moderate => 20,
                RetentionRisk::High => 30,
                RetentionRisk::Critical => 40,
            };
            StudyRecommendation {
                concept_id: snapshot.concept_id.clone(),
                retention_risk: snapshot.retention_risk,
                priority_score,
                recommended_minutes,
                rationale: format!(
                    "{}% mastery with {:?} retention risk after {} day(s) since review.",
                    snapshot.mastery_percent, snapshot.retention_risk, snapshot.days_since_review
                ),
            }
        })
        .collect::<Vec<_>>();
    recommendations.sort_by(|left, right| {
        right
            .priority_score
            .cmp(&left.priority_score)
            .then_with(|| left.concept_id.cmp(&right.concept_id))
    });
    recommendations
}

#[derive(Default)]
struct LearningAggregate {
    attempts: u64,
    correct: u64,
    days_since_review: u32,
}

fn retention_risk(mastery_percent: u8, attempts: u64, days_since_review: u32) -> RetentionRisk {
    if attempts == 0 || days_since_review >= 21 || mastery_percent < 35 {
        RetentionRisk::Critical
    } else if days_since_review >= 14 || mastery_percent < 55 {
        RetentionRisk::High
    } else if days_since_review >= 7 || mastery_percent < 75 {
        RetentionRisk::Moderate
    } else {
        RetentionRisk::Low
    }
}

#[cfg(test)]
mod tests {
    use super::{LearningSignal, RetentionRisk, rank_recommendations, score_mastery};

    #[test]
    fn mastery_and_recommendation_order_are_deterministic() {
        let signals = vec![
            LearningSignal {
                concept_id: "algebra".to_owned(),
                correct: 9,
                attempts: 10,
                days_since_review: 2,
            },
            LearningSignal {
                concept_id: "calculus".to_owned(),
                correct: 2,
                attempts: 10,
                days_since_review: 16,
            },
            LearningSignal {
                concept_id: "algebra".to_owned(),
                correct: 1,
                attempts: 2,
                days_since_review: 5,
            },
        ];

        let snapshots = score_mastery(&signals);
        let recommendations = rank_recommendations(&snapshots);

        assert_eq!(snapshots[0].concept_id, "algebra");
        assert_eq!(snapshots[1].concept_id, "calculus");
        assert_eq!(snapshots[1].retention_risk, RetentionRisk::Critical);
        assert_eq!(recommendations[0].concept_id, "calculus");
        assert_eq!(recommendations[0].recommended_minutes, 40);
    }

    #[test]
    fn blank_concepts_do_not_create_recommendations() {
        let snapshots = score_mastery(&[LearningSignal {
            concept_id: "  ".to_owned(),
            correct: 1,
            attempts: 1,
            days_since_review: 0,
        }]);

        assert!(snapshots.is_empty());
        assert!(rank_recommendations(&snapshots).is_empty());
    }
}
