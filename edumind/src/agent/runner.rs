use std::{
    fmt,
    path::Path,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use chrono::Utc;
use edumind_core::{
    ClaimValidationRequest, LiteratureGraphRequest, ResearchProjectId, ResearchRequest,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    config::{EduMindConfig, types::ExecutionCapsConfig},
    infra::{EduMindError, Result},
    memory::{HybridMemory, MemoryId, MemoryIntelligence, ModuleMemoryScope, NewModuleMemory},
    research::{
        FullTextIngestRequest, FullTextResearchService, OcrMode, ResearchPipelineEngine,
        ResearchProjectService, ResearchSupervisorService, build_literature_graph, validate_claims,
    },
    runtime_tools::RuntimeToolService,
    security::{ActionGrant, ContentGuard, ToolDailyLimitDecision, ToolDailyLimiter, WriteSandbox},
    student::{StudentPageRecordInput, StudentPageStore},
    study::{NewSrsCard, SrsCardId, SrsService},
};

use super::{
    AgentProfile, AgentRegistry, AgentRunLimiter, ChatRole, ModelResolver, ResolvedModel,
    SessionManager, SubagentRegistry, ToolAuditEntry, ToolAuditLog, ToolAuditOutcome, ToolCall,
    ToolDef, ToolPolicy, ToolPolicyDecision, ToolRateLimiter, ToolRegistry,
};

/// A validated image included only with the active model turn.
#[derive(Clone, Eq, PartialEq)]
pub struct TransientImage {
    mime_type: String,
    data_url: String,
}

impl TransientImage {
    /// Creates a transient image after gateway validation.
    #[must_use]
    pub fn new(mime_type: impl Into<String>, data_url: impl Into<String>) -> Self {
        Self {
            mime_type: mime_type.into(),
            data_url: data_url.into(),
        }
    }

    /// Returns the validated MIME type without exposing image bytes.
    #[must_use]
    pub fn mime_type(&self) -> &str {
        &self.mime_type
    }

    /// Returns the data URL for the immediate provider request only.
    #[must_use]
    pub fn data_url(&self) -> &str {
        &self.data_url
    }
}

impl fmt::Debug for TransientImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransientImage")
            .field("mime_type", &self.mime_type)
            .field("encoded_len", &self.data_url.len())
            .finish()
    }
}

/// Input accepted by the chat run loop for one user turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentRunRequest {
    pub session_key: String,
    pub agent_id: Option<String>,
    pub requested_model: Option<String>,
    pub action_grant: Option<ActionGrant>,
    pub input: String,
    pub untrusted_content: bool,
    pub transient_image: Option<TransientImage>,
    pub spawn_depth: u8,
}

impl AgentRunRequest {
    /// Creates a request targeting the configured default agent.
    #[must_use]
    pub fn new(session_key: impl Into<String>, input: impl Into<String>) -> Self {
        Self {
            session_key: session_key.into(),
            agent_id: None,
            requested_model: None,
            action_grant: None,
            input: input.into(),
            untrusted_content: false,
            transient_image: None,
            spawn_depth: 0,
        }
    }

    /// Marks the input as document-derived content that must pass injection checks.
    #[must_use]
    pub fn with_untrusted_content(mut self) -> Self {
        self.untrusted_content = true;
        self
    }

    /// Includes a validated image for this provider turn without persisting it in the session.
    #[must_use]
    pub fn with_transient_image(mut self, image: TransientImage) -> Self {
        self.transient_image = Some(image);
        self
    }
}

/// Context delivered to a model for one deterministic turn iteration.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelRequest {
    pub agent: AgentProfile,
    pub model: ResolvedModel,
    pub messages: Vec<super::SessionMessage>,
    pub tools: Vec<ToolDef>,
    pub transient_image: Option<TransientImage>,
}

/// A model response is either a final answer or exactly one requested tool call.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelResponse {
    Final(String),
    ToolCall(ToolCall),
}

/// Adapter implemented by local or remote chat model providers.
#[async_trait]
pub trait AgentModel: Send + Sync {
    /// Completes one model iteration using only the provided context and tool definitions.
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse>;
}

/// Context passed to an authorized tool executor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolExecutionContext {
    pub agent_id: String,
    pub session_key: String,
    pub spawn_depth: u8,
    pub write_sandbox: WriteSandbox,
}

struct ToolCallScope<'a> {
    agent: &'a AgentProfile,
    session_key: &'a str,
    spawn_depth: u8,
    action_grant: Option<&'a ActionGrant>,
}

/// Executor for an authorized, rate-limited tool invocation.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Executes a previously validated tool call and returns a JSON observation.
    async fn execute(&self, context: &ToolExecutionContext, call: &ToolCall) -> Result<Value>;
}

/// Tool output retained in a completed agent turn.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolExecution {
    pub call_id: String,
    pub tool_name: String,
    pub output: Value,
}

/// Result of a completed model/chat run loop.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentRunResult {
    pub agent_id: String,
    pub session_key: String,
    pub model: ResolvedModel,
    pub content: String,
    pub tool_executions: Vec<ToolExecution>,
}

/// Bounded orchestrator that enforces policy, rate limits, audit, and session persistence.
#[derive(Clone)]
pub struct AgentRunner {
    agents: AgentRegistry,
    models: ModelResolver,
    sessions: SessionManager,
    tools: ToolRegistry,
    policy: ToolPolicy,
    audit: ToolAuditLog,
    rate_limiter: ToolRateLimiter,
    daily_limiter: ToolDailyLimiter,
    run_limiter: AgentRunLimiter,
    action_password_required: bool,
    execution_caps: ExecutionCapsConfig,
    write_sandbox: WriteSandbox,
    content_guard: ContentGuard,
    max_tool_rounds: u16,
}

impl AgentRunner {
    /// Builds a runtime using the active configuration and a durable session manager.
    pub fn new(config: &EduMindConfig, sessions: SessionManager) -> Result<Self> {
        config.validate()?;
        let agents = AgentRegistry::from_config(config)?;
        Ok(Self {
            run_limiter: AgentRunLimiter::new(agents.clone()),
            agents,
            models: ModelResolver::from_config(config)?,
            sessions,
            tools: ToolRegistry::standard(),
            policy: ToolPolicy::from_config(&config.tools),
            audit: ToolAuditLog::new(config.tools.audit_log_capacity)?,
            rate_limiter: ToolRateLimiter::new(config.tools.rate_limit_per_minute)?,
            daily_limiter: ToolDailyLimiter::new(config.security.tool_daily_limits.clone()),
            action_password_required: config.security.action_password_required,
            execution_caps: config.security.execution.clone(),
            write_sandbox: WriteSandbox::from_config(&config.security),
            content_guard: ContentGuard,
            max_tool_rounds: config.tools.max_tool_rounds,
        })
    }

