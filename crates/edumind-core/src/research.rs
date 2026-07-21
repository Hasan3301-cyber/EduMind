//! Shared research-pipeline contracts used by the gateway and native plugins.

use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::runs::PipelineRunId;

/// Relative execution priority for a research plugin within the same stage.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginPriority {
    Critical,
    High,
    #[default]
    Normal,
    Low,
}

impl PluginPriority {
    /// Returns a sortable rank where larger values run first.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Critical => 4,
            Self::High => 3,
            Self::Normal => 2,
            Self::Low => 1,
        }
    }
}

/// The pipeline positions at which a plugin is eligible to execute.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum PluginStage {
    /// The plugin runs at one exact stage position.
    Single { at: u8 },
    /// The plugin runs over an inclusive bounded range.
    Range { start: u8, end: u8 },
    /// The plugin runs at every stage from the supplied position onward.
    Continuous { start: u8 },
}

impl Default for PluginStage {
    fn default() -> Self {
        Self::Single { at: 0 }
    }
}

impl PluginStage {
    /// Creates a single-position stage declaration.
    #[must_use]
    pub const fn single(at: u8) -> Self {
        Self::Single { at }
    }

    /// Creates an inclusive range declaration, normalizing reversed bounds.
    #[must_use]
    pub const fn range(start: u8, end: u8) -> Self {
        if start <= end {
            Self::Range { start, end }
        } else {
            Self::Range {
                start: end,
                end: start,
            }
        }
    }

    /// Creates an open-ended stage declaration.
    #[must_use]
    pub const fn continuous(start: u8) -> Self {
        Self::Continuous { start }
    }

    /// Returns whether this declaration contains a pipeline position.
    #[must_use]
    pub const fn contains(self, position: u8) -> bool {
        match self {
            Self::Single { at } => at == position,
            Self::Range { start, end } => position >= start && position <= end,
            Self::Continuous { start } => position >= start,
        }
    }

    /// Returns the first position at which this plugin may execute.
    #[must_use]
    pub const fn start(self) -> u8 {
        match self {
            Self::Single { at } => at,
            Self::Range { start, .. } | Self::Continuous { start } => start,
        }
    }

    /// Resolves stable research pipeline labels to their native stage positions.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        let normalized = label.trim().to_ascii_lowercase();
        let position = match normalized.as_str() {
            "orchestrator" | "orchestration" | "orchestrator-plugin" => 0,
            "literature" | "discovery" | "literature-discovery" | "literature-discovery-plugin" => {
                10
            }
            "ranking" | "paper-ranking" | "paper-ranking-plugin" => 20,
            "graph" | "knowledge-graph" | "knowledge-graph-plugin" => 30,
            "insight" | "insights" | "insight-generator" | "insight-generator-plugin" => 40,
            "hypothesis" | "hypotheses" | "hypothesis-engine" | "hypothesis-engine-plugin" => 50,
            "critic" | "validation" | "critic-plugin" => 60,
            _ => return None,
        };
        Some(Self::single(position))
    }
}

/// Current lifecycle state of a registered plugin.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginStatus {
    #[default]
    Registered,
    Ready,
    Running,
    Completed,
    Failed,
    Disabled,
}

/// Version and configuration metadata supplied by a plugin implementation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default = "default_json_object")]
    pub configuration: Value,
}

impl PluginMetadata {
    /// Creates metadata for a native plugin with an empty configuration payload.
    #[must_use]
    pub fn native(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            author: Some("EduMind".to_owned()),
            configuration: default_json_object(),
        }
    }
}

/// Serializable result emitted by one pipeline plugin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginOutput {
    pub plugin_name: String,
    pub summary: String,
    #[serde(default = "default_json_object")]
    pub data: Value,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl PluginOutput {
    /// Creates an output with an empty structured payload.
    #[must_use]
    pub fn summary(plugin_name: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            plugin_name: plugin_name.into(),
            summary: summary.into(),
            data: default_json_object(),
            warnings: Vec::new(),
        }
    }
}

