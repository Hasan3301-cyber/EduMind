//! Local-first study services, including spaced repetition.

pub mod learning_engine;
pub mod srs;

pub use learning_engine::{LearningEngine, LearningInsights, PlannerConflict};
pub use srs::{
    NewSrsCard, SrsCard, SrsCardId, SrsReviewPreview, SrsService, SrsStats,
    extract_cards_from_notes,
};
