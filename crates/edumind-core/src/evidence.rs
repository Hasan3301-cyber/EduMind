use std::{collections::BTreeSet, error::Error, fmt};

use serde::{Deserialize, Serialize};

/// A source-level citation attached to a grounded answer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    pub source_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub locator: Option<String>,
}

impl Citation {
    /// Creates a citation with a stable source identifier.
    #[must_use]
    pub fn new(source_id: impl Into<String>) -> Self {
        Self {
            source_id: source_id.into(),
            title: None,
            locator: None,
        }
    }
}

/// A quoted or indexed passage that supports an answer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EvidenceSpan {
    pub source_id: String,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

impl EvidenceSpan {
    /// Creates a passage linked to a citation source.
    #[must_use]
    pub fn new(
        source_id: impl Into<String>,
        start: usize,
        end: usize,
        text: impl Into<String>,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            start,
            end,
            text: text.into(),
        }
    }
}

/// Whether an answer is supported by the supplied evidence.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    #[default]
    InsufficientEvidence,
    Grounded,
}

/// A student-facing answer with explicit evidence and calibrated confidence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroundedAnswer {
    pub answer: String,
    pub status: EvidenceStatus,
    #[serde(default)]
    pub citations: Vec<Citation>,
    #[serde(default)]
    pub evidence: Vec<EvidenceSpan>,
    #[serde(default)]
    pub confidence: f32,
}

impl GroundedAnswer {
    /// Builds and validates an answer that is grounded in supplied material.
    pub fn grounded(
        answer: impl Into<String>,
        citations: Vec<Citation>,
        evidence: Vec<EvidenceSpan>,
        confidence: f32,
    ) -> Result<Self, GroundedAnswerError> {
        let answer = Self {
            answer: answer.into(),
            status: EvidenceStatus::Grounded,
            citations,
            evidence,
            confidence,
        };
        answer.validate()?;
        Ok(answer)
    }

    /// Returns an explicit response when the available material cannot support a claim.
    #[must_use]
    pub fn insufficient_evidence(answer: impl Into<String>) -> Self {
        Self {
            answer: answer.into(),
            status: EvidenceStatus::InsufficientEvidence,
            citations: Vec::new(),
            evidence: Vec::new(),
            confidence: 0.0,
        }
    }

    /// Validates the source-grounded answer contract.
    pub fn validate(&self) -> Result<(), GroundedAnswerError> {
        if self.answer.trim().is_empty() {
            return Err(GroundedAnswerError::EmptyAnswer);
        }
        if !self.confidence.is_finite() || !(0.0..=1.0).contains(&self.confidence) {
            return Err(GroundedAnswerError::InvalidConfidence);
        }
        if self.status == EvidenceStatus::Grounded {
            if self.citations.is_empty() {
                return Err(GroundedAnswerError::MissingCitations);
            }
            if self.evidence.is_empty() {
                return Err(GroundedAnswerError::MissingEvidence);
            }
        }

        let sources = self
            .citations
            .iter()
            .map(|citation| citation.source_id.trim())
            .collect::<BTreeSet<_>>();
        if sources.iter().any(|source| source.is_empty()) {
            return Err(GroundedAnswerError::EmptyCitationSource);
        }
        for span in &self.evidence {
            if span.source_id.trim().is_empty() || !sources.contains(span.source_id.trim()) {
                return Err(GroundedAnswerError::UncitedEvidence {
                    source_id: span.source_id.clone(),
                });
            }
            if span.end <= span.start || span.text.trim().is_empty() {
                return Err(GroundedAnswerError::InvalidEvidenceSpan {
                    source_id: span.source_id.clone(),
                });
            }
        }
        Ok(())
    }
}

/// A validation failure for a source-grounded answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GroundedAnswerError {
    EmptyAnswer,
    EmptyCitationSource,
    InvalidConfidence,
    InvalidEvidenceSpan { source_id: String },
    MissingCitations,
    MissingEvidence,
    UncitedEvidence { source_id: String },
}

impl fmt::Display for GroundedAnswerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAnswer => formatter.write_str("answer must not be empty"),
            Self::EmptyCitationSource => {
                formatter.write_str("citation source_id must not be empty")
            }
            Self::InvalidConfidence => formatter.write_str("confidence must be within 0.0 and 1.0"),
            Self::InvalidEvidenceSpan { source_id } => {
                write!(formatter, "evidence span for source {source_id} is invalid")
            }
            Self::MissingCitations => formatter.write_str("grounded answers require citations"),
            Self::MissingEvidence => formatter.write_str("grounded answers require evidence spans"),
            Self::UncitedEvidence { source_id } => {
                write!(
                    formatter,
                    "evidence source {source_id} has no matching citation"
                )
            }
        }
    }
}

impl Error for GroundedAnswerError {}

#[cfg(test)]
mod tests {
    use super::{Citation, EvidenceSpan, GroundedAnswer, GroundedAnswerError};

    #[test]
    fn grounded_answers_require_citations_and_matching_evidence() {
        let missing_citations =
            GroundedAnswer::grounded("Practice improves recall.", Vec::new(), Vec::new(), 0.8)
                .unwrap_err();
        assert_eq!(missing_citations, GroundedAnswerError::MissingCitations);

        let missing_evidence = GroundedAnswer::grounded(
            "Practice improves recall.",
            vec![Citation::new("note-1")],
            Vec::new(),
            0.8,
        )
        .unwrap_err();
        assert_eq!(missing_evidence, GroundedAnswerError::MissingEvidence);

        let answer = GroundedAnswer::grounded(
            "Practice improves recall.",
            vec![Citation::new("note-1")],
            vec![EvidenceSpan::new(
                "note-1",
                0,
                25,
                "Practice improves recall.",
            )],
            0.8,
        )
        .unwrap();
        assert!(answer.validate().is_ok());
    }

    #[test]
    fn insufficient_evidence_is_explicit_and_serializable() {
        let answer = GroundedAnswer::insufficient_evidence(
            "There is insufficient evidence in the supplied material.",
        );
        let encoded = serde_json::to_string(&answer).unwrap();
        let decoded: GroundedAnswer = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, answer);
        assert!(decoded.validate().is_ok());
    }
}
