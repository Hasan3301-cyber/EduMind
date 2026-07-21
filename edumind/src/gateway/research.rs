use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use edumind_core::{
    ClaimValidationRequest, GraphData, LiteratureGraphRequest, PipelineRunId, PluginInfo,
    ResearchProject, ResearchProjectId, ResearchRequest, ValidationReport,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    gateway::AppState,
    infra::EduMindError,
    research::{
        BibliographyExport, BibliographyFormat, DeepAnswer, FullTextDocument,
        FullTextIngestRequest, ProjectAnswer, ResearchGapsReport, ResearchPipelineResult,
        ResearchRunRecord, ResearchSupervision, ResearchSynthesis, build_literature_graph,
        validate_claims,
    },
};

type ApiError = (StatusCode, Json<Value>);
type ApiResult<T> = std::result::Result<Json<T>, ApiError>;

#[derive(Debug, Deserialize)]
pub struct ListProjectsQuery {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct AskProjectRequest {
    pub question: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ExportProjectRequest {
    #[serde(default = "default_export_format")]
    pub format: BibliographyFormat,
}

#[derive(Debug, Deserialize)]
pub struct ProjectNoteRequest {
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct ProjectQuestionRequest {
    pub question: String,
}

#[derive(Debug, Deserialize)]
pub struct ProjectScopeRequest {
    #[serde(default)]
    pub scope: Option<String>,
}

/// Runs the focused, durable research pipeline for one topic.
pub async fn run_focused(
    State(state): State<AppState>,
    Json(request): Json<ResearchRequest>,
) -> ApiResult<ResearchPipelineResult> {
    state
        .research_pipeline()
        .run(request, chrono::Utc::now())
        .await
        .map(Json)
        .map_err(api_error)
}

/// Validates externally supplied draft claims without starting a literature discovery run.
pub async fn validate_claims_handler(
    Json(request): Json<ClaimValidationRequest>,
) -> ApiResult<ValidationReport> {
    Ok(Json(validate_claims(&request)))
}

/// Builds a literature graph from supplied normalized paper metadata.
pub async fn literature_graph(Json(request): Json<LiteratureGraphRequest>) -> ApiResult<GraphData> {
    Ok(Json(build_literature_graph(&request)))
}

/// Lists the native research plugins and their deterministic stage order.
pub async fn list_pipeline_plugins(State(state): State<AppState>) -> Json<Vec<PluginInfo>> {
    Json(state.research_pipeline().plugins())
}

/// Loads a previously persisted focused research run.
pub async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> ApiResult<ResearchRunRecord> {
    let run_id = Uuid::parse_str(&run_id)
        .map(PipelineRunId)
        .map_err(|error| {
            api_error(EduMindError::Research(format!(
                "invalid research run ID: {error}"
            )))
        })?;
    state
        .research_pipeline()
        .get_run(run_id)
        .map_err(api_error)?
        .map(Json)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": {
                        "code": "research_run_not_found",
                        "message": "No persisted research run matched the supplied ID.",
                    }
                })),
            )
        })
}

/// Lists the most recently updated persisted research projects.
pub async fn list_projects(
    State(state): State<AppState>,
    Query(query): Query<ListProjectsQuery>,
) -> ApiResult<Vec<ResearchProject>> {
    state
        .project_store()
        .list_recent(query.limit.unwrap_or(20).clamp(1, 100))
        .map(Json)
        .map_err(api_error)
}

