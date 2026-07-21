use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use chrono::{DateTime, SecondsFormat, Utc};
use edumind_core::{
    Citation, EvidenceSpan, GroundedAnswer, PipelineContext, PipelineRun, ResearchProject,
    ResearchProjectId, project_topic_key,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::{
    infra::{EduMindError, Result, SqliteMigration, apply_sqlite_migrations},
    memory::TextEmbedder,
};

use super::{
    analysis::truncate_chars,
    bibliography::{BibliographyExport, BibliographyFormat, export_bibliography},
    ranking::{rank_papers_for_query, semantic_rank_papers},
    synthesis::{ResearchSynthesis, build_synthesis},
};

/// SQLite-backed store for durable, accumulating research projects.
#[derive(Clone)]
pub struct ProjectStore {
    connection: Arc<Mutex<Connection>>,
}

/// One abstract-level evidence excerpt used to answer a project question.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectAnswerSource {
    pub paper_id: String,
    pub title: String,
    pub score: f64,
    pub excerpt: String,
}

/// A deterministic, source-linked answer over persisted project metadata and abstracts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectAnswer {
    pub project_id: ResearchProjectId,
    pub question: String,
    pub answer: String,
    #[serde(default)]
    pub sources: Vec<ProjectAnswerSource>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub grounded_answer: Option<GroundedAnswer>,
}

/// Coordinates project persistence with lexical and embedding-based abstract retrieval.
#[derive(Clone)]
pub struct ResearchProjectService {
    store: ProjectStore,
    embedder: Arc<dyn TextEmbedder>,
}

impl ResearchProjectService {
    /// Creates a project assistant over the supplied durable store and text embedder.
    #[must_use]
    pub fn new(store: ProjectStore, embedder: Arc<dyn TextEmbedder>) -> Self {
        Self { store, embedder }
    }

    /// Returns the underlying durable project repository.
    #[must_use]
    pub fn store(&self) -> ProjectStore {
        self.store.clone()
    }

    /// Answers a question strictly from the most relevant paper titles and abstracts in a project.
    pub async fn ask(
        &self,
        project_id: ResearchProjectId,
        question: impl Into<String>,
        limit: usize,
    ) -> Result<Option<ProjectAnswer>> {
        let question = question.into().trim().to_owned();
        if question.is_empty() {
            return Err(EduMindError::Research(
                "project questions require non-empty text".to_owned(),
            ));
        }
        let Some(project) = self.store.get(project_id)? else {
            return Ok(None);
        };
        if project.papers.is_empty() {
            let answer =
                "This project has no papers yet, so it has no abstract-level evidence to answer from."
                    .to_owned();
            return Ok(Some(ProjectAnswer {
                project_id,
                question,
                answer: answer.clone(),
                sources: Vec::new(),
                warnings: vec![
                    "Add papers by running the focused research pipeline first.".to_owned(),
                ],
                grounded_answer: Some(GroundedAnswer::insufficient_evidence(answer)),
            }));
        }
        let limit = limit.clamp(1, 20);
        let candidate_limit = limit.saturating_mul(4).max(limit);
        let (ranked, warnings) = match semantic_rank_papers(
            &project.papers,
            &question,
            self.embedder.as_ref(),
            candidate_limit,
        )
        .await
        {
            Ok(ranked) => (ranked, Vec::new()),
            Err(error) => (
                rank_papers_for_query(&project.papers, &question),
                vec![format!(
                    "Semantic reranking was unavailable; returned lexical project ranking instead: {error}"
                )],
            ),
        };
        let sources = ranked
            .into_iter()
            .take(limit)
            .map(|ranked| {
                let excerpt = if ranked.paper.abstract_text.trim().is_empty() {
                    truncate_chars(&ranked.paper.title, 360)
                } else {
                    truncate_chars(&ranked.paper.abstract_text, 360)
                };
                ProjectAnswerSource {
                    paper_id: ranked.paper.id,
                    title: ranked.paper.title,
                    score: ranked.score,
                    excerpt,
                }
            })
            .collect::<Vec<_>>();
        let answer = if sources.is_empty() {
            format!(
                "No project abstracts matched `{question}` strongly enough to produce a grounded answer."
            )
        } else {
            let evidence = sources
                .iter()
                .map(|source| format!("{}: {}", source.title, source.excerpt))
                .collect::<Vec<_>>()
                .join(" ");
            format!("Abstract-level evidence for `{question}`: {evidence}")
        };
        let grounded_answer = grounded_answer_from_sources(&answer, &sources)?;
        Ok(Some(ProjectAnswer {
            project_id,
            question,
            answer,
            sources,
            warnings,
            grounded_answer: Some(grounded_answer),
        }))
    }

    /// Builds the project comparison matrix and traceable outline.
    pub fn synthesis(&self, project_id: ResearchProjectId) -> Result<Option<ResearchSynthesis>> {
        self.store
            .get(project_id)
            .map(|project| project.map(|project| build_synthesis(&project.papers)))
    }

