use std::collections::{BTreeMap, BTreeSet};

use chrono::{Datelike, Utc};
use edumind_core::{PaperMetadata, ResearchProjectId};
use serde::{Deserialize, Serialize};

use crate::infra::Result;

use super::{
    FullTextStore, ProjectStore, StoredFullText,
    analysis::{paper_concepts, truncate_chars},
};

const MAX_GAPS_PER_PAPER: usize = 5;
const MAX_OPEN_QUESTIONS: usize = 5;
const RECENT_PAPER_WINDOW_YEARS: i32 = 3;

const LIMITATION_MARKERS: &[&str] = &[
    "limitation",
    "limited by",
    "limited to",
    "small sample",
    "insufficient",
    "shortcoming",
    "drawback",
    "cannot generalize",
    "not generalizable",
    "lack of",
    "constraint",
];
const FUTURE_WORK_MARKERS: &[&str] = &[
    "future work",
    "future research",
    "further research",
    "should investigate",
    "should explore",
    "could investigate",
    "remain to be explored",
    "remains to be explored",
];
const OPEN_QUESTION_MARKERS: &[&str] = &[
    "open question",
    "remains unclear",
    "unclear whether",
    "unknown whether",
    "not yet known",
    "remains unknown",
    "it is not known",
];

/// Type of author-stated uncertainty identified in a paper body.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatedGapKind {
    Limitation,
    FutureWork,
    OpenQuestion,
}

impl StatedGapKind {
    fn label(self) -> &'static str {
        match self {
            Self::Limitation => "limitation",
            Self::FutureWork => "future work",
            Self::OpenQuestion => "open question",
        }
    }
}

/// Source metadata retained with an extracted gap so every advisory remains traceable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GapProvenance {
    pub paper_id: String,
    pub title: String,
    pub source: String,
    pub locator: String,
}

/// One bounded author-stated limitation, future direction, or unresolved question.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StatedGap {
    pub kind: StatedGapKind,
    pub excerpt: String,
    pub provenance: GapProvenance,
}

/// A ranked paper recommendation for the next focused reading session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReadingPlanItem {
    pub priority: usize,
    pub paper_id: String,
    pub title: String,
    #[serde(default)]
    pub year: Option<i32>,
    pub citation_count: u32,
    pub ingested: bool,
    pub rationale: String,
}

/// Corpus coverage observations that should shape the learner's next actions.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CorpusHealth {
    pub total_papers: usize,
    pub ingested_documents: usize,
    #[serde(default)]
    pub newest_paper_year: Option<i32>,
    #[serde(default)]
    pub advisories: Vec<String>,
}

/// Focused output for the gaps endpoint.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResearchGapsReport {
    pub project_id: ResearchProjectId,
    pub topic: String,
    #[serde(default)]
    pub stated_gaps: Vec<StatedGap>,
    pub corpus_health: CorpusHealth,
}

/// A deterministic research-supervisor report over durable project and full-text data.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResearchSupervision {
    pub topic: String,
    #[serde(default)]
    pub reading_plan: Vec<ReadingPlanItem>,
    #[serde(default)]
    pub stated_gaps: Vec<StatedGap>,
    #[serde(default)]
    pub open_questions: Vec<String>,
    pub corpus_health: CorpusHealth,
    #[serde(default)]
    pub next_steps: Vec<String>,
}

/// Coordinates gap extraction and supervision reports with the durable research stores.
#[derive(Clone)]
pub struct ResearchSupervisorService {
    projects: ProjectStore,
    fulltext: FullTextStore,
}

impl ResearchSupervisorService {
    /// Creates a supervisor over the project metadata and indexed paper bodies.
    #[must_use]
    pub fn new(projects: ProjectStore, fulltext: FullTextStore) -> Self {
        Self { projects, fulltext }
    }

    /// Returns author-stated gaps for a project, or `None` when it does not exist.
    pub fn gaps(&self, project_id: ResearchProjectId) -> Result<Option<ResearchGapsReport>> {
        let Some(project) = self.projects.get(project_id)? else {
            return Ok(None);
        };
        let documents = self.fulltext.full_texts(project_id)?;
        let stated_gaps = extract_stated_gaps(&documents);
        Ok(Some(ResearchGapsReport {
            project_id,
            topic: project.topic,
            corpus_health: corpus_health(&project.papers, &documents),
            stated_gaps,
        }))
    }