    /// Runs a chat turn until the model returns a final answer or reaches its tool-call bound.
    pub async fn run_turn(
        &self,
        request: AgentRunRequest,
        model_client: &dyn AgentModel,
        tool_executor: &dyn ToolExecutor,
    ) -> Result<AgentRunResult> {
        if request.session_key.trim().is_empty() || request.input.trim().is_empty() {
            return Err(EduMindError::Agent(
                "agent runs require non-empty session_key and input".to_owned(),
            ));
        }
        if request.untrusted_content {
            self.content_guard.require_safe(&request.input)?;
        }
        let agent = self.agents.resolve(request.agent_id.as_deref())?;
        let _run_permit = self.run_limiter.try_acquire(&agent)?;
        let model = self
            .models
            .resolve(&agent, request.requested_model.as_deref())?;
        let now = Utc::now();
        self.sessions
            .get_or_create(&request.session_key, &agent.id, now)?;
        self.sessions
            .append_message(&request.session_key, ChatRole::User, &request.input, now)?;

        let mut tool_executions = Vec::new();
        for _ in 0..usize::from(self.max_tool_rounds) {
            let model_request = ModelRequest {
                agent: agent.clone(),
                model: model.clone(),
                messages: self.sessions.context_for_model(
                    &request.session_key,
                    model.context_window,
                    Utc::now(),
                )?,
                tools: self.available_tools(&agent),
                transient_image: request.transient_image.clone(),
            };
            let response = tokio::time::timeout(
                Duration::from_secs(agent.timeout_secs),
                model_client.complete(model_request),
            )
            .await
            .map_err(|_| {
                EduMindError::Agent(format!("agent `{}` model request timed out", agent.id))
            })??;
            match response {
                ModelResponse::Final(content) => {
                    if content.trim().is_empty() {
                        return Err(EduMindError::Agent(
                            "model returned an empty final response".to_owned(),
                        ));
                    }
                    self.sessions.append_message(
                        &request.session_key,
                        ChatRole::Assistant,
                        &content,
                        Utc::now(),
                    )?;
                    return Ok(AgentRunResult {
                        agent_id: agent.id,
                        session_key: request.session_key,
                        model,
                        content,
                        tool_executions,
                    });
                }
                ModelResponse::ToolCall(call) => {
                    self.execute_tool_call(
                        ToolCallScope {
                            agent: &agent,
                            session_key: &request.session_key,
                            spawn_depth: request.spawn_depth,
                            action_grant: request.action_grant.as_ref(),
                        },
                        call,
                        tool_executor,
                        &mut tool_executions,
                    )
                    .await?;
                }
            }
        }
        Err(EduMindError::Agent(format!(
            "agent `{}` exceeded the configured {} tool-call rounds",
            agent.id, self.max_tool_rounds
        )))
    }

    /// Returns the durable session manager used by this runner.
    #[must_use]
    pub fn sessions(&self) -> SessionManager {
        self.sessions.clone()
    }

    /// Returns the content-free tool audit log.
    #[must_use]
    pub fn audit_log(&self) -> ToolAuditLog {
        self.audit.clone()
    }

    /// Returns the registry used for constrained subagent spawning.
    #[must_use]
    pub fn subagent_registry(&self) -> SubagentRegistry {
        SubagentRegistry::new(self.agents.clone())
    }

    fn available_tools(&self, agent: &AgentProfile) -> Vec<ToolDef> {
        self.tools
            .all()
            .into_iter()
            .filter(|tool| self.policy.authorize(agent, tool).is_allowed())
            .collect()
    }

    async fn execute_tool_call(
        &self,
        scope: ToolCallScope<'_>,
        call: ToolCall,
        tool_executor: &dyn ToolExecutor,
        tool_executions: &mut Vec<ToolExecution>,
    ) -> Result<()> {
        let ToolCallScope {
            agent,
            session_key,
            spawn_depth,
            action_grant,
        } = scope;
        let now = Utc::now();
        if let Err(error) = call.validate() {
            self.record_audit(ToolAuditEntry::new(
                now,
                &agent.id,
                session_key,
                &call.name,
                ToolAuditOutcome::Denied,
                None,
                "invalid tool call",
            ))?;
            return Err(error);
        }
        let tool = match self.tools.get(&call.name) {
            Some(tool) => tool,
            None => {
                self.record_audit(ToolAuditEntry::new(
                    now,
                    &agent.id,
                    session_key,
                    &call.name,
                    ToolAuditOutcome::Denied,
                    None,
                    "unknown tool",
                ))?;
                return Err(EduMindError::Tool(format!(
                    "tool `{}` is not registered",
                    call.name
                )));
            }
        };
        if let ToolPolicyDecision::Denied(reason) = self.policy.authorize(agent, &tool) {
            self.record_audit(ToolAuditEntry::new(
                now,
                &agent.id,
                session_key,
                &tool.name,
                ToolAuditOutcome::Denied,
                None,
                format!("policy denied: {reason}"),
            ))?;
            return Err(EduMindError::Tool(format!(
                "tool `{}` denied: {reason}",
                tool.name
            )));
        }
        if self.requires_action_grant(tool.class)
            && !action_grant.is_some_and(|grant| grant.is_valid(now))
        {
            self.record_audit(ToolAuditEntry::new(
                now,
                &agent.id,
                session_key,
                &tool.name,
                ToolAuditOutcome::Denied,
                None,
                "action password grant required",
            ))?;
            return Err(EduMindError::Security(format!(
                "tool `{}` requires a valid action-password grant",
                tool.name
            )));
        }
        if !self.rate_limiter.try_acquire(&agent.id, &tool.name, now)? {
            self.record_audit(ToolAuditEntry::new(
                now,
                &agent.id,
                session_key,
                &tool.name,
                ToolAuditOutcome::RateLimited,
                None,
                "per-agent per-tool rate limit reached",
            ))?;
            return Err(EduMindError::Tool(format!(
                "tool `{}` is rate limited for agent `{}`",
                tool.name, agent.id
            )));
        }
        if let ToolDailyLimitDecision::Denied(reason) =
            self.daily_limiter.try_acquire(&agent.id, tool.class, now)?
        {
            self.record_audit(ToolAuditEntry::new(
                now,
                &agent.id,
                session_key,
                &tool.name,
                ToolAuditOutcome::RateLimited,
                None,
                format!("daily {reason} tool quota reached"),
            ))?;
            return Err(EduMindError::Security(format!(
                "tool `{}` exceeded the daily {reason} quota",
                tool.name
            )));
        }
        let context = ToolExecutionContext {
            agent_id: agent.id.clone(),
            session_key: session_key.to_owned(),
            spawn_depth,
            write_sandbox: self.write_sandbox.clone(),
        };
        let started = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(
                agent
                    .timeout_secs
                    .min(self.execution_caps.max_tool_timeout_secs),
            ),
            tool_executor.execute(&context, &call),
        )
        .await;
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let output = match result {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => {
                self.record_audit(ToolAuditEntry::new(
                    Utc::now(),
                    &agent.id,
                    session_key,
                    &tool.name,
                    ToolAuditOutcome::Failed,
                    Some(duration_ms),
                    "tool executor returned an error",
                ))?;
                return Err(error);
            }
            Err(_) => {
                self.record_audit(ToolAuditEntry::new(
                    Utc::now(),
                    &agent.id,
                    session_key,
                    &tool.name,
                    ToolAuditOutcome::Failed,
                    Some(duration_ms),
                    "tool execution timed out",
                ))?;
                return Err(EduMindError::Tool(format!(
                    "tool `{}` timed out for agent `{}`",
                    tool.name, agent.id
                )));
            }
        };
        let output_text = serde_json::to_string(&output)?;
        if output_text.len() > self.execution_caps.max_output_bytes {
            self.record_audit(ToolAuditEntry::new(
                Utc::now(),
                &agent.id,
                session_key,
                &tool.name,
                ToolAuditOutcome::Failed,
                Some(duration_ms),
                "tool output exceeded configured byte cap",
            ))?;
            return Err(EduMindError::Security(format!(
                "tool `{}` output exceeds the {} byte cap",
                tool.name, self.execution_caps.max_output_bytes
            )));
        }
        self.sessions
            .append_message(session_key, ChatRole::Tool, output_text, Utc::now())?;
        self.record_audit(ToolAuditEntry::new(
            Utc::now(),
            &agent.id,
            session_key,
            &tool.name,
            ToolAuditOutcome::Succeeded,
            Some(duration_ms),
            "completed",
        ))?;
        tool_executions.push(ToolExecution {
            call_id: call.id,
            tool_name: tool.name,
            output,
        });
        Ok(())
    }

    fn record_audit(&self, entry: ToolAuditEntry) -> Result<()> {
        self.audit.record(entry)
    }

    fn requires_action_grant(&self, class: super::ToolClass) -> bool {
        self.action_password_required
            && matches!(class, super::ToolClass::Write | super::ToolClass::Execution)
    }
}

