use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::Utc;
use edumind_core::GraphData;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    gateway::AppState,
    infra::EduMindError,
    memory::{
        AdvancedSearchHit, GraphNeighborhood, GraphSearchHit, HermesCycle, MemoryId,
        MemoryIngestionResult, MemoryIntelligenceSnapshot, MemoryRecord, ModuleMemoryHit,
        ModuleMemoryScope, ModuleMemorySummary, NewModuleMemory, WikiSearchHit,
    },
};

type ApiError = (StatusCode, Json<Value>);
type ApiResult<T> = std::result::Result<Json<T>, ApiError>;

#[derive(Debug, Deserialize)]
pub struct SearchMemoryRequest {
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct GetMemoryRequest {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct StoreMemoryRequest {
    pub module_id: String,
    pub content: String,
    #[serde(default = "default_content_type")]
    pub content_type: String,
    #[serde(default)]
    pub scope: ModuleMemoryScope,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Deserialize)]
pub struct ModuleMemorySearchRequest {
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default, alias = "scope")]
    pub access_scope: ModuleMemoryScope,
    #[serde(default)]
    pub content_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ModuleMemoryGetRequest {
    pub id: String,
    #[serde(default, alias = "scope")]
    pub access_scope: ModuleMemoryScope,
}

#[derive(Debug, Deserialize)]
pub struct ModuleMemoryStoreRequest {
    pub content: String,
    #[serde(default = "default_content_type")]
    pub content_type: String,
    #[serde(default)]
    pub scope: ModuleMemoryScope,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Deserialize)]