    /// Returns a reading plan and concrete next actions for a project, or `None` when missing.
    pub fn supervise(&self, project_id: ResearchProjectId) -> Result<Option<ResearchSupervision>> {
        let Some(project) = self.projects.get(project_id)? else {
            return Ok(None);
        };
        let documents = self.fulltext.full_texts(project_id)?;
        Ok(Some(build_supervision(
            &project.topic,
            &project.papers,
            &documents,
        )))
    }
}

/// Extracts bounded, author-stated gaps from the full text of each indexed document.
#[must_use]
pub fn extract_stated_gaps(documents: &[StoredFullText]) -> Vec<StatedGap> {
    let mut documents = documents.to_vec();
    documents.sort_by(|left, right| {
        left.document
            .paper_id
            .cmp(&right.document.paper_id)
            .then_with(|| left.document.title.cmp(&right.document.title))
    });
    let mut gaps = Vec::new();

    for stored in documents {
        let mut seen = BTreeSet::new();
        let mut count = 0;
        for sentence in split_sentences(&stored.full_text) {
            let normalized = sentence.to_lowercase();
            let Some(kind) = classify_gap(&normalized) else {
                continue;
            };
            if !seen.insert(normalized) {
                continue;
            }
            gaps.push(StatedGap {
                kind,
                excerpt: truncate_chars(&sentence, 480),
                provenance: GapProvenance {
                    paper_id: stored.document.paper_id.clone(),
                    title: stored.document.title.clone(),
                    source: stored.document.source.clone(),
                    locator: format!("{}#full-text", stored.document.paper_id),
                },
            });
            count += 1;
            if count >= MAX_GAPS_PER_PAPER {
                break;
            }
        }
    }

    gaps
}

/// Builds an actionable plan from the project topic, paper metadata, and ingested paper bodies.
#[must_use]
pub fn build_supervision(
    topic: &str,
    papers: &[PaperMetadata],
    documents: &[StoredFullText],
) -> ResearchSupervision {
    let stated_gaps = extract_stated_gaps(documents);
    let corpus_health = corpus_health(papers, documents);
    let reading_plan = build_reading_plan(papers, documents);
    let open_questions = open_questions(topic, papers, &stated_gaps);
    let next_steps = next_steps(&stated_gaps, &corpus_health, &open_questions);

    ResearchSupervision {
        topic: topic.trim().to_owned(),
        reading_plan,
        stated_gaps,
        open_questions,
        corpus_health,
        next_steps,
    }
}