/// Built-in executor for agent orchestration tools. Other tool calls remain delegated to
/// their dedicated runtime executors in later phases.
#[derive(Clone)]
pub struct BuiltinAgentToolExecutor {
    sessions: SessionManager,
    agents: AgentRegistry,
    subagents: SubagentRegistry,
    srs: Option<SrsService>,
    student_pages: Option<StudentPageStore>,
    hybrid_memory: Option<HybridMemory>,
    memory_intelligence: Option<MemoryIntelligence>,
    research: Option<ResearchPipelineEngine>,
    research_projects: Option<ResearchProjectService>,
    fulltext_projects: Option<FullTextResearchService>,
    research_supervisor: Option<ResearchSupervisorService>,
    runtime_tools: Option<RuntimeToolService>,
}

impl BuiltinAgentToolExecutor {
    /// Creates the built-in executor using the active agent registry and session store.
    #[must_use]
    pub fn new(sessions: SessionManager, agents: AgentRegistry) -> Self {
        Self {
            sessions,
            subagents: SubagentRegistry::new(agents.clone()),
            agents,
            srs: None,
            student_pages: None,
            hybrid_memory: None,
            memory_intelligence: None,
            research: None,
            research_projects: None,
            fulltext_projects: None,
            research_supervisor: None,
            runtime_tools: None,
        }
    }

    /// Attaches the Phase 8 study and Student Page services to this executor.
    #[must_use]
    pub fn with_study_services(
        mut self,
        srs: SrsService,
        student_pages: StudentPageStore,
        hybrid_memory: HybridMemory,
    ) -> Self {
        self.srs = Some(srs);
        self.student_pages = Some(student_pages);
        self.hybrid_memory = Some(hybrid_memory);
        self
    }

    /// Attaches Phase 14 local memory intelligence services to this executor.
    #[must_use]
    pub fn with_memory_intelligence(mut self, memory_intelligence: MemoryIntelligence) -> Self {
        self.memory_intelligence = Some(memory_intelligence);
        self
    }

    /// Attaches the Phase 9 focused research pipeline to this executor.
    #[must_use]
    pub fn with_research_pipeline(mut self, research: ResearchPipelineEngine) -> Self {
        self.research = Some(research);
        self
    }

    /// Attaches the Phase 10 persisted-project assistant to this executor.
    #[must_use]
    pub fn with_research_projects(mut self, research_projects: ResearchProjectService) -> Self {
        self.research_projects = Some(research_projects);
        self
    }

    /// Attaches the Phase 11 full-text project assistant to this executor.
    #[must_use]
    pub fn with_fulltext_projects(mut self, fulltext_projects: FullTextResearchService) -> Self {
        self.fulltext_projects = Some(fulltext_projects);
        self
    }

    /// Attaches the Phase 13 research supervisor for gap detection and reading plans.
    #[must_use]
    pub fn with_research_supervisor(
        mut self,
        research_supervisor: ResearchSupervisorService,
    ) -> Self {
        self.research_supervisor = Some(research_supervisor);
        self
    }

    /// Attaches local artifact, PDF, LaTeX, image, and NotebookLM runtime tools.
    #[must_use]
    pub fn with_runtime_tools(mut self, runtime_tools: RuntimeToolService) -> Self {
        self.runtime_tools = Some(runtime_tools);
        self
    }

    /// Exposes the scheduler so an outer run manager can finish reserved tickets.
    #[must_use]
    pub fn subagent_registry(&self) -> SubagentRegistry {
        self.subagents.clone()
    }
}

