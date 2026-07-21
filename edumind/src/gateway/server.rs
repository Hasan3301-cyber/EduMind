use std::{
    collections::BTreeMap,
    env,
    future::{Future, pending},
    net::IpAddr,
    sync::{Arc, RwLock},
    time::Instant,
};

use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use serde::Serialize;
use serde_json::json;
use tokio::{net::TcpListener, sync::broadcast, task::JoinHandle};
use tower_http::trace::TraceLayer;

use crate::{
    agent::CancellationRegistry,
    config::{
        EduMindConfig,
        types::{AuthMode, GatewayConfig},
    },
    gateway::{
        agent::{run as run_agent, status as agent_status},
        auth::{AuthError, AuthService},
        chat::complete as chat_completion,
        class_notes::export_notes as export_class_notes,
        collab::CollaborationService,
        group_study::{
            ask_ai as ask_group_study_ai, create_group as create_group_study,
            get_group as get_group_study, join_group as join_group_study,
            list_groups as list_group_study_groups, send_message as send_group_study_message,
            share_resource as share_group_study_resource,
        },
        memory::{
            graph_neighbors, hermes_cycles, list_memory, memory_get, memory_graph, memory_search,
            memory_snapshot, memory_store, module_memory_get, module_memory_search,
            module_memory_store, module_memory_summary, search_graph, search_wiki,
        },
        protocol::{
            ConnectParams, EventFrame, PROTOCOL_VERSION, ProtocolError, RequestFrame, ResponseFrame,
        },
        research::{
            add_project_note, add_project_question, ask_project, deep_ask_project, export_project,
            get_project as get_research_project, get_run as get_research_run, ingest_project,
            list_pipeline_plugins, list_project_documents, list_projects, literature_graph,
            project_gaps, project_supervise, project_synthesis,
            run_focused as run_focused_research, set_project_scope, validate_claims_handler,
        },
        runs::{cancel_run, evidence as run_evidence, timeline as run_timeline},
        runtime::{notebooklm_health, status as runtime_status},
        student_pages::{
            get_page as get_student_page, planner_schedule, save_page as save_student_page,
            upsert_record as upsert_student_page_record,
        },
        study::{
            create_card as create_srs_card, due_cards as due_srs_cards,
            generate_cards as generate_srs_cards, insights as study_insights,
            preview_card as preview_srs_card,
            refresh_recommendations as refresh_study_recommendations,
            review_card as review_srs_card, stats as srs_stats,
        },
        wellness::plan as wellness_plan,
    },
    infra::{EduMindError, LocalTelemetry, Result, TelemetryInput},
    memory::{HashEmbedder, HybridMemory, MemoryIntelligence, MemoryStore, TextEmbedder},
    research::{
        FullTextResearchService, FullTextStore, ProjectStore, ResearchPipelineEngine,
        ResearchProjectService, ResearchRunStore, ResearchSupervisorService,
        fulltext_database_path, project_database_path,
    },
    runs::RunStore,
    runtime_tools::RuntimeToolService,
    student::StudentPageStore,
    study::{LearningEngine, SrsService},
};

const BROADCAST_CAPACITY: usize = 128;
const AGENT_RUN_BODY_LIMIT_BYTES: usize = 6 * 1024 * 1024;

/// Shared state for the gateway HTTP, WebSocket, and future subsystem handlers.
#[derive(Clone)]
pub struct AppState {
    config: Arc<RwLock<EduMindConfig>>,
    pub auth: AuthService,
    telemetry: LocalTelemetry,
    broadcaster: Broadcaster,
    hybrid_memory: HybridMemory,
    memory_intelligence: MemoryIntelligence,
    research: ResearchPipelineEngine,
    project_assistant: ResearchProjectService,
    research_supervisor: ResearchSupervisorService,
    projects: ProjectStore,
    fulltext_assistant: FullTextResearchService,
    fulltext: FullTextStore,
    runtime_tools: RuntimeToolService,
    srs: SrsService,
    student_pages: StudentPageStore,
    learning: LearningEngine,
    run_store: RunStore,
    run_cancellations: CancellationRegistry,
    collaboration: CollaborationService,
}

impl AppState {
    /// Creates application state backed by the configured persistent memory database.
    pub fn new(config: EduMindConfig) -> Result<Self> {
        let memory = MemoryStore::open(&config.memory.db_path)?;
        let projects = ProjectStore::open(project_database_path(&config.memory.db_path))?;
        let fulltext = FullTextStore::open(fulltext_database_path(&config.memory.db_path))?;
        Self::with_research_stores(config, memory, projects, fulltext)
    }

    /// Creates isolated application state for tests and short-lived embedded sessions.
    pub fn in_memory(config: EduMindConfig) -> Result<Self> {
        Self::with_stores(
            config,
            MemoryStore::in_memory()?,
            ProjectStore::in_memory()?,
        )
    }

    /// Builds application state around a caller-provided memory store.
    pub fn with_memory_store(config: EduMindConfig, memory: MemoryStore) -> Result<Self> {
        Self::with_stores(config, memory, ProjectStore::in_memory()?)
    }

    /// Builds application state around caller-provided memory and research project stores.
    pub fn with_stores(
        config: EduMindConfig,
        memory: MemoryStore,
        projects: ProjectStore,
    ) -> Result<Self> {
        Self::with_research_stores(config, memory, projects, FullTextStore::in_memory()?)
    }