/// Inspectable registration metadata for one research plugin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub priority: PluginPriority,
    pub stage: PluginStage,
    pub description: String,
    pub status: PluginStatus,
    pub metadata: PluginMetadata,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

/// Source from which academic paper metadata was obtained.
#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum LiteratureSource {
    #[default]
    Manual,
    SemanticScholar,
    PubMed,
    Arxiv,
    Scopus,
    WebFallback,
}

/// Normalized paper metadata used throughout research discovery and analysis.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PaperMetadata {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub abstract_text: String,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub venue: Option<String>,
    #[serde(default)]
    pub doi: Option<String>,
    #[serde(default)]
    pub citation_count: u32,
    #[serde(default)]
    pub open_access_url: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub source: LiteratureSource,
    #[serde(default)]
    pub source_ids: BTreeMap<String, String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub fields_of_study: Vec<String>,
    #[serde(default)]
    pub referenced_paper_ids: Vec<String>,
    #[serde(default)]
    pub influenced_paper_ids: Vec<String>,
    #[serde(default)]
    pub fetched_at: Option<DateTime<Utc>>,
}

/// Stable identifier for one persistent research workspace.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResearchProjectId(pub Uuid);

impl ResearchProjectId {
    /// Creates a new random project identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parses a project identifier supplied by an API or persisted store.
    pub fn parse(value: &str) -> std::result::Result<Self, uuid::Error> {
        Uuid::parse_str(value).map(Self)
    }
}

