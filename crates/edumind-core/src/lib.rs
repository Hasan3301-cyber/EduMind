//! Shared, dependency-light domain contracts for EduMind.

pub mod domain;
pub mod evidence;
pub mod learning;
pub mod research;
pub mod runs;
pub mod wellness;

pub use domain::{ModuleId, StudyModule, Task, TaskId, TaskStatus};
pub use evidence::{Citation, EvidenceSpan, EvidenceStatus, GroundedAnswer, GroundedAnswerError};
pub use learning::{
    LearningSignal, MasterySnapshot, RetentionRisk, StudyRecommendation, rank_recommendations,
    score_mastery,
};
pub use research::{
    ClaimAssessment, ClaimSupport, ClaimValidationRequest, Community, EvidenceSource, GraphData,
    GraphEdge, GraphNode, Hypothesis, InsightType, LiteratureGraphRequest, LiteratureSource,
    PaperMetadata, PipelineContext, PipelineEvent, PipelineProgress, PluginInfo, PluginMetadata,
    PluginOutput, PluginPriority, PluginStage, PluginStatus, ProjectNote, RankedPaper,
    ResearchInsight, ResearchPlugin, ResearchProject, ResearchProjectId, ResearchRequest,
    StructuredDocument, ValidationIssue, ValidationReport, merge_paper_metadata,
    paper_identity_key, project_topic_key,
};
pub use runs::{
    PipelineRun, PipelineRunId, PipelineRunStatus, PipelineStage, PipelineStageStatus, RunBudget,
    RunCheckpoint, RunTimelineEvent, RunVerification,
};
pub use wellness::{
    ActivityLevel, BiologicalSex, FitnessGoal, Meal, MealPlan, NutritionTargets,
    WELLNESS_DISCLAIMER, WellnessProfile, WellnessScheduleBlock, WorkoutPlan, WorkoutSession,
    estimate_nutrition_targets, generate_meal_plan, generate_workout_plan, workout_schedule_blocks,
};