    /// Builds application state around caller-provided durable research project and full-text stores.
    pub fn with_research_stores(
        config: EduMindConfig,
        memory: MemoryStore,
        projects: ProjectStore,
        fulltext: FullTextStore,
    ) -> Result<Self> {
        config.validate()?;
        let telemetry = LocalTelemetry::new(memory.clone());
        let runtime_tools = RuntimeToolService::new(&config)?;
        let config = Arc::new(RwLock::new(config));
        let (embedding_dimensions, ocr_config, hermes_config) = {
            let config = config.read().map_err(|error| {
                EduMindError::Gateway(format!("configuration lock failed: {error}"))
            })?;
            (
                config.memory.embedding.dimensions,
                config.tools.ocr.clone(),
                config.memory.hermes.clone(),
            )
        };
        let embedder: Arc<dyn TextEmbedder> = Arc::new(HashEmbedder::new(embedding_dimensions)?);
        let hybrid_memory = HybridMemory::new(memory.clone(), Arc::clone(&embedder))?;
        let memory_intelligence = MemoryIntelligence::new(hybrid_memory.clone(), hermes_config)?;
        let run_store = RunStore::new(memory.clone());
        let run_cancellations = CancellationRegistry::default();
        let srs = SrsService::new(memory.clone());
        let student_pages = StudentPageStore::new(memory.clone());
        let learning = LearningEngine::new(srs.clone(), student_pages.clone());
        let project_assistant =
            ResearchProjectService::new(projects.clone(), Arc::clone(&embedder));
        let research_supervisor =
            ResearchSupervisorService::new(projects.clone(), fulltext.clone());
        let fulltext_assistant = FullTextResearchService::with_ocr_config(
            projects.clone(),
            fulltext.clone(),
            Arc::clone(&embedder),
            ocr_config,
        )?;
        let research =
            ResearchPipelineEngine::new(ResearchRunStore::new(memory.clone()), projects.clone())?;
        let broadcaster = Broadcaster::new(BROADCAST_CAPACITY);
        let collaboration =
            CollaborationService::with_broadcaster(memory.clone(), broadcaster.clone());
        Ok(Self {
            auth: AuthService::new(Arc::clone(&config)),
            config,
            telemetry,
            broadcaster,
            research,
            project_assistant,
            research_supervisor,
            projects,
            fulltext_assistant,
            fulltext,
            runtime_tools,
            srs,
            student_pages,
            learning,
            hybrid_memory,
            memory_intelligence,
            run_store,
            run_cancellations,
            collaboration,
        })
    }

    /// Returns a clone of the active configuration.
    pub fn config_snapshot(&self) -> Result<EduMindConfig> {
        self.config
            .read()
            .map(|config| config.clone())
            .map_err(|error| EduMindError::Gateway(format!("configuration lock failed: {error}")))
    }

    /// Replaces the active configuration after a successful validation pass.
    pub fn replace_config(&self, config: EduMindConfig) -> Result<()> {
        config.validate()?;
        *self.config.write().map_err(|error| {
            EduMindError::Gateway(format!("configuration lock failed: {error}"))
        })? = config;
        Ok(())
    }

    /// Broadcasts an event to every active WebSocket subscriber.
    pub fn publish(&self, event: EventFrame) {
        self.broadcaster.publish(event);
    }

    /// Subscribes to future gateway events.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<EventFrame> {
        self.broadcaster.subscribe()
    }

    /// Returns the local-only redacted operational telemetry store.
    #[must_use]
    pub fn telemetry(&self) -> LocalTelemetry {
        self.telemetry.clone()
    }

    /// Returns the durable spaced-repetition service.
    #[must_use]
    pub fn srs(&self) -> SrsService {
        self.srs.clone()
    }

    /// Returns the canonical Student OS and Planner state store.
    #[must_use]
    pub fn student_pages(&self) -> StudentPageStore {
        self.student_pages.clone()
    }

    /// Returns the local-only learning analytics engine.
    #[must_use]
    pub fn learning(&self) -> LearningEngine {
        self.learning.clone()
    }

    /// Returns durable storage for premium run recovery and evidence records.
    #[must_use]
    pub fn run_store(&self) -> RunStore {
        self.run_store.clone()
    }

    /// Returns the shared cancellation registry used by run-control endpoints.
    #[must_use]
    pub fn run_cancellations(&self) -> CancellationRegistry {
        self.run_cancellations.clone()
    }

    #[must_use]
    pub fn collaboration(&self) -> CollaborationService {
        self.collaboration.clone()
    }

    /// Returns hybrid memory used to index and retrieve canonical page snapshots.
    #[must_use]
    pub fn hybrid_memory(&self) -> HybridMemory {
        self.hybrid_memory.clone()
    }

    /// Returns local advanced search, graph, wiki, module-memory, and Hermes services.
    #[must_use]
    pub fn memory_intelligence(&self) -> MemoryIntelligence {
        self.memory_intelligence.clone()
    }

    /// Returns local artifact and optional external runtime integrations.
    #[must_use]
    pub fn runtime_tools(&self) -> RuntimeToolService {
        self.runtime_tools.clone()
    }

    /// Starts the optional Hermes background loop for the lifetime of a served gateway.
    #[must_use]
    pub fn start_hermes(&self) -> Option<JoinHandle<()>> {
        self.memory_intelligence.spawn_hermes()
    }

    /// Returns the durable focused research pipeline service.
    #[must_use]
    pub fn research_pipeline(&self) -> ResearchPipelineEngine {
        self.research.clone()
    }

    /// Returns the durable research-project repository.
    #[must_use]
    pub fn project_store(&self) -> ProjectStore {
        self.projects.clone()
    }

    /// Returns the project assistant for abstract-grounded Q&A, synthesis, and bibliography export.
    #[must_use]
    pub fn research_projects(&self) -> ResearchProjectService {
        self.project_assistant.clone()
    }

    /// Returns the deterministic supervisor for research gaps and next-step planning.
    #[must_use]
    pub fn research_supervisor(&self) -> ResearchSupervisorService {
        self.research_supervisor.clone()
    }

    /// Returns the full-text project assistant for PDF ingestion and passage-grounded Q&A.
    #[must_use]
    pub fn fulltext_projects(&self) -> FullTextResearchService {
        self.fulltext_assistant.clone()
    }

    /// Returns the durable full-text document and embedding store.
    #[must_use]
    pub fn fulltext_store(&self) -> FullTextStore {
        self.fulltext.clone()
    }
}