#[async_trait]
impl ToolExecutor for BuiltinAgentToolExecutor {
    async fn execute(&self, context: &ToolExecutionContext, call: &ToolCall) -> Result<Value> {
        match call.name.as_str() {
            "notebooklm_ask" => {
                required_argument(call, "question")?;
                serde_json::to_value(
                    self.runtime_tool_service()?
                        .notebooklm_call(&call.name, call.arguments.clone())
                        .await?,
                )
                .map_err(EduMindError::from)
            }
            "notebooklm_setup_auth" => serde_json::to_value(
                self.runtime_tool_service()?
                    .notebooklm_call(&call.name, call.arguments.clone())
                    .await?,
            )
            .map_err(EduMindError::from),
            "notebooklm_add_notebook" => {
                required_argument(call, "notebook_id")?;
                serde_json::to_value(
                    self.runtime_tool_service()?
                        .notebooklm_call(&call.name, call.arguments.clone())
                        .await?,
                )
                .map_err(EduMindError::from)
            }
            "notebooklm_add_source" => {
                required_argument(call, "source")?;
                serde_json::to_value(
                    self.runtime_tool_service()?
                        .notebooklm_call(&call.name, call.arguments.clone())
                        .await?,
                )
                .map_err(EduMindError::from)
            }
            "notebooklm_list_notebooks" | "notebooklm_select_notebook" => {
                if call.name == "notebooklm_select_notebook" {
                    required_argument(call, "notebook_id")?;
                }
                serde_json::to_value(
                    self.runtime_tool_service()?
                        .notebooklm_call(&call.name, call.arguments.clone())
                        .await?,
                )
                .map_err(EduMindError::from)
            }
            "notebooklm_get_health" => {
                serde_json::to_value(self.runtime_tool_service()?.notebooklm_health().await?)
                    .map_err(EduMindError::from)
            }
            "pdf_extract_text" => serde_json::to_value(
                self.runtime_tool_service()?
                    .extract_pdf(Path::new(required_argument(call, "path")?))
                    .await?,
            )
            .map_err(EduMindError::from),
            "pdf_analyze" => serde_json::to_value(
                self.runtime_tool_service()?
                    .analyze_pdf(Path::new(required_argument(call, "path")?))
                    .await?,
            )
            .map_err(EduMindError::from),
            "doc_create" => serde_json::to_value(self.runtime_tool_service()?.create_document(
                &context.write_sandbox,
                required_argument(call, "title")?,
                optional_text_argument(call, "content")?.unwrap_or_default(),
                Utc::now(),
            )?)
            .map_err(EduMindError::from),
            "doc_view" => serde_json::to_value(
                self.runtime_tool_service()?
                    .document(required_argument(call, "id")?)?
                    .ok_or_else(|| {
                        EduMindError::Tool(format!(
                            "document `{}` does not exist",
                            required_argument(call, "id").unwrap_or_default()
                        ))
                    })?,
            )
            .map_err(EduMindError::from),
            "doc_list" => serde_json::to_value(self.runtime_tool_service()?.list_documents()?)
                .map_err(EduMindError::from),
            "doc_modify" => {
                let title = optional_string_argument(call, "title")?.map(ToOwned::to_owned);
                let content = optional_text_argument(call, "content")?.map(ToOwned::to_owned);
                serde_json::to_value(self.runtime_tool_service()?.modify_document(
                    &context.write_sandbox,
                    required_argument(call, "id")?,
                    title,
                    content,
                    Utc::now(),
                )?)
                .map_err(EduMindError::from)
            }
            "doc_convert" => serde_json::to_value(
                self.runtime_tool_service()?
                    .convert_document(
                        &context.write_sandbox,
                        required_argument(call, "id")?,
                        required_argument(call, "format")?,
                    )
                    .await?,
            )
            .map_err(EduMindError::from),
            "doc_restore" => serde_json::to_value(self.runtime_tool_service()?.restore_document(
                &context.write_sandbox,
                required_argument(call, "id")?,
                required_u64_argument(call, "version")?,
                Utc::now(),
            )?)
            .map_err(EduMindError::from),
            "slide_create" => serde_json::to_value(
                self.runtime_tool_service()?.create_deck(
                    &context.write_sandbox,
                    required_argument(call, "title")?,
                    optional_text_argument(call, "body")?
                        .or(optional_text_argument(call, "content")?)
                        .unwrap_or_default(),
                    Utc::now(),
                )?,
            )
            .map_err(EduMindError::from),
            "slide_read" => serde_json::to_value(
                self.runtime_tool_service()?
                    .deck(required_argument(call, "id")?)?
                    .ok_or_else(|| {
                        EduMindError::Tool(format!(
                            "slide deck `{}` does not exist",
                            required_argument(call, "id").unwrap_or_default()
                        ))
                    })?,
            )
            .map_err(EduMindError::from),
            "slide_delete" => serde_json::to_value(self.runtime_tool_service()?.delete_slide(
                &context.write_sandbox,
                required_argument(call, "id")?,
                required_usize_argument(call, "slide")?,
                Utc::now(),
            )?)
            .map_err(EduMindError::from),
            "slide_insert" => serde_json::to_value(
                self.runtime_tool_service()?.insert_slide(
                    &context.write_sandbox,
                    required_argument(call, "id")?,
                    optional_string_argument(call, "title")?.unwrap_or("New slide"),
                    optional_text_argument(call, "body")?
                        .or(optional_text_argument(call, "content")?)
                        .unwrap_or_default(),
                    optional_usize_argument(call, "after")?,
                    Utc::now(),
                )?,
            )
            .map_err(EduMindError::from),
            "slide_theme" => serde_json::to_value(self.runtime_tool_service()?.set_deck_theme(
                &context.write_sandbox,
                required_argument(call, "id")?,
                required_argument(call, "theme")?,
                Utc::now(),
            )?)
            .map_err(EduMindError::from),
            "slide_screenshot" => {
                serde_json::to_value(self.runtime_tool_service()?.screenshot_slide(
                    &context.write_sandbox,
                    required_argument(call, "id")?,
                    optional_usize_argument(call, "slide")?.unwrap_or(1),
                )?)
                .map_err(EduMindError::from)
            }
            "slide_check_overflow" => serde_json::to_value(
                self.runtime_tool_service()?
                    .check_slide_overflow(required_argument(call, "id")?)?,
            )
            .map_err(EduMindError::from),
            "slide_check" => serde_json::to_value(
                self.runtime_tool_service()?
                    .check_deck(required_argument(call, "id")?)?,
            )
            .map_err(EduMindError::from),
            "slide_restore_snapshot" => {
                serde_json::to_value(self.runtime_tool_service()?.restore_deck(
                    &context.write_sandbox,
                    required_argument(call, "id")?,
                    required_u64_argument(call, "snapshot")?,
                    Utc::now(),
                )?)
                .map_err(EduMindError::from)
            }
            "slide_thumbnail_grid" => serde_json::to_value(
                self.runtime_tool_service()?
                    .thumbnail_grid(&context.write_sandbox, required_argument(call, "id")?)?,
            )
            .map_err(EduMindError::from),
            "slide_list" => serde_json::to_value(self.runtime_tool_service()?.list_decks()?)
                .map_err(EduMindError::from),
            "slide_build_pptx" => serde_json::to_value(
                self.runtime_tool_service()?
                    .build_pptx(&context.write_sandbox, required_argument(call, "id")?)
                    .await?,
            )
            .map_err(EduMindError::from),
            "image_search" => serde_json::to_value(
                self.runtime_tool_service()?
                    .search_images(required_argument(call, "query")?)?,
            )
            .map_err(EduMindError::from),
            "image_download" => serde_json::to_value(
                self.runtime_tool_service()?
                    .download_image(
                        &context.write_sandbox,
                        required_argument(call, "url")?,
                        optional_string_argument(call, "name")?,
                    )
                    .await?,
            )
            .map_err(EduMindError::from),
            "image_ensure_raster" => serde_json::to_value(
                self.runtime_tool_service()?
                    .ensure_raster_image(
                        &context.write_sandbox,
                        Path::new(required_argument(call, "path")?),
                    )
                    .await?,
            )
            .map_err(EduMindError::from),
            "image_generate" => serde_json::to_value(self.runtime_tool_service()?.generate_image(
                &context.write_sandbox,
                required_argument(call, "prompt")?,
                optional_string_argument(call, "name")?,
            )?)
            .map_err(EduMindError::from),
            "latex_compile" => serde_json::to_value(
                self.runtime_tool_service()?
                    .compile_latex(
                        &context.write_sandbox,
                        Path::new(required_argument(call, "path")?),
                    )
                    .await?,
            )
            .map_err(EduMindError::from),
            "srs_card_create" => {
                let mut card = NewSrsCard::new(
                    required_argument(call, "front")?,
                    required_argument(call, "back")?,
                );
                if let Some(deck) = optional_string_argument(call, "deck")? {
                    card.deck = deck.to_owned();
                }
                serde_json::to_value(self.srs_service()?.create_card(card, Utc::now())?)
                    .map_err(EduMindError::from)
            }
            "srs_generate_from_notes" => {
                let deck = optional_string_argument(call, "deck")?.unwrap_or("default");
                serde_json::to_value(self.srs_service()?.generate_from_notes(
                    required_argument(call, "notes")?,
                    deck,
                    Utc::now(),
                )?)
                .map_err(EduMindError::from)
            }
            "srs_due" => {
                let deck = optional_string_argument(call, "deck")?;
                let limit = optional_limit_argument(call, "limit", 20)?;
                serde_json::to_value(self.srs_service()?.due(deck, Utc::now(), limit)?)
                    .map_err(EduMindError::from)
            }
            "srs_review" => {
                let raw_card_id = required_argument(call, "card_id")?;
                let card_id = Uuid::parse_str(raw_card_id)
                    .map(SrsCardId)
                    .map_err(|error| {
                        EduMindError::Tool(format!(
                            "tool `srs_review` received an invalid card_id `{raw_card_id}`: {error}"
                        ))
                    })?;
                let rating = required_rating_argument(call)?;
                serde_json::to_value(self.srs_service()?.review(card_id, rating, Utc::now())?)
                    .map_err(EduMindError::from)
            }
            "srs_stats" => {
                let deck = optional_string_argument(call, "deck")?;
                serde_json::to_value(self.srs_service()?.stats(deck, Utc::now())?)
                    .map_err(EduMindError::from)
            }
            "memory_search" => serde_json::to_value(
                self.memory_intelligence_service()?
                    .search(
                        required_argument(call, "query")?,
                        optional_limit_argument(call, "limit", 10)?,
                    )
                    .await?,
            )
            .map_err(EduMindError::from),
            "memory_get" => {
                let memory_id = parse_tool_memory_id(call)?;
                let record = self
                    .memory_intelligence_service()?
                    .get(memory_id)?
                    .ok_or_else(|| {
                        EduMindError::Tool(format!("memory record `{memory_id}` does not exist"))
                    })?;
                serde_json::to_value(record).map_err(EduMindError::from)
            }
            "memory_store" => {
                let module_id = optional_string_argument(call, "module_id")?.unwrap_or("global");
                let result = self
                    .memory_intelligence_service()?
                    .ingest(module_id, module_memory_input(call)?, Utc::now())
                    .await?;
                serde_json::to_value(result).map_err(EduMindError::from)
            }
            "module_memory_search" => serde_json::to_value(
                self.memory_intelligence_service()?
                    .modules()
                    .search(
                        required_argument(call, "module_id")?,
                        required_argument(call, "query")?,
                        optional_module_memory_scope(call)?,
                        optional_limit_argument(call, "limit", 10)?,
                    )
                    .await?,
            )
            .map_err(EduMindError::from),
            "module_memory_get" => {
                let memory_id = parse_tool_memory_id(call)?;
                let record = self
                    .memory_intelligence_service()?
                    .modules()
                    .get(
                        required_argument(call, "module_id")?,
                        memory_id,
                        optional_module_memory_scope(call)?,
                    )?
                    .ok_or_else(|| {
                        EduMindError::Tool(format!(
                            "module memory `{memory_id}` is absent or not visible"
                        ))
                    })?;
                serde_json::to_value(record).map_err(EduMindError::from)
            }
            "module_memory_store" => {
                let result = self
                    .memory_intelligence_service()?
                    .ingest(
                        required_argument(call, "module_id")?,
                        module_memory_input(call)?,
                        Utc::now(),
                    )
                    .await?;
                serde_json::to_value(result).map_err(EduMindError::from)
            }
            "module_memory_summary" => serde_json::to_value(
                self.memory_intelligence_service()?
                    .modules()
                    .summary(required_argument(call, "module_id")?)?,
            )
            .map_err(EduMindError::from),
            "wiki_search" => {
                serde_json::to_value(self.memory_intelligence_service()?.search_wiki(
                    required_argument(call, "query")?,
                    optional_limit_argument(call, "limit", 10)?,
                )?)
                .map_err(EduMindError::from)
            }
            "graph_search" => {
                serde_json::to_value(self.memory_intelligence_service()?.search_graph(
                    required_argument(call, "query")?,
                    optional_limit_argument(call, "limit", 10)?,
                )?)
                .map_err(EduMindError::from)
            }
            "graph_neighbors" => {
                let node_id = required_argument(call, "node_id")?;
                let neighborhood = self
                    .memory_intelligence_service()?
                    .graph_neighbors(node_id)?
                    .ok_or_else(|| {
                        EduMindError::Tool(format!(
                            "knowledge graph node `{node_id}` does not exist"
                        ))
                    })?;
                serde_json::to_value(neighborhood).map_err(EduMindError::from)
            }
            "student_page_get" => serde_json::to_value(
                self.student_page_service()?
                    .load(required_argument(call, "page")?)?,
            )
            .map_err(EduMindError::from),
            "student_planner_schedule" => {
                serde_json::to_value(self.student_page_service()?.planner_schedule()?)
                    .map_err(EduMindError::from)
            }
            "student_page_upsert" => {
                let page = required_argument(call, "page")?.to_owned();
                let key = required_argument(call, "key")?.to_owned();
                let now = Utc::now();
                let mutation = self.student_page_service()?.upsert_record(
                    &page,
                    StudentPageRecordInput::new(
                        key,
                        required_value_argument(call, "value")?.clone(),
                        now,
                    ),
                    format!("agent:{}", context.agent_id),
                )?;
                let indexed = self.hybrid_memory.as_ref().ok_or_else(|| {
                    EduMindError::Tool(
                        "student page semantic indexing is not configured".to_owned(),
                    )
                })?;
                let indexed_memory = self
                    .student_page_service()?
                    .index_snapshot(indexed, &page, now)
                    .await?;
                Ok(json!({
                    "mutation": mutation,
                    "indexed_memory_id": indexed_memory.id,
                }))
            }
            "student_page_delete" => {
                let page = required_argument(call, "page")?.to_owned();
                let key = required_argument(call, "key")?.to_owned();
                let now = Utc::now();
                let mutation = self.student_page_service()?.delete_record(
                    &page,
                    key,
                    now,
                    format!("agent:{}", context.agent_id),
                )?;
                let indexed = self.hybrid_memory.as_ref().ok_or_else(|| {
                    EduMindError::Tool(
                        "student page semantic indexing is not configured".to_owned(),
                    )
                })?;
                let indexed_memory = self
                    .student_page_service()?
                    .index_snapshot(indexed, &page, now)
                    .await?;
                Ok(json!({
                    "mutation": mutation,
                    "indexed_memory_id": indexed_memory.id,
                }))
            }
            "research_run" => {
                let mut request = ResearchRequest::new(required_argument(call, "query")?);
                if let Some(value) = call.arguments.get("topic") {
                    request.topic = non_empty_json_string(call, "topic", value)?.to_owned();
                    request.query = request.topic.clone();
                }
                if let Some(seed_papers) = optional_json_argument(call, "seed_papers")? {
                    request.seed_papers = seed_papers;
                }
                if let Some(claims) = optional_json_argument(call, "claims")? {
                    request.claims = claims;
                }
                if let Some(sources) = optional_json_argument(call, "sources")? {
                    request.sources = sources;
                }
                if call.arguments.get("max_results").is_some() {
                    request.max_results = optional_limit_argument(call, "max_results", 20)?;
                }
                serde_json::to_value(self.research_pipeline()?.run(request, Utc::now()).await?)
                    .map_err(EduMindError::from)
            }
            "research_validate_claims" => {
                let request = if let Some(request) = optional_json_argument(call, "request")? {
                    request
                } else {
                    ClaimValidationRequest {
                        claims: required_string_list_argument(call, "claims")?,
                        evidence: optional_json_argument(call, "evidence")?.unwrap_or_default(),
                        support_threshold: call
                            .arguments
                            .get("support_threshold")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.35),
                    }
                };
                serde_json::to_value(validate_claims(&request)).map_err(EduMindError::from)
            }
            "research_literature_graph" => {
                let request: LiteratureGraphRequest = required_json_argument(call, "request")?;
                serde_json::to_value(build_literature_graph(&request)).map_err(EduMindError::from)
            }
            "research_project_ask" => {
                let raw_project_id = required_argument(call, "project_id")?;
                let project_id = ResearchProjectId::parse(raw_project_id).map_err(|error| {
                    EduMindError::Tool(format!(
                        "tool `research_project_ask` received an invalid project_id `{raw_project_id}`: {error}"
                    ))
                })?;
                let answer = self
                    .research_project_service()?
                    .ask(
                        project_id,
                        required_argument(call, "question")?,
                        optional_limit_argument(call, "limit", 5)?,
                    )
                    .await?
                    .ok_or_else(|| {
                        EduMindError::Tool(format!(
                            "research project `{project_id}` does not exist"
                        ))
                    })?;
                serde_json::to_value(answer).map_err(EduMindError::from)
            }
            "research_ingest" => {
                let raw_project_id = required_argument(call, "project_id")?;
                let project_id = ResearchProjectId::parse(raw_project_id).map_err(|error| {
                    EduMindError::Tool(format!(
                        "tool `research_ingest` received an invalid project_id `{raw_project_id}`: {error}"
                    ))
                })?;
                let document = self
                    .fulltext_project_service()?
                    .ingest(
                        project_id,
                        FullTextIngestRequest {
                            paper_id: optional_string_argument(call, "paper_id")?
                                .map(ToOwned::to_owned),
                            source: Some(required_argument(call, "source")?.to_owned()),
                            title: optional_string_argument(call, "title")?.map(ToOwned::to_owned),
                            ocr: optional_ocr_mode_argument(call)?,
                        },
                        Utc::now(),
                    )
                    .await?
                    .ok_or_else(|| {
                        EduMindError::Tool(format!(
                            "research project `{project_id}` does not exist"
                        ))
                    })?;
                serde_json::to_value(document).map_err(EduMindError::from)
            }
            "research_deep_ask" => {
                let raw_project_id = required_argument(call, "project_id")?;
                let project_id = ResearchProjectId::parse(raw_project_id).map_err(|error| {
                    EduMindError::Tool(format!(
                        "tool `research_deep_ask` received an invalid project_id `{raw_project_id}`: {error}"
                    ))
                })?;
                let answer = self
                    .fulltext_project_service()?
                    .deep_ask(
                        project_id,
                        required_argument(call, "question")?,
                        optional_limit_argument(call, "limit", 5)?,
                    )
                    .await?
                    .ok_or_else(|| {
                        EduMindError::Tool(format!(
                            "research project `{project_id}` does not exist"
                        ))
                    })?;
                serde_json::to_value(answer).map_err(EduMindError::from)
            }
            "research_gaps" => {
                let project_id = parse_tool_project_id(call)?;
                let report = self
                    .research_supervisor_service()?
                    .gaps(project_id)?
                    .ok_or_else(|| {
                        EduMindError::Tool(format!(
                            "research project `{project_id}` does not exist"
                        ))
                    })?;
                serde_json::to_value(report).map_err(EduMindError::from)
            }
            "research_supervise" => {
                let project_id = parse_tool_project_id(call)?;
                let report = self
                    .research_supervisor_service()?
                    .supervise(project_id)?
                    .ok_or_else(|| {
                        EduMindError::Tool(format!(
                            "research project `{project_id}` does not exist"
                        ))
                    })?;
                serde_json::to_value(report).map_err(EduMindError::from)
            }
            "sessions_spawn" => {
                let agent_id = required_argument(call, "agent_id")?;
                let session_key = required_argument(call, "session_key")?;
                self.agents.resolve(Some(agent_id))?;
                let session =
                    self.sessions
                        .spawn(&context.session_key, session_key, agent_id, Utc::now())?;
                Ok(json!({
                    "session_key": session.session_key,
                    "agent_id": session.agent_id,
                    "status": "created",
                }))
            }
            "run_subagent" => {
                let agent_id = required_argument(call, "agent_id")?;
                let session_key = required_argument(call, "session_key")?;
                let ticket = self.subagents.spawn(
                    &context.agent_id,
                    context.spawn_depth,
                    agent_id,
                    session_key,
                    Utc::now(),
                )?;
                serde_json::to_value(ticket).map_err(EduMindError::from)
            }
            _ => Err(EduMindError::Tool(format!(
                "built-in executor does not handle tool `{}`",
                call.name
            ))),
        }
    }
}