pub struct GraphNeighborsRequest {
    pub node_id: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct HermesCyclesQuery {
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Runs advanced hybrid retrieval with the deterministic Jaccard/MMR reranker.
pub async fn memory_search(
    State(state): State<AppState>,
    Json(request): Json<SearchMemoryRequest>,
) -> ApiResult<Vec<AdvancedSearchHit>> {
    state
        .memory_intelligence()
        .search(&request.query, bounded_limit(request.limit))
        .await
        .map(Json)
        .map_err(api_error)
}

/// Lists raw local memory records for trusted local administration.
pub async fn list_memory(State(state): State<AppState>) -> ApiResult<Vec<MemoryRecord>> {
    state
        .memory_intelligence()
        .list()
        .map(Json)
        .map_err(api_error)
}

/// Loads one raw local memory record by stable ID.
pub async fn memory_get(
    State(state): State<AppState>,
    Json(request): Json<GetMemoryRequest>,
) -> ApiResult<MemoryRecord> {
    let memory_id = MemoryId::parse(&request.id).map_err(api_error)?;
    state
        .memory_intelligence()
        .get(memory_id)
        .map_err(api_error)?
        .map(Json)
        .ok_or_else(|| not_found("memory_not_found", "Memory record does not exist."))
}

/// Stores one embedded local memory and refreshes graph/wiki-derived coverage.
pub async fn memory_store(
    State(state): State<AppState>,
    Json(request): Json<StoreMemoryRequest>,
) -> ApiResult<MemoryIngestionResult> {
    let input = NewModuleMemory {
        content: request.content,
        content_type: request.content_type,
        scope: request.scope,
        metadata: request.metadata,
    };
    state
        .memory_intelligence()
        .ingest(request.module_id, input, Utc::now())
        .await
        .map(Json)
        .map_err(api_error)
}

/// Returns graph/wiki/index coverage for a local memory-intelligence status panel.
pub async fn memory_snapshot(
    State(state): State<AppState>,
) -> ApiResult<MemoryIntelligenceSnapshot> {
    state
        .memory_intelligence()
        .snapshot()
        .map(Json)
        .map_err(api_error)
}

/// Returns the complete derived knowledge graph and communities for visualization clients.
pub async fn memory_graph(State(state): State<AppState>) -> ApiResult<GraphData> {
    state
        .memory_intelligence()
        .graph()
        .map(Json)
        .map_err(api_error)
}

/// Searches extracted knowledge-graph concepts and memory excerpts.
pub async fn search_graph(
    State(state): State<AppState>,
    Json(request): Json<SearchMemoryRequest>,
) -> ApiResult<Vec<GraphSearchHit>> {
    state
        .memory_intelligence()
        .search_graph(&request.query, bounded_limit(request.limit))
        .map(Json)
        .map_err(api_error)
}

/// Returns direct neighbors for a selected graph node.
pub async fn graph_neighbors(
    State(state): State<AppState>,
    Json(request): Json<GraphNeighborsRequest>,
) -> ApiResult<GraphNeighborhood> {
    state
        .memory_intelligence()
        .graph_neighbors(&request.node_id)
        .map_err(api_error)?
        .map(Json)
        .ok_or_else(|| {
            not_found(
                "graph_node_not_found",
                "Knowledge graph node does not exist.",
            )
        })
}

/// Searches locally generated, source-linked concept wiki pages.
pub async fn search_wiki(
    State(state): State<AppState>,
    Json(request): Json<SearchMemoryRequest>,
) -> ApiResult<Vec<WikiSearchHit>> {
    state
        .memory_intelligence()
        .search_wiki(&request.query, bounded_limit(request.limit))
        .map(Json)
        .map_err(api_error)
}

/// Searches embedded memory visible to one module under its requested access scope.
pub async fn module_memory_search(
    State(state): State<AppState>,
    Path(module_id): Path<String>,
    Json(request): Json<ModuleMemorySearchRequest>,
) -> ApiResult<Vec<ModuleMemoryHit>> {
    state
        .memory_intelligence()
        .modules()
        .search_by_content_type(
            module_id,
            &request.query,
            request.access_scope,
            request.content_type.as_deref(),
            bounded_limit(request.limit),
        )
        .await
        .map(Json)
        .map_err(api_error)
}

/// Stores an embedded module memory with durable private/module/cross-module/global scope.
pub async fn module_memory_store(
    State(state): State<AppState>,
    Path(module_id): Path<String>,
    Json(request): Json<ModuleMemoryStoreRequest>,
) -> ApiResult<MemoryIngestionResult> {
    let input = NewModuleMemory {
        content: request.content,
        content_type: request.content_type,
        scope: request.scope,
        metadata: request.metadata,
    };
    state
        .memory_intelligence()
        .ingest(module_id, input, Utc::now())
        .await
        .map(Json)
        .map_err(api_error)
}

/// Loads one module-visible memory record by ID.
pub async fn module_memory_get(
    State(state): State<AppState>,
    Path(module_id): Path<String>,
    Json(request): Json<ModuleMemoryGetRequest>,
) -> ApiResult<MemoryRecord> {
    let memory_id = MemoryId::parse(&request.id).map_err(api_error)?;
    state
        .memory_intelligence()
        .modules()
        .get(module_id, memory_id, request.access_scope)
        .map_err(api_error)?
        .map(Json)
        .ok_or_else(|| {
            not_found(
                "module_memory_not_found",
                "Module memory is absent or not visible.",
            )
        })
}

/// Returns a scope-aware inventory and recent excerpts for one module namespace.
pub async fn module_memory_summary(
    State(state): State<AppState>,
    Path(module_id): Path<String>,
) -> ApiResult<ModuleMemorySummary> {
    state
        .memory_intelligence()
        .modules()
        .summary(module_id)
        .map(Json)
        .map_err(api_error)
}

/// Lists persisted Hermes learning cycles without triggering a new cycle.
pub async fn hermes_cycles(
    State(state): State<AppState>,
    Query(query): Query<HermesCyclesQuery>,
) -> ApiResult<Vec<HermesCycle>> {
    state
        .memory_intelligence()
        .hermes_cycles(bounded_limit(query.limit))
        .map(Json)
        .map_err(api_error)
}

fn bounded_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(20).clamp(1, 100)
}

fn default_content_type() -> String {
    "note".to_owned()
}

fn api_error(error: EduMindError) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": {"code": "memory_request_invalid", "message": error.to_string()},
        })),
    )
}

fn not_found(code: &str, message: &str) -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": {"code": code, "message": message}})),
    )
}