/// Lightweight broadcast fan-out for gateway events.
#[derive(Clone)]
pub struct Broadcaster {
    sender: broadcast::Sender<EventFrame>,
}

impl Broadcaster {
    /// Creates a broadcaster with a bounded per-subscriber event backlog.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publishes an event; a lack of subscribers is not an application error.
    pub fn publish(&self, event: EventFrame) {
        let _ = self.sender.send(event);
    }

    /// Subscribes to the broadcaster's future events.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<EventFrame> {
        self.sender.subscribe()
    }
}

/// Builds the loopback gateway router and applies tracing plus dynamic security middleware.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/ws", get(websocket))
        .route("/api/v1/chat/completions", post(chat_completion))
        .route(
            "/api/v1/agent/run",
            post(run_agent).layer(DefaultBodyLimit::max(AGENT_RUN_BODY_LIMIT_BYTES)),
        )
        .route("/api/v1/agents/status", get(agent_status))
        .route("/api/v1/runs/{run_id}/cancel", post(cancel_run))
        .route("/api/v1/runs/{run_id}/timeline", get(run_timeline))
        .route("/api/v1/runs/{run_id}/evidence", get(run_evidence))
        .route("/api/v1/study/srs/card", post(create_srs_card))
        .route("/api/v1/study/srs/generate", post(generate_srs_cards))
        .route("/api/v1/study/srs/due", post(due_srs_cards))
        .route("/api/v1/study/srs/preview", post(preview_srs_card))
        .route("/api/v1/study/srs/review", post(review_srs_card))
        .route("/api/v1/study/srs/stats", post(srs_stats))
        .route("/api/v1/study/insights", get(study_insights))
        .route(
            "/api/v1/study/recommendations/refresh",
            post(refresh_study_recommendations),
        )
        .route("/api/v1/student/page-state/get", post(get_student_page))
        .route("/api/v1/student/page-state/save", post(save_student_page))
        .route(
            "/api/v1/student/page-state/upsert",
            post(upsert_student_page_record),
        )
        .route("/api/v1/student/planner/schedule", get(planner_schedule))
        .route("/api/v1/wellness/plan", post(wellness_plan))
        .route("/api/v1/class-notes/export", post(export_class_notes))
        .route(
            "/api/v1/group-study/groups",
            get(list_group_study_groups).post(create_group_study),
        )
        .route("/api/v1/group-study/join", post(join_group_study))
        .route(
            "/api/v1/group-study/groups/{group_id}",
            get(get_group_study),
        )
        .route(
            "/api/v1/group-study/groups/{group_id}/messages",
            post(send_group_study_message),
        )
        .route(
            "/api/v1/group-study/groups/{group_id}/ai",
            post(ask_group_study_ai),
        )
        .route(
            "/api/v1/group-study/groups/{group_id}/resources",
            post(share_group_study_resource),
        )
        .route("/api/v1/memory/search", post(memory_search))
        .route("/api/v1/memory/list", post(list_memory))
        .route("/api/v1/memory/get", post(memory_get))
        .route("/api/v1/memory/store", post(memory_store))
        .route("/api/v1/memory/stats", get(memory_snapshot))
        .route("/api/v1/memory/graph", get(memory_graph))
        .route("/api/v1/memory/graph/search", post(search_graph))
        .route("/api/v1/memory/graph/neighbors", post(graph_neighbors))
        .route("/api/v1/memory/wiki/search", post(search_wiki))
        .route(
            "/api/v1/modules/{module_id}/memory/search",
            post(module_memory_search),
        )
        .route(
            "/api/v1/modules/{module_id}/memory/store",
            post(module_memory_store),
        )
        .route(
            "/api/v1/modules/{module_id}/memory/get",
            post(module_memory_get),
        )
        .route(
            "/api/v1/modules/{module_id}/memory/summary",
            post(module_memory_summary),
        )
        .route("/api/v1/hermes/cycles", get(hermes_cycles))
        .route("/api/v1/runtime/status", get(runtime_status))
        .route("/api/v1/runtime/notebooklm/health", get(notebooklm_health))
        .route("/api/v1/research/focused/run", post(run_focused_research))
        .route(
            "/api/v1/research/validate-claims",
            post(validate_claims_handler),
        )
        .route("/api/v1/research/literature-graph", post(literature_graph))
        .route(
            "/api/v1/research/pipeline/plugins",
            get(list_pipeline_plugins),
        )
        .route("/api/v1/research/runs/{run_id}", get(get_research_run))
        .route("/api/v1/research/projects", get(list_projects))
        .route(
            "/api/v1/research/projects/{project_id}",
            get(get_research_project),
        )
        .route(
            "/api/v1/research/projects/{project_id}/ask",
            post(ask_project),
        )
        .route(
            "/api/v1/research/projects/{project_id}/synthesis",
            post(project_synthesis),
        )
        .route(
            "/api/v1/research/projects/{project_id}/export",
            post(export_project),
        )
        .route(
            "/api/v1/research/projects/{project_id}/note",
            post(add_project_note),
        )
        .route(
            "/api/v1/research/projects/{project_id}/question",
            post(add_project_question),
        )
        .route(
            "/api/v1/research/projects/{project_id}/scope",
            post(set_project_scope),
        )
        .route(
            "/api/v1/research/projects/{project_id}/ingest",
            post(ingest_project),
        )
        .route(
            "/api/v1/research/projects/{project_id}/documents",
            get(list_project_documents),
        )
        .route(
            "/api/v1/research/projects/{project_id}/deep-ask",
            post(deep_ask_project),
        )
        .route(
            "/api/v1/research/projects/{project_id}/gaps",
            post(project_gaps),
        )
        .route(
            "/api/v1/research/projects/{project_id}/supervise",
            post(project_supervise),
        )
        .route("/study/srs/card", post(create_srs_card))
        .route("/study/srs/generate", post(generate_srs_cards))
        .route("/study/srs/due", post(due_srs_cards))
        .route("/study/srs/preview", post(preview_srs_card))
        .route("/study/srs/review", post(review_srs_card))
        .route("/study/srs/stats", post(srs_stats))
        .route("/student/page-state/get", post(get_student_page))
        .route("/student/page-state/save", post(save_student_page))
        .route(
            "/student/page-state/upsert",
            post(upsert_student_page_record),
        )
        .route("/student/planner/schedule", get(planner_schedule))
        .route("/wellness/plan", post(wellness_plan))
        .route("/class-notes/export", post(export_class_notes))
        .route("/memory/search", post(memory_search))
        .route("/memory/list", post(list_memory))
        .route("/memory/get", post(memory_get))
        .route("/memory/store", post(memory_store))
        .route("/memory/stats", get(memory_snapshot))
        .route("/memory/graph", get(memory_graph))
        .route("/memory/graph/search", post(search_graph))
        .route("/memory/graph/neighbors", post(graph_neighbors))
        .route("/memory/wiki/search", post(search_wiki))
        .route(
            "/modules/{module_id}/memory/search",
            post(module_memory_search),
        )
        .route(
            "/modules/{module_id}/memory/store",
            post(module_memory_store),
        )
        .route("/modules/{module_id}/memory/get", post(module_memory_get))
        .route(
            "/modules/{module_id}/memory/summary",
            post(module_memory_summary),
        )
        .route("/hermes/cycles", get(hermes_cycles))
        .route("/runtime/status", get(runtime_status))
        .route("/runtime/notebooklm/health", get(notebooklm_health))
        .route("/research/focused/run", post(run_focused_research))
        .route("/research/validate-claims", post(validate_claims_handler))
        .route("/research/literature-graph", post(literature_graph))
        .route("/research/pipeline/plugins", get(list_pipeline_plugins))
        .route("/research/runs/{run_id}", get(get_research_run))
        .route("/research/projects", get(list_projects))
        .route("/research/projects/{project_id}", get(get_research_project))
        .route("/research/projects/{project_id}/ask", post(ask_project))
        .route(
            "/research/projects/{project_id}/synthesis",
            post(project_synthesis),
        )
        .route(
            "/research/projects/{project_id}/export",
            post(export_project),
        )
        .route(
            "/research/projects/{project_id}/note",
            post(add_project_note),
        )
        .route(
            "/research/projects/{project_id}/question",
            post(add_project_question),
        )
        .route(
            "/research/projects/{project_id}/scope",
            post(set_project_scope),
        )
        .route(
            "/research/projects/{project_id}/ingest",
            post(ingest_project),
        )
        .route(
            "/research/projects/{project_id}/documents",
            get(list_project_documents),
        )
        .route(
            "/research/projects/{project_id}/deep-ask",
            post(deep_ask_project),
        )
        .route("/research/projects/{project_id}/gaps", post(project_gaps))
        .route(
            "/research/projects/{project_id}/supervise",
            post(project_supervise),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            request_telemetry_middleware,
        ))
        .with_state(state)
}