impl BuiltinAgentToolExecutor {
    fn srs_service(&self) -> Result<&SrsService> {
        self.srs.as_ref().ok_or_else(|| {
            EduMindError::Tool("SRS service is not configured for this executor".to_owned())
        })
    }

    fn student_page_service(&self) -> Result<&StudentPageStore> {
        self.student_pages.as_ref().ok_or_else(|| {
            EduMindError::Tool(
                "Student Page service is not configured for this executor".to_owned(),
            )
        })
    }

    fn research_pipeline(&self) -> Result<&ResearchPipelineEngine> {
        self.research.as_ref().ok_or_else(|| {
            EduMindError::Tool("research pipeline is not configured for this executor".to_owned())
        })
    }

    fn research_project_service(&self) -> Result<&ResearchProjectService> {
        self.research_projects.as_ref().ok_or_else(|| {
            EduMindError::Tool(
                "research project assistant is not configured for this executor".to_owned(),
            )
        })
    }

    fn fulltext_project_service(&self) -> Result<&FullTextResearchService> {
        self.fulltext_projects.as_ref().ok_or_else(|| {
            EduMindError::Tool(
                "full-text project assistant is not configured for this executor".to_owned(),
            )
        })
    }

    fn research_supervisor_service(&self) -> Result<&ResearchSupervisorService> {
        self.research_supervisor.as_ref().ok_or_else(|| {
            EduMindError::Tool("research supervisor is not configured for this executor".to_owned())
        })
    }