fn build_reading_plan(
    papers: &[PaperMetadata],
    documents: &[StoredFullText],
) -> Vec<ReadingPlanItem> {
    let ingested = documents
        .iter()
        .map(|document| document.document.paper_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut papers = papers.to_vec();
    papers.sort_by(|left, right| {
        right
            .citation_count
            .cmp(&left.citation_count)
            .then_with(|| right.year.cmp(&left.year))
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });

    papers
        .into_iter()
        .enumerate()
        .map(|(index, paper)| {
            let has_full_text = ingested.contains(paper.id.as_str());
            let recency = paper
                .year
                .map_or_else(|| "undated".to_owned(), |year| year.to_string());
            let evidence = if has_full_text {
                "full text is ingested"
            } else {
                "abstract-only evidence is available"
            };
            ReadingPlanItem {
                priority: index + 1,
                paper_id: paper.id,
                title: paper.title,
                year: paper.year,
                citation_count: paper.citation_count,
                ingested: has_full_text,
                rationale: format!(
                    "Prioritized by {} citations, then publication year ({recency}); {evidence}.",
                    paper.citation_count
                ),
            }
        })
        .collect()
}

fn corpus_health(papers: &[PaperMetadata], documents: &[StoredFullText]) -> CorpusHealth {
    let total_papers = papers.len();
    let paper_ids = papers
        .iter()
        .map(|paper| paper.id.as_str())
        .collect::<BTreeSet<_>>();
    let ingested_documents = documents
        .iter()
        .filter(|document| paper_ids.contains(document.document.paper_id.as_str()))
        .count();
    let newest_paper_year = papers.iter().filter_map(|paper| paper.year).max();
    let mut advisories = Vec::new();

    if total_papers == 0 {
        advisories.push("no papers are in this project yet".to_owned());
    } else if ingested_documents < total_papers {
        advisories.push(format!(
            "only {ingested_documents} of {total_papers} papers ingested; add full text for deeper supervision"
        ));
    }

    let recent_cutoff = Utc::now().year() - (RECENT_PAPER_WINDOW_YEARS - 1);
    if newest_paper_year.is_none_or(|year| year < recent_cutoff) {
        advisories.push(format!(
            "no recent papers (published since {recent_cutoff}) were found"
        ));
    }

    CorpusHealth {
        total_papers,
        ingested_documents,
        newest_paper_year,
        advisories,
    }
}

fn open_questions(topic: &str, papers: &[PaperMetadata], gaps: &[StatedGap]) -> Vec<String> {
    if !gaps.is_empty() {
        return gaps
            .iter()
            .take(MAX_OPEN_QUESTIONS)
            .map(|gap| {
                format!(
                    "Investigate the {} stated in {}: {}",
                    gap.kind.label(),
                    gap.provenance.title,
                    gap.excerpt
                )
            })
            .collect();
    }

    let mut frequencies = BTreeMap::<String, usize>::new();
    for paper in papers {
        for concept in paper_concepts(paper) {
            *frequencies.entry(concept).or_insert(0) += 1;
        }
    }
    frequencies
        .into_iter()
        .filter(|(_, count)| *count <= 1)
        .take(MAX_OPEN_QUESTIONS)
        .map(|(concept, _)| format!("How does {topic} vary across {concept}?"))
        .collect()
}

fn next_steps(
    stated_gaps: &[StatedGap],
    corpus_health: &CorpusHealth,
    open_questions: &[String],
) -> Vec<String> {
    let mut steps = Vec::new();
    if corpus_health.total_papers == 0 {
        steps.push("Run focused research to add an initial evidence corpus.".to_owned());
    }
    let missing = corpus_health
        .total_papers
        .saturating_sub(corpus_health.ingested_documents);
    if missing > 0 {
        steps.push(format!(
            "Ingest full text for {missing} more paper(s) before relying on deep supervision."
        ));
    }
    if let Some(gap) = stated_gaps.first() {
        steps.push(format!(
            "Turn the top {} into a scoped research question with an observable outcome.",
            gap.kind.label()
        ));
    } else if let Some(question) = open_questions.first() {
        steps.push(format!(
            "Choose and refine this corpus-derived question: {question}"
        ));
    }
    if corpus_health
        .advisories
        .iter()
        .any(|advisory| advisory.starts_with("no recent papers"))
    {
        steps.push("Add recent literature before finalizing a research direction.".to_owned());
    }
    steps
}

fn split_sentences(text: &str) -> Vec<String> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized
        .split_inclusive(['.', '!', '?'])
        .map(str::trim)
        .filter(|sentence| sentence.len() >= 24)
        .map(ToOwned::to_owned)
        .collect()
}

fn classify_gap(sentence: &str) -> Option<StatedGapKind> {
    if contains_marker(sentence, FUTURE_WORK_MARKERS) {
        Some(StatedGapKind::FutureWork)
    } else if contains_marker(sentence, OPEN_QUESTION_MARKERS) {
        Some(StatedGapKind::OpenQuestion)
    } else if contains_marker(sentence, LIMITATION_MARKERS) {
        Some(StatedGapKind::Limitation)
    } else {
        None
    }
}