/// Validates the binding policy and opens the configured TCP listener.
pub async fn bind_listener(config: &GatewayConfig) -> Result<TcpListener> {
    ensure_secure_bind(config)?;
    let address = format_bind_address(&config.bind_address, config.port);
    TcpListener::bind(&address)
        .await
        .map_err(|error| EduMindError::Gateway(format!("failed to bind {address}: {error}")))
}

/// Serves the gateway until the listener is closed or the task is cancelled.
pub async fn serve(listener: TcpListener, state: AppState) -> Result<()> {
    serve_with_shutdown(listener, state, pending()).await
}

/// Serves the gateway until a caller-owned shutdown future resolves.
pub async fn serve_with_shutdown<F>(
    listener: TcpListener,
    state: AppState,
    shutdown: F,
) -> Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let hermes_task = state.start_hermes();
    let result = axum::serve(listener, build_router(state))
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|error| EduMindError::Gateway(format!("gateway server failed: {error}")));
    if let Some(task) = hermes_task {
        task.abort();
    }
    result
}

/// Rejects unauthenticated non-loopback bindings unless the explicit environment override is set.
pub fn ensure_secure_bind(config: &GatewayConfig) -> Result<()> {
    let override_enabled = env::var("EDUMIND_ALLOW_INSECURE_BIND").is_ok_and(|value| value == "1");
    ensure_secure_bind_with_override(config, override_enabled)
}