/// Loads one research project and its accumulated papers and curation state.
pub async fn get_project(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<ResearchProject> {
    state
        .project_store()
        .get(parse_project_id(&project_id)?)
        .map_err(api_error)?
        .map(Json)
        .ok_or_else(project_not_found)
}

/// Answers a question using traceable excerpts from the project's papers.
pub async fn ask_project(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(request): Json<AskProjectRequest>,
) -> ApiResult<ProjectAnswer> {
    state
        .research_projects()
        .ask(
            parse_project_id(&project_id)?,
            request.question,
            request.limit.unwrap_or(5).clamp(1, 20),
        )
        .await
        .map_err(api_error)?
        .map(Json)
        .ok_or_else(project_not_found)
}

/// Ingests a local or HTTPS PDF into a project for full-text retrieval.
pub async fn ingest_project(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(request): Json<FullTextIngestRequest>,
) -> ApiResult<FullTextDocument> {
    state
        .fulltext_projects()
        .ingest(parse_project_id(&project_id)?, request, chrono::Utc::now())
        .await
        .map_err(api_error)?
        .map(Json)
        .ok_or_else(project_not_found)
}

/// Lists metadata for every PDF body ingested into a project.
pub async fn list_project_documents(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<Vec<FullTextDocument>> {
    let project_id = parse_project_id(&project_id)?;
    if state
        .project_store()
        .get(project_id)
        .map_err(api_error)?
        .is_none()
    {
        return Err(project_not_found());
    }
    state
        .fulltext_store()
        .list_documents(project_id)
        .map(Json)
        .map_err(api_error)
}

/// Answers a question using cited passages from a project's ingested paper bodies.
pub async fn deep_ask_project(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(request): Json<AskProjectRequest>,
) -> ApiResult<DeepAnswer> {
    state
        .fulltext_projects()
        .deep_ask(
            parse_project_id(&project_id)?,
            request.question,
            request.limit.unwrap_or(5).clamp(1, 20),
        )
        .await
        .map_err(api_error)?
        .map(Json)
        .ok_or_else(project_not_found)
}

/// Returns source-linked limitations, future work, and open questions from ingested paper bodies.
pub async fn project_gaps(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<ResearchGapsReport> {
    state
        .research_supervisor()
        .gaps(parse_project_id(&project_id)?)
        .map_err(api_error)?
        .map(Json)
        .ok_or_else(project_not_found)
}

/// Builds a deterministic reading plan, corpus health review, and concrete next actions.
pub async fn project_supervise(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<ResearchSupervision> {
    state
        .research_supervisor()
        .supervise(parse_project_id(&project_id)?)
        .map_err(api_error)?
        .map(Json)
        .ok_or_else(project_not_found)
}

/// Creates a theme-aware, traceable synthesis for a persisted project.
pub async fn project_synthesis(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<ResearchSynthesis> {
    state
        .research_projects()
        .synthesis(parse_project_id(&project_id)?)
        .map_err(api_error)?
        .map(Json)
        .ok_or_else(project_not_found)
}

/// Exports the project's deduplicated bibliography as BibTeX or RIS.
pub async fn export_project(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(request): Json<ExportProjectRequest>,
) -> ApiResult<BibliographyExport> {
    state
        .research_projects()
        .export(parse_project_id(&project_id)?, request.format)
        .map_err(api_error)?
        .map(Json)
        .ok_or_else(project_not_found)
}

/// Appends a dated note to a project.
pub async fn add_project_note(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(request): Json<ProjectNoteRequest>,
) -> ApiResult<ResearchProject> {
    state
        .project_store()
        .add_note(
            parse_project_id(&project_id)?,
            request.content,
            chrono::Utc::now(),
        )
        .map_err(api_error)?
        .map(Json)
        .ok_or_else(project_not_found)
}

/// Adds a research question to a project.
pub async fn add_project_question(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(request): Json<ProjectQuestionRequest>,
) -> ApiResult<ResearchProject> {
    state
        .project_store()
        .add_question(
            parse_project_id(&project_id)?,
            request.question,
            chrono::Utc::now(),
        )
        .map_err(api_error)?
        .map(Json)
        .ok_or_else(project_not_found)
}

/// Updates the optional project scope.
pub async fn set_project_scope(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(request): Json<ProjectScopeRequest>,
) -> ApiResult<ResearchProject> {
    state
        .project_store()
        .set_scope(
            parse_project_id(&project_id)?,
            request.scope,
            chrono::Utc::now(),
        )
        .map_err(api_error)?
        .map(Json)
        .ok_or_else(project_not_found)
}

fn parse_project_id(project_id: &str) -> std::result::Result<ResearchProjectId, ApiError> {
    ResearchProjectId::parse(project_id).map_err(|error| {
        api_error(EduMindError::Research(format!(
            "invalid research project ID: {error}"
        )))
    })
}

fn project_not_found() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": {
                "code": "research_project_not_found",
                "message": "No persisted research project matched the supplied ID.",
            }
        })),
    )
}

const fn default_export_format() -> BibliographyFormat {
    BibliographyFormat::Bibtex
}

fn api_error(error: EduMindError) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": {"code": "research_request_invalid", "message": error.to_string()},
        })),
    )
}