impl Default for ResearchProjectId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ResearchProjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// One timestamped curator note belonging to a persistent research project.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectNote {
    pub id: Uuid,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Durable, accumulating workspace for an ongoing research topic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResearchProject {
    pub id: ResearchProjectId,
    pub topic: String,
    #[serde(default)]
    pub questions: Vec<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub papers: Vec<PaperMetadata>,
    #[serde(default)]
    pub notes: Vec<ProjectNote>,
    #[serde(default)]
    pub last_run_id: Option<PipelineRunId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ResearchProject {
    /// Creates an empty research project with a normalized topic label.
    #[must_use]
    pub fn new(topic: impl Into<String>, now: DateTime<Utc>) -> Self {
        Self {
            id: ResearchProjectId::new(),
            topic: normalize_topic(topic.into()),
            questions: Vec::new(),
            scope: None,
            papers: Vec::new(),
            notes: Vec::new(),
            last_run_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Merges papers by DOI, arXiv ID, or normalized title and returns the number newly added.
    pub fn add_papers<I>(&mut self, papers: I, now: DateTime<Utc>) -> usize
    where
        I: IntoIterator<Item = PaperMetadata>,
    {
        let mut added = 0;
        let mut changed = false;
        for candidate in papers {
            if let Some(index) = self
                .papers
                .iter()
                .position(|existing| papers_match(existing, &candidate))
            {
                let merged = merge_paper_metadata(self.papers[index].clone(), candidate);
                if self.papers[index] != merged {
                    self.papers[index] = merged;
                    changed = true;
                }
            } else {
                self.papers.push(candidate);
                added += 1;
                changed = true;
            }
        }
        if changed {
            self.papers.sort_by(|left, right| {
                normalized_title(&left.title)
                    .cmp(&normalized_title(&right.title))
                    .then_with(|| left.id.cmp(&right.id))
            });
            self.updated_at = now;
        }
        added
    }

    /// Adds one unique, non-empty research question and reports whether it was accepted.
    pub fn add_question(&mut self, question: impl Into<String>, now: DateTime<Utc>) -> bool {
        let question = question.into().trim().to_owned();
        if question.is_empty()
            || self
                .questions
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&question))
        {
            return false;
        }
        self.questions.push(question);
        self.updated_at = now;
        true
    }

    /// Adds a non-empty curator note and returns the created record.
    pub fn add_note(
        &mut self,
        content: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Option<ProjectNote> {
        let content = content.into().trim().to_owned();
        if content.is_empty() {
            return None;
        }
        let note = ProjectNote {
            id: Uuid::new_v4(),
            content,
            created_at: now,
            updated_at: now,
        };
        self.notes.push(note.clone());
        self.updated_at = now;
        Some(note)
    }

    /// Replaces the optional project scope, treating blank input as an explicit clear.
    pub fn set_scope(&mut self, scope: Option<String>, now: DateTime<Utc>) -> bool {
        let scope = scope.and_then(|scope| {
            let normalized = scope.trim().to_owned();
            (!normalized.is_empty()).then_some(normalized)
        });
        if self.scope == scope {
            return false;
        }
        self.scope = scope;
        self.updated_at = now;
        true
    }
}

/// Returns a stable, case-insensitive lookup key for project topics.
#[must_use]
pub fn project_topic_key(topic: &str) -> String {
    normalize_topic(topic.to_owned()).to_lowercase()
}

/// Returns a stable DOI → arXiv ID → normalized-title paper identity key.
#[must_use]
pub fn paper_identity_key(paper: &PaperMetadata) -> String {
    if let Some(doi) = normalized_doi(paper) {
        return format!("doi:{doi}");
    }
    if let Some(arxiv_id) = arxiv_id(paper) {
        return format!("arxiv:{arxiv_id}");
    }
    let title = normalized_title(&paper.title);
    if !title.is_empty() {
        return format!("title:{title}");
    }
    format!(
        "id:{}:{}",
        source_label(paper.source),
        normalized_identifier(&paper.id)
    )
}

/// Merges two representations of the same paper while retaining the richer available metadata.
#[must_use]
pub fn merge_paper_metadata(existing: PaperMetadata, candidate: PaperMetadata) -> PaperMetadata {
    let (mut primary, secondary) = if paper_richness(&candidate) > paper_richness(&existing) {
        (candidate, existing)
    } else {
        (existing, candidate)
    };
    primary.citation_count = primary.citation_count.max(secondary.citation_count);
    if primary.abstract_text.chars().count() < secondary.abstract_text.chars().count() {
        primary.abstract_text = secondary.abstract_text.clone();
    }
    if primary.year.is_none() {
        primary.year = secondary.year;
    }
    if primary.venue.is_none() {
        primary.venue = secondary.venue.clone();
    }
    if primary.doi.is_none() {
        primary.doi = secondary.doi.clone();
    }
    if primary.open_access_url.is_none() {
        primary.open_access_url = secondary.open_access_url.clone();
    }
    if primary.source_url.is_none() {
        primary.source_url = secondary.source_url.clone();
    }
    if secondary.fetched_at > primary.fetched_at {
        primary.fetched_at = secondary.fetched_at;
    }
    merge_strings(&mut primary.authors, secondary.authors);
    merge_strings(&mut primary.keywords, secondary.keywords);
    merge_strings(&mut primary.fields_of_study, secondary.fields_of_study);
    merge_strings(
        &mut primary.referenced_paper_ids,
        secondary.referenced_paper_ids,
    );
    merge_strings(
        &mut primary.influenced_paper_ids,
        secondary.influenced_paper_ids,
    );
    for (key, value) in secondary.source_ids {
        primary.source_ids.entry(key).or_insert(value);
    }
    primary
}

fn papers_match(left: &PaperMetadata, right: &PaperMetadata) -> bool {
    if let (Some(left_doi), Some(right_doi)) = (normalized_doi(left), normalized_doi(right)) {
        return left_doi == right_doi;
    }
    if let (Some(left_arxiv), Some(right_arxiv)) = (arxiv_id(left), arxiv_id(right)) {
        return left_arxiv == right_arxiv;
    }
    let left_title = normalized_title(&left.title);
    let right_title = normalized_title(&right.title);
    !left_title.is_empty() && left_title == right_title
}

fn normalized_doi(paper: &PaperMetadata) -> Option<String> {
    paper
        .doi
        .as_deref()
        .map(normalized_identifier)
        .filter(|doi| !doi.is_empty())
}

fn arxiv_id(paper: &PaperMetadata) -> Option<String> {
    paper
        .source_ids
        .get("arxiv")
        .map(String::as_str)
        .or_else(|| {
            (paper.source == LiteratureSource::Arxiv)
                .then_some(paper.id.strip_prefix("arxiv:").unwrap_or(paper.id.as_str()))
        })
        .map(normalized_identifier)
        .filter(|identifier| !identifier.is_empty())
}

fn paper_richness(paper: &PaperMetadata) -> (u32, usize, i32, usize, String) {
    (
        paper.citation_count,
        paper.abstract_text.chars().count(),
        paper.year.unwrap_or(i32::MIN),
        paper.keywords.len() + paper.fields_of_study.len(),
        paper.id.clone(),
    )
}

fn merge_strings(target: &mut Vec<String>, values: Vec<String>) {
    let mut merged = target
        .iter()
        .chain(&values)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    *target = std::mem::take(&mut merged).into_iter().collect();
}

fn normalize_topic(topic: String) -> String {
    topic.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalized_identifier(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("https://doi.org/")
        .trim_start_matches("http://doi.org/")
        .trim_start_matches("doi:")
        .to_ascii_lowercase()
}

fn normalized_title(value: &str) -> String {
    value
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

const fn source_label(source: LiteratureSource) -> &'static str {
    match source {
        LiteratureSource::Manual => "manual",
        LiteratureSource::SemanticScholar => "semantic_scholar",
        LiteratureSource::PubMed => "pubmed",
        LiteratureSource::Arxiv => "arxiv",
        LiteratureSource::Scopus => "scopus",
        LiteratureSource::WebFallback => "web_fallback",
    }
}

/// A normalized text document associated with a discovered paper.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StructuredDocument {
    pub id: String,
    pub paper_id: String,
    pub title: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub extracted_at: Option<DateTime<Utc>>,
    #[serde(default = "default_json_object")]
    pub metadata: Value,
}

/// The deterministic category assigned to a generated research insight.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsightType {
    DominantTheme,
    EmergingTrend,
    MaturingField,
    ResearchGap,
    CorpusNovelty,
}

/// Evidence-grounded observation generated from a research corpus.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResearchInsight {
    pub id: String,
    pub insight_type: InsightType,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub evidence_paper_ids: Vec<String>,
    pub confidence: f64,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
}