/// Detects common IPv4, IPv6, and hostname forms of a loopback bind address.
#[must_use]
pub fn bind_is_loopback(bind_address: &str) -> bool {
    let normalized = bind_address.trim().trim_matches(['[', ']']);
    normalized.eq_ignore_ascii_case("localhost")
        || normalized
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn ensure_secure_bind_with_override(config: &GatewayConfig, override_enabled: bool) -> Result<()> {
    if config.auth.mode == AuthMode::None
        && !bind_is_loopback(&config.bind_address)
        && !override_enabled
    {
        return Err(EduMindError::Gateway(
            "refusing non-loopback bind when gateway.auth.mode=none; set EDUMIND_ALLOW_INSECURE_BIND=1 to override"
                .to_owned(),
        ));
    }
    Ok(())
}

async fn request_telemetry_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let module_id = telemetry_module_id(request.uri().path());
    let started = Instant::now();
    let response = next.run(request).await;
    let status = response.status();
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let outcome = if status.is_server_error() {
        "server_error"
    } else if status.is_client_error() {
        "client_error"
    } else {
        "ok"
    };
    let metadata = BTreeMap::from([
        ("duration_ms".to_owned(), duration_ms.to_string()),
        ("status_code".to_owned(), status.as_u16().to_string()),
    ]);
    let _ = state.telemetry.record(
        TelemetryInput {
            event_name: "gateway.request".to_owned(),
            module_id: module_id.to_owned(),
            outcome: outcome.to_owned(),
            metadata,
        },
        Utc::now(),
    );
    response
}

fn telemetry_module_id(path: &str) -> &'static str {
    let path = path.strip_prefix("/api/v1").unwrap_or(path);
    if path.starts_with("/agent") || path.starts_with("/agents") {
        "agent"
    } else if path.starts_with("/chat") {
        "chat"
    } else if path.starts_with("/runs") {
        "runs"
    } else if path.starts_with("/study") {
        "study-review"
    } else if path.starts_with("/student") {
        "student-os"
    } else if path.starts_with("/wellness") {
        "wellness"
    } else if path.starts_with("/class-notes") {
        "class-notes"
    } else if path.starts_with("/group-study") {
        "group-study"
    } else if path.starts_with("/modules") {
        "module-memory"
    } else if path.starts_with("/memory") || path.starts_with("/hermes") {
        "memory"
    } else if path.starts_with("/runtime") {
        "runtime"
    } else if path.starts_with("/research") {
        "research"
    } else {
        "gateway"
    }
}

async fn security_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let origin = match state
        .auth
        .validate_origin(request.headers().get(header::ORIGIN))
    {
        Ok(origin) => origin,
        Err(error) => {
            return protocol_error_response(
                StatusCode::FORBIDDEN,
                "origin_not_allowed",
                error.to_string(),
            );
        }
    };

    if request.method() == Method::OPTIONS {
        return apply_cors(StatusCode::NO_CONTENT.into_response(), origin);
    }

    let response = if is_public_path(request.uri().path()) {
        next.run(request).await
    } else {
        match state.auth.authenticate_headers(request.headers()) {
            Ok(principal) => {
                request.extensions_mut().insert(principal);
                next.run(request).await
            }
            Err(error) => auth_error_response(error),
        }
    };
    apply_cors(response, origin)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        protocol_version: PROTOCOL_VERSION,
    })
}

async fn ready(State(state): State<AppState>) -> Response {
    match state.config_snapshot().and_then(|config| config.validate()) {
        Ok(()) => Json(ReadyResponse {
            ready: true,
            reason: None,
        })
        .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadyResponse {
                ready: false,
                reason: Some(error.to_string()),
            }),
        )
            .into_response(),
    }
}