    fn memory_intelligence_service(&self) -> Result<&MemoryIntelligence> {
        self.memory_intelligence.as_ref().ok_or_else(|| {
            EduMindError::Tool("memory intelligence is not configured for this executor".to_owned())
        })
    }

    fn runtime_tool_service(&self) -> Result<&RuntimeToolService> {
        self.runtime_tools.as_ref().ok_or_else(|| {
            EduMindError::Tool("runtime tools are not configured for this executor".to_owned())
        })
    }
}

fn parse_tool_project_id(call: &ToolCall) -> Result<ResearchProjectId> {
    let raw_project_id = required_argument(call, "project_id")?;
    ResearchProjectId::parse(raw_project_id).map_err(|error| {
        EduMindError::Tool(format!(
            "tool `{}` received an invalid project_id `{raw_project_id}`: {error}",
            call.name
        ))
    })
}

fn optional_ocr_mode_argument(call: &ToolCall) -> Result<OcrMode> {
    let Some(value) = call.arguments.get("ocr") else {
        return Ok(OcrMode::Auto);
    };
    serde_json::from_value(value.clone()).map_err(|error| {
        EduMindError::Tool(format!(
            "tool `{}` received an invalid `ocr` mode: {error}",
            call.name
        ))
    })
}

fn parse_tool_memory_id(call: &ToolCall) -> Result<MemoryId> {
    let raw_memory_id = required_argument(call, "id")?;
    MemoryId::parse(raw_memory_id).map_err(|error| {
        EduMindError::Tool(format!(
            "tool `{}` received an invalid memory id `{raw_memory_id}`: {error}",
            call.name
        ))
    })
}

fn optional_module_memory_scope(call: &ToolCall) -> Result<ModuleMemoryScope> {
    let Some(value) = call.arguments.get("scope") else {
        return Ok(ModuleMemoryScope::Module);
    };
    serde_json::from_value(value.clone()).map_err(|error| {
        EduMindError::Tool(format!(
            "tool `{}` received an invalid module-memory scope: {error}",
            call.name
        ))
    })
}

fn module_memory_input(call: &ToolCall) -> Result<NewModuleMemory> {
    Ok(NewModuleMemory {
        content: required_argument(call, "content")?.to_owned(),
        content_type: optional_string_argument(call, "content_type")?
            .unwrap_or("note")
            .to_owned(),
        scope: optional_module_memory_scope(call)?,
        metadata: optional_json_argument(call, "metadata")?.unwrap_or(Value::Null),
    })
}

fn required_argument<'a>(call: &'a ToolCall, name: &str) -> Result<&'a str> {
    call.arguments
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            EduMindError::Tool(format!(
                "tool `{}` requires a non-empty `{name}` argument",
                call.name
            ))
        })
}

fn required_value_argument<'a>(call: &'a ToolCall, name: &str) -> Result<&'a Value> {
    call.arguments.get(name).ok_or_else(|| {
        EduMindError::Tool(format!("tool `{}` requires a `{name}` argument", call.name))
    })
}

fn required_json_argument<T>(call: &ToolCall, name: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(required_value_argument(call, name)?.clone()).map_err(|error| {
        EduMindError::Tool(format!(
            "tool `{}` received an invalid `{name}` payload: {error}",
            call.name
        ))
    })
}

fn optional_json_argument<T>(call: &ToolCall, name: &str) -> Result<Option<T>>
where
    T: DeserializeOwned,
{
    match call.arguments.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| {
                EduMindError::Tool(format!(
                    "tool `{}` received an invalid `{name}` payload: {error}",
                    call.name
                ))
            }),
    }
}

fn required_string_list_argument(call: &ToolCall, name: &str) -> Result<Vec<String>> {
    let values = match required_value_argument(call, name)? {
        Value::String(value) if !value.trim().is_empty() => vec![value.trim().to_owned()],
        Value::Array(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| {
                        EduMindError::Tool(format!(
                            "tool `{}` requires every `{name}` value to be a non-empty string",
                            call.name
                        ))
                    })
            })
            .collect::<Result<Vec<_>>>()?,
        _ => {
            return Err(EduMindError::Tool(format!(
                "tool `{}` requires `{name}` as a non-empty string or array of strings",
                call.name
            )));
        }
    };
    if values.is_empty() {
        return Err(EduMindError::Tool(format!(
            "tool `{}` requires at least one `{name}` value",
            call.name
        )));
    }
    Ok(values)
}