/// A testable proposition grounded in the available literature corpus.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hypothesis {
    pub id: String,
    pub statement: String,
    pub rationale: String,
    pub test_method: String,
    #[serde(default)]
    pub evidence_paper_ids: Vec<String>,
    pub confidence: f64,
}

/// A single paper and its deterministic ranking score.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RankedPaper {
    pub paper: PaperMetadata,
    pub score: f64,
}

/// A node in a literature or knowledge graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub weight: f64,
    #[serde(default = "default_json_object")]
    pub metadata: Value,
}

/// A weighted directed relationship in a literature or knowledge graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub weight: f64,
}

/// A deterministic community of connected literature nodes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Community {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub node_ids: Vec<String>,
}

/// Graph output built from paper citations and concept similarity.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GraphData {
    #[serde(default)]
    pub nodes: Vec<GraphNode>,
    #[serde(default)]
    pub edges: Vec<GraphEdge>,
    #[serde(default)]
    pub communities: Vec<Community>,
}

/// A source excerpt supplied to claim validation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EvidenceSource {
    pub id: String,
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub citation_label: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
}

/// Support classification for one draft claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimSupport {
    Supported,
    Partial,
    Unsupported,
}

/// Evidence match and quality assessment for a single claim.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClaimAssessment {
    pub claim: String,
    pub support: ClaimSupport,
    pub support_score: f64,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

/// An actionable issue found during claim validation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValidationIssue {
    #[serde(default)]
    pub claim_index: Option<usize>,
    pub code: String,
    pub message: String,
}