async fn websocket(State(state): State<AppState>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let connect = match socket.recv().await {
        Some(Ok(Message::Text(text))) => match serde_json::from_str::<ConnectParams>(text.as_str())
        {
            Ok(params) => params,
            Err(_) => {
                let _ = send_json(
                    &mut socket,
                    &ResponseFrame::failure(
                        "_connect",
                        ProtocolError::new("invalid_connect", "Invalid WebSocket connect payload."),
                    ),
                )
                .await;
                return;
            }
        },
        _ => return,
    };

    if connect.protocol_version != PROTOCOL_VERSION {
        let _ = send_json(
            &mut socket,
            &ResponseFrame::failure(
                "_connect",
                ProtocolError::new("protocol_version_mismatch", "Unsupported protocol version."),
            ),
        )
        .await;
        return;
    }

    let principal = match state.auth.authenticate(connect.token.as_deref()) {
        Ok(principal) => principal,
        Err(error) => {
            let _ = send_json(
                &mut socket,
                &ResponseFrame::failure(
                    "_connect",
                    ProtocolError::new("unauthorized", error.to_string()),
                ),
            )
            .await;
            return;
        }
    };

    if !send_json(
        &mut socket,
        &EventFrame::new(
            "connected",
            json!({"subject": principal.subject, "role": principal.role}),
        ),
    )
    .await
    {
        return;
    }

    let mut events = state.subscribe();
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(incoming) = incoming else { break; };
                let Ok(message) = incoming else { break; };
                match message {
                    Message::Text(text) => {
                        let response = match serde_json::from_str::<RequestFrame>(text.as_str()) {
                            Ok(request) => dispatch_request(request),
                            Err(_) => ResponseFrame::failure(
                                "_invalid",
                                ProtocolError::new("invalid_request", "Invalid request frame."),
                            ),
                        };
                        if !send_json(&mut socket, &response).await {
                            break;
                        }
                    }
                    Message::Ping(payload) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    Message::Binary(_) | Message::Pong(_) => {}
                }
            }
            event = events.recv() => {
                match event {
                    Ok(event) => {
                        if !send_json(&mut socket, &event).await {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        if !send_json(&mut socket, &EventFrame::new("events_lagged", json!({"skipped": skipped}))).await {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

fn dispatch_request(request: RequestFrame) -> ResponseFrame {
    match request.method.as_str() {
        "health" => ResponseFrame::success(
            request.id,
            json!({"status": "ok", "protocol_version": PROTOCOL_VERSION}),
        ),
        "ping" => ResponseFrame::success(request.id, request.params),
        _ => ResponseFrame::failure(
            request.id,
            ProtocolError::new("unknown_method", "Requested method is not supported."),
        ),
    }
}

async fn send_json<T: Serialize>(socket: &mut WebSocket, value: &T) -> bool {
    match serde_json::to_string(value) {
        Ok(text) => socket.send(Message::Text(text.into())).await.is_ok(),
        Err(_) => false,
    }
}

fn is_public_path(path: &str) -> bool {
    matches!(path, "/health" | "/ready" | "/ws")
}

fn protocol_error_response(status: StatusCode, code: &str, message: String) -> Response {
    (
        status,
        Json(json!({"error": ProtocolError::new(code, message)})),
    )
        .into_response()
}

fn auth_error_response(error: AuthError) -> Response {
    let status = match error {
        AuthError::OriginNotAllowed => StatusCode::FORBIDDEN,
        AuthError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        AuthError::InvalidAuthorizationHeader
        | AuthError::InvalidCredentials
        | AuthError::Required => StatusCode::UNAUTHORIZED,
    };
    protocol_error_response(status, "unauthorized", error.to_string())
}

fn apply_cors(mut response: Response, origin: Option<HeaderValue>) -> Response {
    let Some(origin) = origin else {
        return response;
    };
    let headers = response.headers_mut();
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("authorization, content-type"),
    );
    headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    response
}

fn format_bind_address(host: &str, port: u16) -> String {
    let host = host.trim();
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    protocol_version: u16,
}

#[derive(Serialize)]
struct ReadyResponse {
    ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use chrono::TimeZone;
    use serde_json::json;
    use tower::ServiceExt;

    use super::{
        AppState, Broadcaster, bind_is_loopback, build_router, ensure_secure_bind_with_override,
    };
    use crate::{
        config::{EduMindConfig, types::AuthMode},
        gateway::EventFrame,
    };

    #[test]
    fn detects_loopback_bind_addresses() {
        assert!(bind_is_loopback("localhost"));
        assert!(bind_is_loopback("127.0.0.1"));
        assert!(bind_is_loopback("[::1]"));
        assert!(!bind_is_loopback("0.0.0.0"));
    }

    #[test]
    fn rejects_unauthenticated_non_loopback_binds() {
        let mut config = EduMindConfig::default();
        config.gateway.bind_address = "0.0.0.0".to_owned();

        assert!(ensure_secure_bind_with_override(&config.gateway, false).is_err());
        assert!(ensure_secure_bind_with_override(&config.gateway, true).is_ok());
    }

    #[tokio::test]
    async fn protects_non_public_routes_when_token_auth_is_enabled() {
        let mut config = EduMindConfig::default();
        config.gateway.auth.mode = AuthMode::Token;
        config.gateway.auth.token = Some("gateway-token".to_owned());
        let app = build_router(AppState::in_memory(config).unwrap());

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let authorized = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/config")
                    .header(header::AUTHORIZATION, "Bearer gateway-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(authorized.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn permits_only_configured_origins_and_adds_cors_headers() {
        let app = build_router(AppState::in_memory(EduMindConfig::default()).unwrap());
        let allowed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header(header::ORIGIN, "http://localhost")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let blocked = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header(header::ORIGIN, "https://untrusted.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(allowed.status(), StatusCode::OK);
        assert_eq!(
            allowed.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            "http://localhost"
        );
        assert_eq!(blocked.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn broadcaster_delivers_events_to_subscribers() {
        let broadcaster = Broadcaster::new(4);
        let mut receiver = broadcaster.subscribe();
        broadcaster.publish(EventFrame::new("config_reloaded", json!({"version": 2})));

        assert_eq!(receiver.recv().await.unwrap().event, "config_reloaded");
    }

    #[tokio::test]
    async fn request_telemetry_keeps_only_coarse_redacted_operational_data() {
        let state = AppState::in_memory(EduMindConfig::default()).unwrap();
        let app = build_router(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health?student_id=sensitive-student")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let events = state.telemetry().recent(1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_name, "gateway.request");
        assert_eq!(events[0].module_id, "gateway");
        assert_eq!(events[0].outcome, "ok");
        assert_eq!(events[0].metadata.get("status_code").unwrap(), "200");
        assert!(events[0].metadata.contains_key("duration_ms"));
        let serialized = serde_json::to_string(&events[0]).unwrap();
        assert!(!serialized.contains("sensitive-student"));
        assert!(!serialized.contains("student_id"));
        assert!(!serialized.contains("/health"));
    }

    #[tokio::test]
    async fn study_and_student_page_endpoints_persist_canonical_state() {
        let app = build_router(AppState::in_memory(EduMindConfig::default()).unwrap());
        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/study/srs/card")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"front": "Derivative", "back": "Rate of change"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::OK);

        let saved = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/student/page-state/save")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "page": "planner",
                            "source": "desktop",
                            "updated_at": "2026-07-15T13:00:00Z",
                            "records": [{
                                "key": "monday",
                                "value": {
                                    "kind": "schedule-block",
                                    "day": "Monday",
                                    "title": "Calculus",
                                    "start": "09:00",
                                    "end": "10:00"
                                },
                                "updated_at": "2026-07-15T13:00:00Z"
                            }]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(saved.status(), StatusCode::OK);

        let schedule = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/student/planner/schedule")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(schedule.status(), StatusCode::OK);
        let body = to_bytes(schedule.into_body(), 1_048_576).await.unwrap();
        let schedule: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(schedule["days"][0]["day"], "Monday");
        assert_eq!(schedule["days"][0]["entries"][0]["title"], "Calculus");

        let loaded = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/student/page-state/get")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"page": "student_planner"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(loaded.status(), StatusCode::OK);
        let body = to_bytes(loaded.into_body(), 1_048_576).await.unwrap();
        let snapshot: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(snapshot["page"], "student-planner");
        assert_eq!(snapshot["count"], 1);
    }

    #[tokio::test]
    async fn research_endpoints_run_seeded_pipeline_and_expose_persisted_results() {
        let app = build_router(AppState::in_memory(EduMindConfig::default()).unwrap());
        let plugins = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/research/pipeline/plugins")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(plugins.status(), StatusCode::OK);

        let run = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/research/focused/run")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "topic": "retrieval fairness",
                            "sources": ["manual"],
                            "seed_papers": [{
                                "id": "paper-a",
                                "title": "Retrieval practice for students",
                                "abstract_text": "The study reports retrieval supports student learning.",
                                "year": 2026,
                                "citation_count": 8,
                                "keywords": ["retrieval", "education"]
                            }],
                            "claims": ["Retrieval supports student learning [paper-a]"]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(run.status(), StatusCode::OK);
        let body = to_bytes(run.into_body(), 1_048_576).await.unwrap();
        let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(result["run"]["status"], "completed");
        assert_eq!(result["context"]["papers"].as_array().unwrap().len(), 1);
        let run_id = result["run"]["id"].as_str().unwrap();

        let persisted = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/research/runs/{run_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(persisted.status(), StatusCode::OK);

        let validation = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/research/validate-claims")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "claims": ["Retrieval helps learning"],
                            "evidence": [{
                                "id": "paper-a",
                                "title": "Retrieval study",
                                "text": "Retrieval helps student learning."
                            }]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(validation.status(), StatusCode::OK);

        let graph = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/research/literature-graph")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "papers": [{
                                "id": "paper-a",
                                "title": "Retrieval practice",
                                "keywords": ["retrieval"]
                            }]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(graph.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn research_project_endpoints_manage_an_accumulated_project() {
        let app = build_router(AppState::in_memory(EduMindConfig::default()).unwrap());
        let run = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/research/focused/run")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "topic": "retrieval practice project",
                            "sources": ["manual"],
                            "seed_papers": [{
                                "id": "paper-retrieval",
                                "title": "Retrieval practice improves learning",
                                "abstract_text": "Retrieval practice improves durable learning outcomes for students.",
                                "authors": ["Taylor Researcher"],
                                "year": 2026,
                                "venue": "Learning Sciences",
                                "citation_count": 18,
                                "keywords": ["retrieval", "learning"]
                            }]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(run.status(), StatusCode::OK);
        let run_body = to_bytes(run.into_body(), 1_048_576).await.unwrap();
        let run_result: serde_json::Value = serde_json::from_slice(&run_body).unwrap();
        let project_id = run_result["project"]["id"].as_str().unwrap().to_owned();
        assert_eq!(run_result["project"]["papers"].as_array().unwrap().len(), 1);

        let projects = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/research/projects")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(projects.status(), StatusCode::OK);
        let projects_body = to_bytes(projects.into_body(), 1_048_576).await.unwrap();
        let projects: serde_json::Value = serde_json::from_slice(&projects_body).unwrap();
        assert_eq!(projects.as_array().unwrap().len(), 1);

        let note = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/research/projects/{project_id}/note"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"content": "Prioritize classroom evidence."}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(note.status(), StatusCode::OK);

        let question = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/research/projects/{project_id}/question"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"question": "How does retrieval practice affect learning?"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(question.status(), StatusCode::OK);

        let scope = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/research/projects/{project_id}/scope"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"scope": "K-12 classrooms"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(scope.status(), StatusCode::OK);

        let answer = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/research/projects/{project_id}/ask"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"question": "What learning outcome is reported?"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(answer.status(), StatusCode::OK);
        let answer_body = to_bytes(answer.into_body(), 1_048_576).await.unwrap();
        let answer: serde_json::Value = serde_json::from_slice(&answer_body).unwrap();
        assert_eq!(answer["sources"].as_array().unwrap().len(), 1);

        let synthesis = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/research/projects/{project_id}/synthesis"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(synthesis.status(), StatusCode::OK);
        let synthesis_body = to_bytes(synthesis.into_body(), 1_048_576).await.unwrap();
        let synthesis: serde_json::Value = serde_json::from_slice(&synthesis_body).unwrap();
        assert_eq!(synthesis["comparison_matrix"].as_array().unwrap().len(), 1);

        let export = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/research/projects/{project_id}/export"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"format": "bibtex"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(export.status(), StatusCode::OK);
        let export_body = to_bytes(export.into_body(), 1_048_576).await.unwrap();
        let export: serde_json::Value = serde_json::from_slice(&export_body).unwrap();
        assert!(export["content"].as_str().unwrap().contains("@article"));

        let project = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/research/projects/{project_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(project.status(), StatusCode::OK);
        let project_body = to_bytes(project.into_body(), 1_048_576).await.unwrap();
        let project: serde_json::Value = serde_json::from_slice(&project_body).unwrap();
        assert_eq!(project["scope"], "K-12 classrooms");
        assert_eq!(project["questions"].as_array().unwrap().len(), 1);
        assert_eq!(project["notes"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn research_fulltext_endpoints_ingest_list_and_deep_ask() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();
        let state = AppState::in_memory(EduMindConfig::default()).unwrap();
        let mut project = edumind_core::ResearchProject::new("retrieval project", now);
        project.add_papers(
            vec![edumind_core::PaperMetadata {
                id: "retrieval-paper".to_owned(),
                title: "Retrieval practice trial".to_owned(),
                ..edumind_core::PaperMetadata::default()
            }],
            now,
        );
        state.project_store().save(&project).unwrap();
        let path =
            std::env::temp_dir().join(format!("edumind-gateway-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(
            &path,
            minimal_pdf(
                "Retrieval practice improved long-term retention for students. This study is limited by a small sample.",
            ),
        )
        .unwrap();
        let app = build_router(state);

        let ingest = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/research/projects/{}/ingest", project.id))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "paper_id": "retrieval-paper",
                            "source": path.to_string_lossy(),
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(ingest.status(), StatusCode::OK);
        let ingest_body = to_bytes(ingest.into_body(), 1_048_576).await.unwrap();
        let ingest: serde_json::Value = serde_json::from_slice(&ingest_body).unwrap();
        assert_eq!(ingest["paper_id"], "retrieval-paper");
        assert_eq!(ingest["ocr"], false);
        assert!(ingest["chunk_count"].as_u64().unwrap() >= 1);

        let documents = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/research/projects/{}/documents", project.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(documents.status(), StatusCode::OK);
        let documents_body = to_bytes(documents.into_body(), 1_048_576).await.unwrap();
        let documents: serde_json::Value = serde_json::from_slice(&documents_body).unwrap();
        assert_eq!(documents.as_array().unwrap().len(), 1);
        assert!(documents[0].get("full_text").is_none());

        let gaps = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/research/projects/{}/gaps", project.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(gaps.status(), StatusCode::OK);
        let gaps_body = to_bytes(gaps.into_body(), 1_048_576).await.unwrap();
        let gaps: serde_json::Value = serde_json::from_slice(&gaps_body).unwrap();
        assert_eq!(gaps["stated_gaps"][0]["kind"], "limitation");

        let supervision = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/research/projects/{}/supervise",
                        project.id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(supervision.status(), StatusCode::OK);
        let supervision_body = to_bytes(supervision.into_body(), 1_048_576).await.unwrap();
        let supervision: serde_json::Value = serde_json::from_slice(&supervision_body).unwrap();
        assert_eq!(
            supervision["reading_plan"][0]["paper_id"],
            "retrieval-paper"
        );
        assert_eq!(supervision["stated_gaps"][0]["kind"], "limitation");

        let answer = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/research/projects/{}/deep-ask", project.id))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"question": "What outcome did retrieval practice improve?"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(answer.status(), StatusCode::OK);
        let answer_body = to_bytes(answer.into_body(), 1_048_576).await.unwrap();
        let answer: serde_json::Value = serde_json::from_slice(&answer_body).unwrap();
        assert_eq!(answer["passages"][0]["paper_id"], "retrieval-paper");
        assert!(
            answer["answer"]
                .as_str()
                .unwrap()
                .contains("[retrieval-paper#0]")
        );
    }

    #[tokio::test]
    async fn memory_intelligence_endpoints_store_and_query_local_views() {
        let app = build_router(AppState::in_memory(EduMindConfig::default()).unwrap());
        let stored = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/memory/store")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "module_id": "class-notes",
                            "content": "Calculus limits revision improves derivative readiness.",
                            "content_type": "note",
                            "metadata": {"skill": "calculus", "success": true},
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stored.status(), StatusCode::OK);
        let stored_body = to_bytes(stored.into_body(), 1_048_576).await.unwrap();
        let stored: serde_json::Value = serde_json::from_slice(&stored_body).unwrap();
        let memory_id = stored["record"]["id"].as_str().unwrap().to_owned();
        assert!(stored["graph_node_count"].as_u64().unwrap() >= 2);
        assert!(stored["wiki_page_count"].as_u64().unwrap() >= 1);

        let search = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/memory/search")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"query": "calculus"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(search.status(), StatusCode::OK);
        let search_body = to_bytes(search.into_body(), 1_048_576).await.unwrap();
        let search: serde_json::Value = serde_json::from_slice(&search_body).unwrap();
        assert_eq!(search[0]["record"]["id"], memory_id);

        let module_get = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/modules/class-notes/memory/get")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"id": memory_id}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(module_get.status(), StatusCode::OK);

        let graph = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/memory/graph")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(graph.status(), StatusCode::OK);
        let graph_body = to_bytes(graph.into_body(), 1_048_576).await.unwrap();
        let graph: serde_json::Value = serde_json::from_slice(&graph_body).unwrap();
        assert!(graph["nodes"].as_array().unwrap().len() >= 2);

        let wiki = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/memory/wiki/search")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"query": "calculus"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(wiki.status(), StatusCode::OK);

        let stats = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/memory/stats")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stats.status(), StatusCode::OK);

        let hermes = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/hermes/cycles")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(hermes.status(), StatusCode::OK);
        let hermes_body = to_bytes(hermes.into_body(), 1_048_576).await.unwrap();
        let hermes: serde_json::Value = serde_json::from_slice(&hermes_body).unwrap();
        assert!(hermes.as_array().unwrap().is_empty());
    }

    fn minimal_pdf(text: &str) -> Vec<u8> {
        let stream = format!("BT\n/F1 18 Tf\n72 720 Td\n({text}) Tj\nET");
        let objects = vec![
            "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_owned(),
            "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n".to_owned(),
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>\nendobj\n".to_owned(),
            "4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n".to_owned(),
            format!(
                "5 0 obj\n<< /Length {} >>\nstream\n{stream}\nendstream\nendobj\n",
                stream.len()
            ),
        ];
        let mut bytes = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for object in objects {
            offsets.push(bytes.len());
            bytes.extend_from_slice(object.as_bytes());
        }
        let xref_offset = bytes.len();
        bytes.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
        for offset in offsets {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n")
                .as_bytes(),
        );
        bytes
    }
}