fn non_empty_json_string<'a>(call: &ToolCall, name: &str, value: &'a Value) -> Result<&'a str> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            EduMindError::Tool(format!(
                "tool `{}` requires `{name}` to be a non-empty string",
                call.name
            ))
        })
}

fn optional_string_argument<'a>(call: &'a ToolCall, name: &str) -> Result<Option<&'a str>> {
    match call.arguments.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value)),
        Some(_) => Err(EduMindError::Tool(format!(
            "tool `{}` optional `{name}` argument must be a non-empty string when set",
            call.name
        ))),
    }
}

fn optional_text_argument<'a>(call: &'a ToolCall, name: &str) -> Result<Option<&'a str>> {
    match call.arguments.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(EduMindError::Tool(format!(
            "tool `{}` optional `{name}` argument must be a string when set",
            call.name
        ))),
    }
}

fn required_u64_argument(call: &ToolCall, name: &str) -> Result<u64> {
    optional_u64_argument(call, name)?.ok_or_else(|| {
        EduMindError::Tool(format!(
            "tool `{}` requires a numeric `{name}` argument",
            call.name
        ))
    })
}

fn required_usize_argument(call: &ToolCall, name: &str) -> Result<usize> {
    let value = required_u64_argument(call, name)?;
    usize::try_from(value).map_err(|_| {
        EduMindError::Tool(format!(
            "tool `{}` received an oversized `{name}` argument",
            call.name
        ))
    })
}

fn optional_usize_argument(call: &ToolCall, name: &str) -> Result<Option<usize>> {
    match optional_u64_argument(call, name)? {
        Some(value) => usize::try_from(value).map(Some).map_err(|_| {
            EduMindError::Tool(format!(
                "tool `{}` received an oversized `{name}` argument",
                call.name
            ))
        }),
        None => Ok(None),
    }
}

fn optional_u64_argument(call: &ToolCall, name: &str) -> Result<Option<u64>> {
    match call.arguments.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value.as_u64().map(Some).ok_or_else(|| {
            EduMindError::Tool(format!(
                "tool `{}` optional `{name}` argument must be a non-negative integer",
                call.name
            ))
        }),
        Some(Value::String(value)) => value.trim().parse::<u64>().map(Some).map_err(|_| {
            EduMindError::Tool(format!(
                "tool `{}` optional `{name}` argument must be a non-negative integer",
                call.name
            ))
        }),
        Some(_) => Err(EduMindError::Tool(format!(
            "tool `{}` optional `{name}` argument must be a non-negative integer",
            call.name
        ))),
    }
}

fn optional_limit_argument(call: &ToolCall, name: &str, default: usize) -> Result<usize> {
    match call.arguments.get(name) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0 && *value <= 100)
            .ok_or_else(|| {
                EduMindError::Tool(format!(
                    "tool `{}` optional `{name}` argument must be an integer from 1 to 100",
                    call.name
                ))
            }),
    }
}