/// Complete deterministic review of claims against supplied evidence.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ValidationReport {
    #[serde(default)]
    pub claims: Vec<ClaimAssessment>,
    #[serde(default)]
    pub hallucinations: Vec<ValidationIssue>,
    #[serde(default)]
    pub citation_errors: Vec<ValidationIssue>,
    #[serde(default)]
    pub logical_issues: Vec<ValidationIssue>,
    #[serde(default)]
    pub bias_flags: Vec<ValidationIssue>,
    pub overall_score: f64,
}

/// Input for deterministic claim validation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClaimValidationRequest {
    #[serde(default)]
    pub claims: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<EvidenceSource>,
    #[serde(default = "default_support_threshold")]
    pub support_threshold: f64,
}

/// Input for literature graph construction.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LiteratureGraphRequest {
    #[serde(default)]
    pub papers: Vec<PaperMetadata>,
    #[serde(default = "default_similarity_threshold")]
    pub similarity_threshold: f64,
}

/// User or agent input accepted by the focused research pipeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResearchRequest {
    pub topic: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub seed_papers: Vec<PaperMetadata>,
    #[serde(default)]
    pub claims: Vec<String>,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    #[serde(default = "default_literature_sources")]
    pub sources: Vec<LiteratureSource>,
}

impl ResearchRequest {
    /// Creates a request that uses its topic as the literature search query.
    #[must_use]
    pub fn new(topic: impl Into<String>) -> Self {
        let topic = topic.into();
        Self {
            query: topic.clone(),
            topic,
            seed_papers: Vec::new(),
            claims: Vec::new(),
            max_results: default_max_results(),
            sources: default_literature_sources(),
        }
    }

    /// Returns the explicit query or falls back to the topic for discovery.
    #[must_use]
    pub fn effective_query(&self) -> &str {
        if self.query.trim().is_empty() {
            &self.topic
        } else {
            &self.query
        }
    }
}

impl Default for ResearchRequest {
    fn default() -> Self {
        Self::new(String::new())
    }
}

/// Accumulator passed through the ordered research plugin stages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PipelineContext {
    pub run_id: PipelineRunId,
    pub topic: String,
    pub query: String,
    #[serde(default)]
    pub query_terms: Vec<String>,
    #[serde(default)]
    pub requested_sources: Vec<LiteratureSource>,
    pub max_results: usize,
    #[serde(default)]
    pub papers: Vec<PaperMetadata>,
    #[serde(default)]
    pub ranked_papers: Vec<RankedPaper>,
    #[serde(default)]
    pub insights: Vec<ResearchInsight>,
    #[serde(default)]
    pub hypotheses: Vec<Hypothesis>,
    #[serde(default)]
    pub graph: Option<GraphData>,
    #[serde(default)]
    pub validation: Option<ValidationReport>,
    #[serde(default)]
    pub claims: Vec<String>,
    #[serde(default)]
    pub plugin_outputs: Vec<PluginOutput>,
    #[serde(default)]
    pub events: Vec<PipelineEvent>,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub started_at: DateTime<Utc>,
}

impl PipelineContext {
    /// Builds an empty accumulator seeded with caller-provided literature.
    #[must_use]
    pub fn new(run_id: PipelineRunId, request: ResearchRequest, now: DateTime<Utc>) -> Self {
        let query = request.effective_query().to_owned();
        let query_terms = query
            .split_whitespace()
            .map(|term| term.to_ascii_lowercase())
            .filter(|term| !term.is_empty())
            .collect();
        Self {
            run_id,
            topic: request.topic,
            query,
            query_terms,
            requested_sources: request.sources,
            max_results: request.max_results.max(1),
            papers: request.seed_papers,
            ranked_papers: Vec::new(),
            insights: Vec::new(),
            hypotheses: Vec::new(),
            graph: None,
            validation: None,
            claims: request.claims,
            plugin_outputs: Vec::new(),
            events: Vec::new(),
            warnings: Vec::new(),
            started_at: now,
        }
    }
}