    /// Exports the project corpus in BibTeX or RIS format.
    pub fn export(
        &self,
        project_id: ResearchProjectId,
        format: BibliographyFormat,
    ) -> Result<Option<BibliographyExport>> {
        self.store
            .get(project_id)
            .map(|project| project.map(|project| export_bibliography(&project.papers, format)))
    }
}

fn grounded_answer_from_sources(
    answer: &str,
    sources: &[ProjectAnswerSource],
) -> Result<GroundedAnswer> {
    let supported = sources
        .iter()
        .filter(|source| !source.excerpt.trim().is_empty())
        .collect::<Vec<_>>();
    if supported.is_empty() {
        return Ok(GroundedAnswer::insufficient_evidence(answer));
    }
    let citations = supported
        .iter()
        .map(|source| Citation {
            source_id: source.paper_id.clone(),
            title: Some(source.title.clone()),
            locator: Some("abstract".to_owned()),
        })
        .collect::<Vec<_>>();
    let evidence = supported
        .iter()
        .map(|source| {
            EvidenceSpan::new(
                source.paper_id.clone(),
                0,
                source.excerpt.len(),
                source.excerpt.clone(),
            )
        })
        .collect::<Vec<_>>();
    GroundedAnswer::grounded(answer, citations, evidence, 0.7)
        .map_err(|error| EduMindError::Research(format!("invalid project evidence: {}", error)))
}