fn required_rating_argument(call: &ToolCall) -> Result<u8> {
    call.arguments
        .get("rating")
        .and_then(Value::as_u64)
        .and_then(|rating| u8::try_from(rating).ok())
        .filter(|rating| *rating <= 5)
        .ok_or_else(|| {
            EduMindError::Tool(format!(
                "tool `{}` requires a numeric `rating` from 0 to 5",
                call.name
            ))
        })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;
    use serde_json::{Value, json};

    use super::{
        AgentModel, AgentRunRequest, AgentRunner, BuiltinAgentToolExecutor, ModelRequest,
        ModelResponse, ToolExecutionContext, ToolExecutor,
    };
    use crate::{
        agent::{AgentRegistry, ChatRole, SessionManager, ToolAuditOutcome, ToolCall},
        config::{
            EduMindConfig,
            types::{AgentConfig, ToolProfile},
        },
        infra::{EduMindError, Result},
        memory::{HashEmbedder, HybridMemory, MemoryIntelligence, MemoryStore},
        student::StudentPageStore,
        study::SrsService,
    };

    struct ScriptedModel {
        responses: Mutex<VecDeque<ModelResponse>>,
    }

    #[async_trait]
    impl AgentModel for ScriptedModel {
        async fn complete(&self, _: ModelRequest) -> Result<ModelResponse> {
            self.responses
                .lock()
                .map_err(|error| EduMindError::Agent(format!("test model lock failed: {error}")))?
                .pop_front()
                .ok_or_else(|| EduMindError::Agent("test model ran out of responses".to_owned()))
        }
    }

    struct SearchExecutor;

    #[async_trait]
    impl ToolExecutor for SearchExecutor {
        async fn execute(&self, _: &ToolExecutionContext, call: &ToolCall) -> Result<Value> {
            if call.name != "memory_search" {
                return Err(EduMindError::Tool("unexpected tool".to_owned()));
            }
            Ok(json!({"hits": ["retrieved note"]}))
        }
    }

    #[tokio::test]
    async fn run_loop_persists_observations_and_audits_authorized_tools() {
        let mut config = EduMindConfig::default();
        config.agents.list[0].allowed_tools = vec!["memory_search".to_owned()];
        let sessions = SessionManager::in_memory(&config.session).unwrap();
        let runner = AgentRunner::new(&config, sessions.clone()).unwrap();
        let model = ScriptedModel {
            responses: Mutex::new(VecDeque::from([
                ModelResponse::ToolCall(ToolCall::new(
                    "call-1",
                    "memory_search",
                    json!({"query": "calculus"}),
                )),
                ModelResponse::Final("Use the retrieved note for review.".to_owned()),
            ])),
        };

        let result = runner
            .run_turn(
                AgentRunRequest::new("desktop:student", "Help me review calculus."),
                &model,
                &SearchExecutor,
            )
            .await
            .unwrap();

        assert_eq!(result.tool_executions.len(), 1);
        assert_eq!(result.content, "Use the retrieved note for review.");
        let messages = sessions
            .messages("desktop:student", chrono::Utc::now())
            .unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1].role, ChatRole::Tool);
        assert_eq!(runner.audit_log().entries().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn tool_calls_are_denied_before_execution_when_not_allowlisted() {
        let config = EduMindConfig::default();
        let sessions = SessionManager::in_memory(&config.session).unwrap();
        let runner = AgentRunner::new(&config, sessions).unwrap();
        let model = ScriptedModel {
            responses: Mutex::new(VecDeque::from([ModelResponse::ToolCall(ToolCall::new(
                "call-1",
                "memory_search",
                json!({"query": "calculus"}),
            ))])),
        };

        assert!(
            runner
                .run_turn(
                    AgentRunRequest::new("desktop:student", "Help me review calculus."),
                    &model,
                    &SearchExecutor,
                )
                .await
                .is_err()
        );
        assert_eq!(runner.audit_log().entries().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn mutating_tools_require_an_action_grant() {
        let mut config = EduMindConfig::default();
        config.tools.profile = ToolProfile::Full;
        config.agents.list[0].allowed_tools = vec!["write".to_owned()];
        let sessions = SessionManager::in_memory(&config.session).unwrap();
        let runner = AgentRunner::new(&config, sessions).unwrap();
        let model = ScriptedModel {
            responses: Mutex::new(VecDeque::from([ModelResponse::ToolCall(ToolCall::new(
                "call-1",
                "write",
                json!({"path": "data/OUTPUT/note.txt", "content": "note"}),
            ))])),
        };

        assert!(
            runner
                .run_turn(
                    AgentRunRequest::new("desktop:student", "Save this note."),
                    &model,
                    &SearchExecutor,
                )
                .await
                .is_err()
        );
        let entry = runner.audit_log().entries().unwrap().pop().unwrap();
        assert_eq!(entry.outcome, ToolAuditOutcome::Denied);
        assert_eq!(entry.detail, "action password grant required");
    }

    #[tokio::test]
    async fn rejects_tool_outputs_over_the_security_byte_cap() {
        let mut config = EduMindConfig::default();
        config.agents.list[0].allowed_tools = vec!["memory_search".to_owned()];
        config.security.execution.max_output_bytes = 8;
        let sessions = SessionManager::in_memory(&config.session).unwrap();
        let runner = AgentRunner::new(&config, sessions).unwrap();
        let model = ScriptedModel {
            responses: Mutex::new(VecDeque::from([ModelResponse::ToolCall(ToolCall::new(
                "call-1",
                "memory_search",
                json!({"query": "calculus"}),
            ))])),
        };

        assert!(
            runner
                .run_turn(
                    AgentRunRequest::new("desktop:student", "Find my calculus note."),
                    &model,
                    &SearchExecutor,
                )
                .await
                .is_err()
        );
        let entry = runner.audit_log().entries().unwrap().pop().unwrap();
        assert_eq!(entry.outcome, ToolAuditOutcome::Failed);
        assert_eq!(entry.detail, "tool output exceeded configured byte cap");
    }

    #[tokio::test]
    async fn builtins_spawn_sessions_and_constrained_subagents() {
        let mut config = EduMindConfig::default();
        config.tools.profile = ToolProfile::Full;
        config.agents.list[0].allowed_subagents = vec!["researcher".to_owned()];
        let researcher = AgentConfig {
            id: "researcher".to_owned(),
            name: "Research Agent".to_owned(),
            ..AgentConfig::default()
        };
        config.agents.list.push(researcher);
        let agents = AgentRegistry::from_config(&config).unwrap();
        let sessions = SessionManager::in_memory(&config.session).unwrap();
        sessions
            .get_or_create("desktop:parent", "master", chrono::Utc::now())
            .unwrap();
        let executor = BuiltinAgentToolExecutor::new(sessions.clone(), agents);
        let context = ToolExecutionContext {
            agent_id: "master".to_owned(),
            session_key: "desktop:parent".to_owned(),
            spawn_depth: 0,
            write_sandbox: crate::security::WriteSandbox::from_config(&config.security),
        };

        executor
            .execute(
                &context,
                &ToolCall::new(
                    "call-session",
                    "sessions_spawn",
                    json!({"agent_id": "researcher", "session_key": "desktop:child"}),
                ),
            )
            .await
            .unwrap();
        executor
            .execute(
                &context,
                &ToolCall::new(
                    "call-agent",
                    "run_subagent",
                    json!({"agent_id": "researcher", "session_key": "desktop:child"}),
                ),
            )
            .await
            .unwrap();

        assert!(
            sessions
                .get("desktop:child", chrono::Utc::now())
                .unwrap()
                .is_some()
        );
        assert_eq!(
            executor
                .subagent_registry()
                .active_count("researcher")
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn builtins_execute_srs_and_student_page_tools_with_canonical_services() {
        let mut config = EduMindConfig::default();
        config.tools.profile = ToolProfile::Full;
        let agents = AgentRegistry::from_config(&config).unwrap();
        let sessions = SessionManager::in_memory(&config.session).unwrap();
        let memory_store = MemoryStore::in_memory().unwrap();
        let hybrid_memory = HybridMemory::new(
            memory_store.clone(),
            Arc::new(HashEmbedder::new(32).unwrap()),
        )
        .unwrap();
        let executor = BuiltinAgentToolExecutor::new(sessions, agents).with_study_services(
            SrsService::new(memory_store.clone()),
            StudentPageStore::new(memory_store),
            hybrid_memory,
        );
        let context = ToolExecutionContext {
            agent_id: "master".to_owned(),
            session_key: "desktop:student".to_owned(),
            spawn_depth: 0,
            write_sandbox: crate::security::WriteSandbox::from_config(&config.security),
        };

        let card = executor
            .execute(
                &context,
                &ToolCall::new(
                    "call-card",
                    "srs_card_create",
                    json!({"front": "Vector", "back": "Magnitude and direction"}),
                ),
            )
            .await
            .unwrap();
        let page = executor
            .execute(
                &context,
                &ToolCall::new(
                    "call-page",
                    "student_page_upsert",
                    json!({
                        "page": "planner",
                        "key": "monday",
                        "value": {
                            "kind": "schedule-block",
                            "day": "Monday",
                            "title": "Physics",
                            "start": "09:00",
                            "end": "10:00"
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        let schedule = executor
            .execute(
                &context,
                &ToolCall::new("call-schedule", "student_planner_schedule", json!({})),
            )
            .await
            .unwrap();

        assert!(card["id"].is_string());
        assert_eq!(page["mutation"]["record"]["key"], "monday");
        assert!(page["indexed_memory_id"].is_string());
        assert_eq!(schedule["days"][0]["entries"][0]["title"], "Physics");
    }

    #[tokio::test]
    async fn builtins_execute_local_memory_intelligence_tools() {
        let mut config = EduMindConfig::default();
        config.tools.profile = ToolProfile::Full;
        let agents = AgentRegistry::from_config(&config).unwrap();
        let sessions = SessionManager::in_memory(&config.session).unwrap();
        let store = MemoryStore::in_memory().unwrap();
        let hybrid = HybridMemory::new(store, Arc::new(HashEmbedder::new(64).unwrap())).unwrap();
        let intelligence = MemoryIntelligence::new(hybrid, config.memory.hermes.clone()).unwrap();
        let executor =
            BuiltinAgentToolExecutor::new(sessions, agents).with_memory_intelligence(intelligence);
        let context = ToolExecutionContext {
            agent_id: "master".to_owned(),
            session_key: "desktop:student".to_owned(),
            spawn_depth: 0,
            write_sandbox: crate::security::WriteSandbox::from_config(&config.security),
        };

        let stored = executor
            .execute(
                &context,
                &ToolCall::new(
                    "memory-store",
                    "module_memory_store",
                    json!({
                        "module_id": "class-notes",
                        "content": "Calculus limits revision improves derivative readiness.",
                        "scope": "module",
                    }),
                ),
            )
            .await
            .unwrap();
        let memory_id = stored["record"]["id"].as_str().unwrap().to_owned();

        let module_search = executor
            .execute(
                &context,
                &ToolCall::new(
                    "module-search",
                    "module_memory_search",
                    json!({"module_id": "class-notes", "query": "calculus"}),
                ),
            )
            .await
            .unwrap();
        assert_eq!(module_search.as_array().unwrap().len(), 1);

        let record = executor
            .execute(
                &context,
                &ToolCall::new("memory-get", "memory_get", json!({"id": memory_id})),
            )
            .await
            .unwrap();
        assert_eq!(record["module_id"], "class-notes");

        let wiki = executor
            .execute(
                &context,
                &ToolCall::new("wiki", "wiki_search", json!({"query": "calculus"})),
            )
            .await
            .unwrap();
        assert!(!wiki.as_array().unwrap().is_empty());

        let graph = executor
            .execute(
                &context,
                &ToolCall::new("graph", "graph_search", json!({"query": "calculus"})),
            )
            .await
            .unwrap();
        let node_id = graph[0]["node"]["id"].as_str().unwrap().to_owned();
        let neighbors = executor
            .execute(
                &context,
                &ToolCall::new("neighbors", "graph_neighbors", json!({"node_id": node_id})),
            )
            .await
            .unwrap();
        assert!(!neighbors["neighbors"].as_array().unwrap().is_empty());
    }
}