/// Progress signal emitted whenever a plugin changes the pipeline state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PipelineProgress {
    pub run_id: PipelineRunId,
    pub plugin_name: String,
    pub completed_stages: usize,
    pub total_stages: usize,
    pub message: String,
    pub at: DateTime<Utc>,
}

/// Durable progress event emitted by a pipeline run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PipelineEvent {
    pub progress: PipelineProgress,
    #[serde(default)]
    pub output: Option<PluginOutput>,
}

/// Typed contract implemented by every native research pipeline plugin.
#[async_trait]
pub trait ResearchPlugin: Send + Sync {
    /// Stable plugin identifier used in manifests, run records, and dependencies.
    fn name(&self) -> &'static str;

    /// Relative priority used to order plugins in a shared stage.
    fn priority(&self) -> PluginPriority;

    /// Pipeline stage eligibility declaration.
    fn stage(&self) -> PluginStage;

    /// Short human-readable description of the plugin's responsibility.
    fn description(&self) -> &'static str;

    /// Initializes the plugin before its first use.
    async fn initialize(&self) -> std::result::Result<(), String> {
        Ok(())
    }

    /// Mutates the accumulated context and returns a serializable stage result.
    async fn execute(
        &self,
        context: &mut PipelineContext,
    ) -> std::result::Result<PluginOutput, String>;

    /// Indicates whether the plugin should run for the supplied request context.
    fn should_run(&self, _context: &PipelineContext) -> bool {
        true
    }

    /// Names of plugins that must finish successfully first.
    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    /// Supports typed inspection of registered plugin implementations.
    fn as_any(&self) -> &dyn Any;
}

fn default_json_object() -> Value {
    json!({})
}

const fn default_max_results() -> usize {
    20
}

const fn default_support_threshold() -> f64 {
    0.35
}

const fn default_similarity_threshold() -> f64 {
    0.20
}

fn default_literature_sources() -> Vec<LiteratureSource> {
    vec![
        LiteratureSource::SemanticScholar,
        LiteratureSource::PubMed,
        LiteratureSource::Arxiv,
        LiteratureSource::Scopus,
        LiteratureSource::WebFallback,
    ]
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::{PaperMetadata, PluginStage, ResearchProject, ResearchRequest};

    #[test]
    fn plugin_stage_normalizes_ranges_and_labels() {
        let stage = PluginStage::range(8, 2);

        assert!(stage.contains(2));
        assert!(stage.contains(8));
        assert!(!stage.contains(9));
        assert_eq!(
            PluginStage::from_label("ranking"),
            Some(PluginStage::single(20))
        );
    }

    #[test]
    fn research_request_uses_topic_when_query_is_empty() {
        let mut request = ResearchRequest::new("Causal learning");
        request.query.clear();

        assert_eq!(request.effective_query(), "Causal learning");
    }

    #[test]
    fn project_accumulates_deduplicated_papers_and_curated_state() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let mut project = ResearchProject::new(" Retrieval learning ", now);
        let sparse = PaperMetadata {
            id: "first".to_owned(),
            title: "Retrieval Learning Outcomes".to_owned(),
            ..PaperMetadata::default()
        };
        let richer = PaperMetadata {
            id: "second".to_owned(),
            title: "Retrieval learning outcomes".to_owned(),
            doi: Some("10.1/example".to_owned()),
            abstract_text: "A detailed abstract.".to_owned(),
            citation_count: 4,
            ..PaperMetadata::default()
        };

        assert_eq!(project.add_papers(vec![sparse], now), 1);
        assert_eq!(project.add_papers(vec![richer], now), 0);
        assert_eq!(project.topic, "Retrieval learning");
        assert_eq!(project.papers.len(), 1);
        assert_eq!(project.papers[0].citation_count, 4);
        assert!(project.add_question("What changes outcomes?", now));
        assert!(!project.add_question("what changes outcomes?", now));
        assert!(
            project
                .add_note("Compare classroom studies.", now)
                .is_some()
        );
        assert!(project.set_scope(Some("Undergraduate learning".to_owned()), now));
    }
}