fn contains_marker(sentence: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| sentence.contains(marker))
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use edumind_core::{PaperMetadata, ResearchProjectId};

    use super::{ResearchSupervisorService, StatedGapKind, build_supervision, extract_stated_gaps};
    use crate::research::{
        EmbeddedTextChunk, FullTextDocument, FullTextStore, NewFullTextDocument, ProjectStore,
        StoredFullText,
    };

    fn stored_document(paper_id: &str, text: &str) -> StoredFullText {
        StoredFullText {
            document: FullTextDocument {
                project_id: ResearchProjectId::new(),
                paper_id: paper_id.to_owned(),
                title: format!("{paper_id} title"),
                source: format!("{paper_id}.pdf"),
                ocr: false,
                char_count: text.len(),
                chunk_count: 1,
                extracted_at: Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
            },
            full_text: text.to_owned(),
        }
    }

    #[test]
    fn extracts_classified_author_stated_gaps_with_provenance() {
        let documents = vec![stored_document(
            "paper-a",
            "This study is limited by a small sample. Future work should investigate diverse classrooms. It remains unclear whether the effect transfers to exams.",
        )];

        let gaps = extract_stated_gaps(&documents);

        assert_eq!(gaps.len(), 3);
        assert_eq!(gaps[0].kind, StatedGapKind::Limitation);
        assert_eq!(gaps[1].kind, StatedGapKind::FutureWork);
        assert_eq!(gaps[2].kind, StatedGapKind::OpenQuestion);
        assert_eq!(gaps[0].provenance.locator, "paper-a#full-text");
    }

    #[test]
    fn supervision_prioritizes_citations_then_recency_and_reports_health() {
        let documents = vec![stored_document(
            "high-impact",
            "The study limitation is limited by a small sample.",
        )];
        let papers = vec![
            PaperMetadata {
                id: "recent".to_owned(),
                title: "Recent paper".to_owned(),
                year: Some(2026),
                citation_count: 5,
                keywords: vec!["equity".to_owned()],
                ..PaperMetadata::default()
            },
            PaperMetadata {
                id: "high-impact".to_owned(),
                title: "High impact paper".to_owned(),
                year: Some(2024),
                citation_count: 99,
                ..PaperMetadata::default()
            },
        ];

        let report = build_supervision("retrieval practice", &papers, &documents);

        assert_eq!(report.reading_plan[0].paper_id, "high-impact");
        assert!(report.reading_plan[0].ingested);
        assert_eq!(report.corpus_health.ingested_documents, 1);
        assert!(
            report
                .corpus_health
                .advisories
                .iter()
                .any(|advisory| advisory.contains("only 1 of 2 papers ingested"))
        );
        assert!(!report.open_questions.is_empty());
    }

    #[test]
    fn service_returns_none_for_missing_project() {
        let service = ResearchSupervisorService::new(
            ProjectStore::in_memory().unwrap(),
            FullTextStore::in_memory().unwrap(),
        );

        assert!(service.gaps(ResearchProjectId::new()).unwrap().is_none());
        assert!(
            service
                .supervise(ResearchProjectId::new())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn service_reads_persisted_project_and_full_text() {
        let now = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();
        let projects = ProjectStore::in_memory().unwrap();
        let fulltext = FullTextStore::in_memory().unwrap();
        let mut project = edumind_core::ResearchProject::new("supervision", now);
        project.add_papers(
            vec![PaperMetadata {
                id: "paper-one".to_owned(),
                title: "Paper one".to_owned(),
                year: Some(2026),
                ..PaperMetadata::default()
            }],
            now,
        );
        projects.save(&project).unwrap();
        fulltext
            .store_document(
                &NewFullTextDocument {
                    project_id: project.id,
                    paper_id: "paper-one".to_owned(),
                    title: "Paper one".to_owned(),
                    source: "paper-one.pdf".to_owned(),
                    ocr: false,
                    full_text: "Future work should explore longitudinal effects.".to_owned(),
                    extracted_at: now,
                },
                &[EmbeddedTextChunk {
                    index: 0,
                    text: "Future work should explore longitudinal effects.".to_owned(),
                    embedding: vec![1.0, 0.0],
                }],
            )
            .unwrap();
        let service = ResearchSupervisorService::new(projects, fulltext);

        let report = service.supervise(project.id).unwrap().unwrap();

        assert_eq!(report.stated_gaps.len(), 1);
        assert_eq!(report.stated_gaps[0].kind, StatedGapKind::FutureWork);
    }
}
