use chrono::{DateTime, Utc};
use edumind::{
    agent::CancellationRegistry,
    memory::{Embedding, MemoryId, MemoryStore, VectorIndex},
    runs::RunStore,
    security::{ContentGuard, ContentRisk},
    student::{StudentPageRecordInput, StudentPageStore},
    study::{LearningEngine, SrsService},
};
use edumind_core::{
    EvidenceStatus, GroundedAnswer, LearningSignal, PipelineRunId, RunBudget, RunTimelineEvent,
    rank_recommendations, score_mastery,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Thresholds {
    minimum_citation_coverage: f32,
    minimum_evidence_precision: f32,
    minimum_policy_denial_rate: f32,
    minimum_recommendation_priority: u16,
    minimum_vector_exact_score: f32,
}

#[derive(Deserialize)]
struct RecommendationFixture {
    signals: Vec<LearningSignal>,
    expected_top: String,
    minimum_priority: u16,
}

#[derive(Deserialize)]
struct InjectionFixture {
    contents: Vec<String>,
    expected_risk: String,
}

#[derive(Deserialize)]
struct EvidenceFixture {
    answer: String,
    expected_status: String,
}

#[derive(Deserialize)]
struct BudgetFixture {
    budget: BudgetLimits,
    first_consumption: Consumption,
    rejected_consumption: Consumption,
    expected_timeline_event: String,
}

#[derive(Deserialize)]
struct BudgetLimits {
    max_tool_calls: u32,
    max_output_bytes: u64,
    max_elapsed_secs: u64,
}

#[derive(Deserialize)]
struct Consumption {
    tool_calls: u32,
    output_bytes: u64,
    elapsed_secs: u64,
}

#[derive(Deserialize)]
struct GroundedFixture {
    sanitized_notes_text: String,
    pdf_as_text: String,
    expected_citation_coverage: f32,
    expected_evidence_precision: f32,
    answers: Vec<GroundedFixtureAnswer>,
}

#[derive(Deserialize)]
struct GroundedFixtureAnswer {
    expected_supported: bool,
    answer: GroundedAnswer,
}

#[derive(Deserialize)]
struct PlannerFixture {
    generated_at: DateTime<Utc>,
    records: Vec<PlannerRecordFixture>,
    expected_conflicts: usize,
}

#[derive(Deserialize)]
struct PlannerRecordFixture {
    key: String,
    value: Value,
}

#[derive(Deserialize)]
struct VectorFixture {
    query: Vec<f32>,
    entries: Vec<VectorEntryFixture>,
    expected_top_id: String,
    expected_generation: u64,
}

#[derive(Deserialize)]
struct VectorEntryFixture {
    id: String,
    values: Vec<f32>,
}

fn thresholds() -> Thresholds {
    serde_json::from_str(include_str!("fixtures/premium_eval/thresholds.json")).unwrap()
}

#[test]
fn normal_fixture_enforces_deterministic_recommendation_threshold() {
    let fixture: RecommendationFixture = serde_json::from_str(include_str!(
        "fixtures/premium_eval/normal_recommendations.json",
    ))
    .unwrap();
    let thresholds = thresholds();

    let mastery = score_mastery(&fixture.signals);
    let recommendations = rank_recommendations(&mastery);

    assert_eq!(recommendations[0].concept_id, fixture.expected_top);
    assert!(recommendations[0].priority_score >= fixture.minimum_priority);
    assert!(recommendations[0].priority_score >= thresholds.minimum_recommendation_priority);
    assert_eq!(
        recommendations,
        rank_recommendations(&score_mastery(&fixture.signals))
    );
}

#[test]
fn negative_fixture_meets_policy_denial_threshold() {
    let fixture: InjectionFixture = serde_json::from_str(include_str!(
        "fixtures/premium_eval/negative_injection.json",
    ))
    .unwrap();
    let thresholds = thresholds();

    assert_eq!(fixture.expected_risk, "blocked");
    let denied = fixture
        .contents
        .iter()
        .filter(|content| ContentGuard.inspect(content).risk == ContentRisk::Blocked)
        .count();
    let denial_rate = denied as f32 / fixture.contents.len() as f32;

    assert!(denial_rate >= thresholds.minimum_policy_denial_rate);
}

#[test]
fn degraded_fixture_never_claims_missing_evidence() {
    let fixture: EvidenceFixture =
        serde_json::from_str(include_str!("fixtures/premium_eval/degraded_evidence.json",))
            .unwrap();

    let answer = GroundedAnswer::insufficient_evidence(fixture.answer);

    assert_eq!(fixture.expected_status, "insufficient_evidence");
    assert_eq!(answer.status, EvidenceStatus::InsufficientEvidence);
    assert!(answer.citations.is_empty());
}

#[test]
fn grounded_fixture_meets_citation_and_precision_thresholds() {
    let fixture: GroundedFixture =
        serde_json::from_str(include_str!("fixtures/premium_eval/grounded_answers.json",)).unwrap();
    let thresholds = thresholds();

    assert!(!fixture.sanitized_notes_text.trim().is_empty());
    assert!(!fixture.pdf_as_text.trim().is_empty());
    let expected_supported = fixture
        .answers
        .iter()
        .filter(|item| item.expected_supported)
        .collect::<Vec<_>>();
    let supported_with_citations = expected_supported
        .iter()
        .filter(|item| {
            item.answer.status == EvidenceStatus::Grounded
                && !item.answer.citations.is_empty()
                && item.answer.validate().is_ok()
        })
        .count();
    let citation_coverage = supported_with_citations as f32 / expected_supported.len() as f32;
    let evidence = expected_supported
        .iter()
        .flat_map(|item| item.answer.evidence.iter().map(move |span| (item, span)))
        .collect::<Vec<_>>();
    let precise_evidence = evidence
        .iter()
        .filter(|(item, span)| {
            item.answer
                .citations
                .iter()
                .any(|citation| citation.source_id == span.source_id)
        })
        .count();
    let evidence_precision = precise_evidence as f32 / evidence.len() as f32;

    assert_eq!(citation_coverage, fixture.expected_citation_coverage);
    assert_eq!(evidence_precision, fixture.expected_evidence_precision);
    assert!(citation_coverage >= thresholds.minimum_citation_coverage);
    assert!(evidence_precision >= thresholds.minimum_evidence_precision);
}

#[test]
fn recovery_fixture_rejects_budget_overrun_and_persists_cancellation() {
    let fixture: BudgetFixture =
        serde_json::from_str(include_str!("fixtures/premium_eval/recovery_budget.json",)).unwrap();
    let mut budget = RunBudget {
        max_tool_calls: Some(fixture.budget.max_tool_calls),
        max_output_bytes: Some(fixture.budget.max_output_bytes),
        max_elapsed_secs: Some(fixture.budget.max_elapsed_secs),
        ..RunBudget::default()
    };

    assert!(budget.consume(
        fixture.first_consumption.tool_calls,
        fixture.first_consumption.output_bytes,
        fixture.first_consumption.elapsed_secs,
    ));
    assert!(!budget.consume(
        fixture.rejected_consumption.tool_calls,
        fixture.rejected_consumption.output_bytes,
        fixture.rejected_consumption.elapsed_secs,
    ));

    let store = RunStore::new(MemoryStore::in_memory().unwrap());
    let cancellations = CancellationRegistry::default();
    let run_id = PipelineRunId::new();
    cancellations.cancel(run_id).unwrap();
    assert!(cancellations.token_for(run_id).unwrap().is_cancelled());
    let event = RunTimelineEvent::new(
        run_id,
        fixture.expected_timeline_event.clone(),
        "Cancellation requested by the user.",
        Utc::now(),
    );
    store.append_timeline_event(&event).unwrap();

    assert_eq!(store.timeline(run_id).unwrap(), vec![event]);
}

#[test]
fn vector_fixture_emits_exact_rerank_provenance() {
    let fixture: VectorFixture =
        serde_json::from_str(include_str!("fixtures/premium_eval/vector_provenance.json",))
            .unwrap();
    let thresholds = thresholds();
    let mut index = VectorIndex::default();

    for entry in &fixture.entries {
        index
            .insert(
                MemoryId::parse(&entry.id).unwrap(),
                Embedding::new("premium-eval", entry.values.clone()).unwrap(),
            )
            .unwrap();
    }
    let hits = index
        .search_ann(
            &Embedding::new("premium-eval", fixture.query).unwrap(),
            fixture.entries.len(),
            fixture.entries.len(),
        )
        .unwrap();

    assert_eq!(hits[0].memory_id.to_string(), fixture.expected_top_id);
    assert_eq!(hits[0].index_generation, fixture.expected_generation);
    assert!(hits[0].candidate_rank > 0);
    assert!(hits[0].exact_rerank_score >= thresholds.minimum_vector_exact_score);
}

#[test]
fn planner_fixture_surfaces_conflicts_in_local_learning_insights() {
    let fixture: PlannerFixture =
        serde_json::from_str(include_str!("fixtures/premium_eval/planner_conflicts.json",))
            .unwrap();
    let memory = MemoryStore::in_memory().unwrap();
    let srs = SrsService::new(memory.clone());
    let pages = StudentPageStore::new(memory);
    let engine = LearningEngine::new(srs, pages.clone());
    let generated_at = fixture.generated_at;

    pages
        .save_all(
            "student-planner",
            fixture
                .records
                .into_iter()
                .map(|record| StudentPageRecordInput::new(record.key, record.value, generated_at))
                .collect(),
            "premium-eval",
            generated_at,
        )
        .unwrap();
    let insights = engine.refresh(generated_at).unwrap();

    assert_eq!(insights.planner_conflicts.len(), fixture.expected_conflicts);
}