impl ProjectStore {
    /// Opens or creates the dedicated research-project database and applies migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|source| EduMindError::StorageIo {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        Self::from_connection(Connection::open(path)?)
    }

    /// Creates an isolated in-memory project store for deterministic tests and ephemeral sessions.
    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    /// Saves a complete project snapshot using its ID as the stable replacement key.
    pub fn save(&self, project: &ResearchProject) -> Result<()> {
        let topic_key = project_topic_key(&project.topic);
        if topic_key.is_empty() {
            return Err(EduMindError::Research(
                "research projects require a non-empty topic".to_owned(),
            ));
        }
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO research_projects (
                id, topic, topic_key, project_json, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                topic = excluded.topic,
                topic_key = excluded.topic_key,
                project_json = excluded.project_json,
                updated_at = excluded.updated_at",
            params![
                project.id.to_string(),
                &project.topic,
                topic_key,
                serde_json::to_string(project)?,
                format_timestamp(project.created_at),
                format_timestamp(project.updated_at),
            ],
        )?;
        Ok(())
    }

    /// Loads a project by its stable project ID.
    pub fn get(&self, project_id: ResearchProjectId) -> Result<Option<ResearchProject>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT project_json FROM research_projects WHERE id = ?1",
                params![project_id.to_string()],
                project_from_row,
            )
            .optional()
            .map_err(EduMindError::from)
    }

    /// Finds the most recently updated project for a case-insensitive topic match.
    pub fn find_by_topic(&self, topic: &str) -> Result<Option<ResearchProject>> {
        let topic_key = project_topic_key(topic);
        if topic_key.is_empty() {
            return Ok(None);
        }
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT project_json FROM research_projects
                 WHERE topic_key = ?1
                 ORDER BY updated_at DESC, id ASC
                 LIMIT 1",
                params![topic_key],
                project_from_row,
            )
            .optional()
            .map_err(EduMindError::from)
    }

    /// Lists complete projects in deterministic most-recent-first order.
    pub fn list_recent(&self, limit: usize) -> Result<Vec<ResearchProject>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT project_json FROM research_projects
             ORDER BY updated_at DESC, id ASC
             LIMIT ?1",
        )?;
        let mut rows = statement.query(params![i64::try_from(limit).unwrap_or(i64::MAX)])?;
        let mut projects = Vec::new();
        while let Some(row) = rows.next()? {
            projects.push(project_from_row(row)?);
        }
        Ok(projects)
    }

    /// Returns an existing topic workspace or creates and saves a new empty project.
    pub fn get_or_create(
        &self,
        topic: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<ResearchProject> {
        let topic = topic.into();
        if let Some(project) = self.find_by_topic(&topic)? {
            return Ok(project);
        }
        let project = ResearchProject::new(topic, now);
        if project.topic.is_empty() {
            return Err(EduMindError::Research(
                "research projects require a non-empty topic".to_owned(),
            ));
        }
        self.save(&project)?;
        Ok(project)
    }

    /// Merges a completed pipeline corpus into its topic project and stamps the last pipeline run.
    pub fn record_run(
        &self,
        run: &PipelineRun,
        context: &PipelineContext,
    ) -> Result<ResearchProject> {
        let mut project = self.get_or_create(&context.topic, run.updated_at)?;
        project.add_papers(context.papers.clone(), run.updated_at);
        project.last_run_id = Some(run.id);
        project.updated_at = run.updated_at;
        self.save(&project)?;
        Ok(project)
    }

    /// Adds one note to a project, returning `None` if the project does not exist.
    pub fn add_note(
        &self,
        project_id: ResearchProjectId,
        content: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<Option<ResearchProject>> {
        let Some(mut project) = self.get(project_id)? else {
            return Ok(None);
        };
        if project.add_note(content, now).is_none() {
            return Err(EduMindError::Research(
                "project notes require non-empty content".to_owned(),
            ));
        }
        self.save(&project)?;
        Ok(Some(project))
    }

    /// Adds one unique research question to a project, returning `None` if it is missing.
    pub fn add_question(
        &self,
        project_id: ResearchProjectId,
        question: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<Option<ResearchProject>> {
        let Some(mut project) = self.get(project_id)? else {
            return Ok(None);
        };
        if !project.add_question(question, now) {
            return Err(EduMindError::Research(
                "project questions must be non-empty and unique".to_owned(),
            ));
        }
        self.save(&project)?;
        Ok(Some(project))
    }

    /// Updates or clears a project scope, returning `None` when the project is missing.
    pub fn set_scope(
        &self,
        project_id: ResearchProjectId,
        scope: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<Option<ResearchProject>> {
        let Some(mut project) = self.get(project_id)? else {
            return Ok(None);
        };
        project.set_scope(scope, now);
        self.save(&project)?;
        Ok(Some(project))
    }

    fn from_connection(mut connection: Connection) -> Result<Self> {
        apply_sqlite_migrations(
            &mut connection,
            "research project database",
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;",
            &[SqliteMigration::new(
                1,
                "initial research project schema",
                "
                CREATE TABLE IF NOT EXISTS research_projects (
                    id TEXT PRIMARY KEY NOT NULL,
                    topic TEXT NOT NULL,
                    topic_key TEXT NOT NULL,
                    project_json TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_research_projects_topic_key_updated
                    ON research_projects(topic_key, updated_at DESC);
                CREATE INDEX IF NOT EXISTS idx_research_projects_updated
                    ON research_projects(updated_at DESC);
                ",
            )],
        )?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|error| EduMindError::Research(format!("project store lock failed: {error}")))
    }
}

/// Derives the dedicated research-project database path beside the configured memory database.
#[must_use]
pub fn project_database_path(memory_database_path: &Path) -> PathBuf {
    memory_database_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(
            || PathBuf::from("research_projects.db"),
            |parent| parent.join("research_projects.db"),
        )
}

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<ResearchProject> {
    let encoded: String = row.get(0)?;
    serde_json::from_str(&encoded).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

fn format_timestamp(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Micros, true)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::TimeZone;
    use edumind_core::{
        PaperMetadata, PipelineContext, PipelineRun, PipelineRunStatus, ResearchProject,
        ResearchRequest, TaskId,
    };

    use super::{ProjectStore, ResearchProjectService, project_database_path};
    use crate::memory::HashEmbedder;

    #[test]
    fn get_or_create_reuses_case_insensitive_topics_and_persists_curation() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let store = ProjectStore::in_memory().unwrap();
        let project = store.get_or_create(" Retrieval Learning ", now).unwrap();
        let reused = store
            .get_or_create("retrieval learning", now + chrono::Duration::seconds(1))
            .unwrap();

        assert_eq!(project.id, reused.id);
        let updated = store
            .add_note(project.id, "Compare course contexts.", now)
            .unwrap()
            .unwrap();
        assert_eq!(updated.notes.len(), 1);
        assert_eq!(store.list_recent(10).unwrap().len(), 1);
    }

    #[test]
    fn record_run_accumulates_and_deduplicates_topic_papers() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let store = ProjectStore::in_memory().unwrap();
        let mut run = PipelineRun::new(TaskId::new(), now);
        run.status = PipelineRunStatus::Completed;
        let mut context = PipelineContext::new(run.id, ResearchRequest::new("retrieval"), now);
        context.papers = vec![
            PaperMetadata {
                id: "one".to_owned(),
                title: "Retrieval study".to_owned(),
                ..PaperMetadata::default()
            },
            PaperMetadata {
                id: "two".to_owned(),
                title: "retrieval study".to_owned(),
                abstract_text: "Richer metadata".to_owned(),
                ..PaperMetadata::default()
            },
        ];

        let project = store.record_run(&run, &context).unwrap();

        assert_eq!(project.papers.len(), 1);
        assert_eq!(project.last_run_id, Some(run.id));
        assert_eq!(project.papers[0].abstract_text, "Richer metadata");
    }

    #[test]
    fn derives_project_database_beside_memory_database() {
        let path = project_database_path(std::path::Path::new("data/memory.db"));

        assert_eq!(path, std::path::PathBuf::from("data/research_projects.db"));
    }

    #[tokio::test]
    async fn project_service_answers_from_ranked_abstract_evidence() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let store = ProjectStore::in_memory().unwrap();
        let mut project = ResearchProject::new("retrieval", now);
        project.add_papers(
            vec![PaperMetadata {
                id: "retrieval-paper".to_owned(),
                title: "Retrieval practice for learning".to_owned(),
                abstract_text: "Retrieval practice improves learning outcomes in the study."
                    .to_owned(),
                ..PaperMetadata::default()
            }],
            now,
        );
        store.save(&project).unwrap();
        let service = ResearchProjectService::new(store, Arc::new(HashEmbedder::new(64).unwrap()));

        let answer = service
            .ask(project.id, "How does retrieval affect learning?", 3)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(answer.sources[0].paper_id, "retrieval-paper");
        assert!(answer.answer.contains("Retrieval practice"));
    }
}
