use std::{collections::HashSet, net::IpAddr, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::infra::{EduMindError, Result};

/// Complete, serializable configuration for the EduMind gateway and desktop app.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EduMindConfig {
    pub meta: MetaConfig,
    pub gateway: GatewayConfig,
    pub models: ModelsConfig,
    pub agents: AgentsConfig,
    pub channels: ChannelsConfig,
    pub routing: RoutingConfig,
    pub memory: MemoryConfig,
    pub plugins: PluginsConfig,
    pub tools: ToolsConfig,
    pub messages: MessagesConfig,
    pub session: SessionConfig,
    pub web: WebConfig,
    pub jobs: JobsConfig,
    pub security: SecurityConfig,
}

impl EduMindConfig {
    /// Validates cross-section invariants before a configuration becomes active.
    pub fn validate(&self) -> Result<()> {
        if self.meta.name.trim().is_empty() {
            return Err(EduMindError::ConfigValidation(
                "meta.name must not be empty".to_owned(),
            ));
        }
        if self.gateway.bind_address.trim().is_empty() {
            return Err(EduMindError::ConfigValidation(
                "gateway.bind_address must not be empty".to_owned(),
            ));
        }
        if self.gateway.request_body_max_bytes == 0 {
            return Err(EduMindError::ConfigValidation(
                "gateway.request_body_max_bytes must be greater than zero".to_owned(),
            ));
        }
        if self.gateway.auth.mode == AuthMode::None && !is_loopback_bind(&self.gateway.bind_address)
        {
            return Err(EduMindError::ConfigValidation(
                "gateway.auth.mode=none requires a loopback gateway.bind_address".to_owned(),
            ));
        }
        match self.gateway.auth.mode {
            AuthMode::Token if is_blank(&self.gateway.auth.token) => {
                return Err(EduMindError::ConfigValidation(
                    "gateway.auth.token is required when gateway.auth.mode=token".to_owned(),
                ));
            }
            AuthMode::Jwt if is_blank(&self.gateway.auth.jwt_secret) => {
                return Err(EduMindError::ConfigValidation(
                    "gateway.auth.jwt_secret is required when gateway.auth.mode=jwt".to_owned(),
                ));
            }
            _ => {}
        }
        if self.memory.db_path.as_os_str().is_empty() {
            return Err(EduMindError::ConfigValidation(
                "memory.db_path must not be empty".to_owned(),
            ));
        }
        if self.memory.embedding.dimensions == 0 || self.memory.vector.dimensions == 0 {
            return Err(EduMindError::ConfigValidation(
                "memory embedding and vector dimensions must be greater than zero".to_owned(),
            ));
        }
        if self.memory.embedding.dimensions != self.memory.vector.dimensions {
            return Err(EduMindError::ConfigValidation(
                "memory.embedding.dimensions must equal memory.vector.dimensions".to_owned(),
            ));
        }
        if self.memory.vector.candidate_count == 0 {
            return Err(EduMindError::ConfigValidation(
                "memory.vector.candidate_count must be greater than zero".to_owned(),
            ));
        }
        if self.memory.hermes.max_cycles_per_day == 0 || self.memory.hermes.cooldown_secs == 0 {
            return Err(EduMindError::ConfigValidation(
                "memory.hermes max_cycles_per_day and cooldown_secs must be greater than zero"
                    .to_owned(),
            ));
        }
        if self.agents.defaults.max_concurrent_runs == 0 {
            return Err(EduMindError::ConfigValidation(
                "agents.defaults.max_concurrent_runs must be greater than zero".to_owned(),
            ));
        }
        if self.agents.defaults.default_agent.trim().is_empty() {
            return Err(EduMindError::ConfigValidation(
                "agents.defaults.default_agent must not be empty".to_owned(),
            ));
        }
        if self.agents.defaults.default_model.trim().is_empty() {
            return Err(EduMindError::ConfigValidation(
                "agents.defaults.default_model must not be empty".to_owned(),
            ));
        }
        if self.agents.sandbox.enabled && self.agents.sandbox.root.as_os_str().is_empty() {
            return Err(EduMindError::ConfigValidation(
                "agents.sandbox.root must not be empty when the sandbox is enabled".to_owned(),
            ));
        }
        if self.agents.sandbox.max_control_file_bytes == 0
            || self.agents.sandbox.max_control_file_bytes > 256 * 1024
        {
            return Err(EduMindError::ConfigValidation(
                "agents.sandbox.max_control_file_bytes must be between 1 and 262144".to_owned(),
            ));
        }
        if self.agents.list.is_empty() {
            return Err(EduMindError::ConfigValidation(
                "agents.list must contain at least one agent".to_owned(),
            ));
        }
        for agent in &self.agents.list {
            if agent.name.trim().is_empty() {
                return Err(EduMindError::ConfigValidation(format!(
                    "agents.list[{}].name must not be empty",
                    agent.id
                )));
            }
            if agent.timeout_secs == 0 {
                return Err(EduMindError::ConfigValidation(format!(
                    "agents.list[{}].timeout_secs must be greater than zero",
                    agent.id
                )));
            }
            if agent.max_concurrent_runs == Some(0) {
                return Err(EduMindError::ConfigValidation(format!(
                    "agents.list[{}].max_concurrent_runs must be greater than zero when set",
                    agent.id
                )));
            }
        }
        if !self
            .agents
            .list
            .iter()
            .any(|agent| agent.id == self.agents.defaults.default_agent && agent.enabled)
        {
            return Err(EduMindError::ConfigValidation(format!(
                "agents.defaults.default_agent `{}` must reference an enabled agent",
                self.agents.defaults.default_agent
            )));
        }
        if self.tools.rate_limit_per_minute == 0
            || self.tools.audit_log_capacity == 0
            || self.tools.max_tool_rounds == 0
        {
            return Err(EduMindError::ConfigValidation(
                "tools.rate_limit_per_minute, tools.audit_log_capacity, and tools.max_tool_rounds must be greater than zero"
                    .to_owned(),
            ));
        }
        if self.session.max_messages == 0 || self.session.max_age_minutes == 0 {
            return Err(EduMindError::ConfigValidation(
                "session.max_messages and session.max_age_minutes must be greater than zero"
                    .to_owned(),
            ));
        }
        self.routing.validate()?;
        self.channels.validate()?;
        self.jobs.validate()?;
        for job in &self.jobs.schedules {
            if job.enabled
                && !self
                    .agents
                    .list
                    .iter()
                    .any(|agent| agent.id == job.agent_id && agent.enabled)
            {
                return Err(EduMindError::ConfigValidation(format!(
                    "jobs.schedules[{}].agent_id `{}` must reference an enabled agent",
                    job.id, job.agent_id
                )));
            }
        }
        if self.security.allowed_origins.is_empty() {
            return Err(EduMindError::ConfigValidation(
                "security.allowed_origins must contain at least one origin".to_owned(),
            ));
        }
        if self
            .security
            .action_password_hash_path
            .as_os_str()
            .is_empty()
        {
            return Err(EduMindError::ConfigValidation(
                "security.action_password_hash_path must not be empty".to_owned(),
            ));
        }
        if self.security.action_password_min_length < 8 {
            return Err(EduMindError::ConfigValidation(
                "security.action_password_min_length must be at least 8".to_owned(),
            ));
        }
        let execution = &self.security.execution;
        if execution.max_tool_timeout_secs == 0
            || execution.max_output_bytes == 0
            || execution.max_write_bytes == 0
            || execution.process_memory_limit_mb == Some(0)
        {
            return Err(EduMindError::ConfigValidation(
                "security.execution caps must be greater than zero when configured".to_owned(),
            ));
        }
        validate_unique_ids(
            self.models
                .providers
                .iter()
                .map(|provider| provider.id.as_str()),
            "model provider",
        )?;
        validate_unique_ids(
            self.agents.list.iter().map(|agent| agent.id.as_str()),
            "agent",
        )?;
        validate_external_tool("tools.latex_compile", &self.tools.latex_compile)?;
        validate_external_tool("tools.document_engine", &self.tools.document_engine)?;
        validate_external_tool("tools.slide_engine", &self.tools.slide_engine)?;
        validate_external_tool("tools.image_engine", &self.tools.image_engine)?;
        validate_notebooklm_config("tools.notebooklm", &self.tools.notebooklm)?;
        validate_notebooklm_config("tools.notebooklm_py", &self.tools.notebooklm_py)?;
        validate_ocr_config(&self.tools.ocr)?;
        Ok(())
    }

    /// Identifies settings that require a full gateway restart when changed.
    #[must_use]
    pub fn restart_required_changes(&self, next: &Self) -> Vec<&'static str> {
        let mut changes = Vec::new();
        if self.gateway.bind_address != next.gateway.bind_address
            || self.gateway.port != next.gateway.port
        {
            changes.push("gateway.bind_address_or_port");
        }
        if self.gateway.auth != next.gateway.auth {
            changes.push("gateway.auth");
        }
        if self.gateway.remote != next.gateway.remote {
            changes.push("gateway.remote");
        }
        if self.gateway.request_body_max_bytes != next.gateway.request_body_max_bytes {
            changes.push("gateway.request_body_max_bytes");
        }
        if self.channels != next.channels {
            changes.push("channels");
        }
        if self.memory != next.memory {
            changes.push("memory");
        }
        if self.session != next.session {
            changes.push("session");
        }
        if self.jobs != next.jobs {
            changes.push("jobs");
        }
        if self.tools != next.tools {
            changes.push("tools");
        }
        changes
    }
}

fn validate_unique_ids<'a>(values: impl Iterator<Item = &'a str>, kind: &str) -> Result<()> {
    let mut known = HashSet::new();
    for value in values {
        if value.trim().is_empty() {
            return Err(EduMindError::ConfigValidation(format!(
                "{kind} id must not be empty"
            )));
        }
        if !known.insert(value) {
            return Err(EduMindError::ConfigValidation(format!(
                "duplicate {kind} id `{value}`"
            )));
        }
    }
    Ok(())
}

fn validate_external_tool(name: &str, tool: &ExternalToolConfig) -> Result<()> {
    if tool.enabled
        && tool
            .command
            .as_deref()
            .is_none_or(|command| command.trim().is_empty())
    {
        return Err(EduMindError::ConfigValidation(format!(
            "{name}.command is required when {name}.enabled=true"
        )));
    }
    Ok(())
}

fn validate_ocr_config(config: &OcrConfig) -> Result<()> {
    if config.enabled
        && config
            .command
            .as_deref()
            .is_none_or(|command| command.trim().is_empty())
    {
        return Err(EduMindError::ConfigValidation(
            "tools.ocr.command is required when tools.ocr.enabled=true".to_owned(),
        ));
    }
    if config.language.trim().is_empty() || config.timeout_secs == 0 || config.min_text_chars == 0 {
        return Err(EduMindError::ConfigValidation(
            "tools.ocr language, timeout_secs, and min_text_chars must be greater than zero"
                .to_owned(),
        ));
    }
    Ok(())
}

fn validate_notebooklm_config(name: &str, config: &NotebookLmConfig) -> Result<()> {
    if config.timeout_secs == 0 {
        return Err(EduMindError::ConfigValidation(format!(
            "{name}.timeout_secs must be greater than zero"
        )));
    }
    if !config.enabled {
        return Ok(());
    }
    let endpoint = config
        .endpoint
        .as_deref()
        .map(str::trim)
        .filter(|endpoint| !endpoint.is_empty())
        .ok_or_else(|| {
            EduMindError::ConfigValidation(format!("{name}.endpoint is required when enabled"))
        })?;
    if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
        return Err(EduMindError::ConfigValidation(format!(
            "{name}.endpoint must use http:// or https://"
        )));
    }
    Ok(())
}

fn is_blank(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(|text| text.trim().is_empty())
}

fn is_loopback_bind(bind_address: &str) -> bool {
    let normalized = bind_address.trim().trim_matches(['[', ']']);
    normalized.eq_ignore_ascii_case("localhost")
        || normalized
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MetaConfig {
    pub name: String,
    #[serde(alias = "dataDir")]
    pub data_dir: PathBuf,
    #[serde(alias = "logLevel")]
    pub log_level: String,
}

impl Default for MetaConfig {
    fn default() -> Self {
        Self {
            name: "EduMind".to_owned(),
            data_dir: PathBuf::from("data"),
            log_level: "info".to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    #[serde(alias = "bindAddress")]
    pub bind_address: String,
    pub port: u16,
    #[serde(alias = "requestBodyMaxBytes")]
    pub request_body_max_bytes: usize,
    pub auth: AuthConfig,
    pub remote: RemoteConfig,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".to_owned(),
            port: 7878,
            request_body_max_bytes: 10 * 1024 * 1024,
            auth: AuthConfig::default(),
            remote: RemoteConfig::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    #[default]
    None,
    Token,
    Jwt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    pub mode: AuthMode,
    pub token: Option<String>,
    #[serde(alias = "jwtSecret")]
    pub jwt_secret: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            mode: AuthMode::None,
            token: None,
            jwt_secret: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteConfig {
    pub enabled: bool,
    pub endpoint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelsConfig {
    #[serde(alias = "defaultProvider")]
    pub default_provider: String,
    pub providers: Vec<ModelProviderConfig>,
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self {
            default_provider: "local".to_owned(),
            providers: vec![ModelProviderConfig::default()],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProviderKind {
    #[default]
    Ollama,
    OpenAiCompatible,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelProviderConfig {
    pub id: String,
    pub kind: ModelProviderKind,
    #[serde(alias = "baseUrl")]
    pub base_url: Option<String>,
    #[serde(alias = "apiKey")]
    pub api_key: Option<String>,
    pub models: Vec<ModelConfig>,
}

impl Default for ModelProviderConfig {
    fn default() -> Self {
        Self {
            id: "local".to_owned(),
            kind: ModelProviderKind::Ollama,
            base_url: Some("http://127.0.0.1:11434".to_owned()),
            api_key: None,
            models: vec![ModelConfig::default()],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    pub id: String,
    #[serde(alias = "contextWindow")]
    pub context_window: u32,
    #[serde(alias = "inputCostPerMillion")]
    pub input_cost_per_million: f64,
    #[serde(alias = "outputCostPerMillion")]
    pub output_cost_per_million: f64,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            id: "llama3.2".to_owned(),
            context_window: 32_768,
            input_cost_per_million: 0.0,
            output_cost_per_million: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentsConfig {
    pub defaults: AgentDefaultsConfig,
    pub list: Vec<AgentConfig>,
    pub sandbox: AgentSandboxConfig,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            defaults: AgentDefaultsConfig::default(),
            list: vec![AgentConfig::default()],
            sandbox: AgentSandboxConfig::default(),
        }
    }
}

/// Local-only control-file workspace available to configured desktop agents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSandboxConfig {
    pub enabled: bool,
    pub root: PathBuf,
    #[serde(alias = "maxControlFileBytes")]
    pub max_control_file_bytes: usize,
}

impl Default for AgentSandboxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            root: PathBuf::from("Sandbox"),
            max_control_file_bytes: 32 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentDefaultsConfig {
    #[serde(alias = "defaultAgent")]
    pub default_agent: String,
    #[serde(alias = "defaultModel")]
    pub default_model: String,
    #[serde(alias = "maxConcurrentRuns")]
    pub max_concurrent_runs: u16,
}

impl Default for AgentDefaultsConfig {
    fn default() -> Self {
        Self {
            default_agent: "master".to_owned(),
            default_model: "local/llama3.2".to_owned(),
            max_concurrent_runs: 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub model: Option<String>,
    pub workspace: Option<PathBuf>,
    #[serde(alias = "systemPrompt")]
    pub system_prompt: String,
    #[serde(alias = "allowedChannels")]
    pub allowed_channels: Vec<String>,
    #[serde(alias = "allowedTools")]
    pub allowed_tools: Vec<String>,
    #[serde(alias = "allowedSubagents")]
    pub allowed_subagents: Vec<String>,
    pub identity: String,
    #[serde(alias = "timeoutSecs")]
    pub timeout_secs: u64,
    #[serde(alias = "maxConcurrentRuns")]
    pub max_concurrent_runs: Option<u16>,
    #[serde(alias = "maxSpawnDepth")]
    pub max_spawn_depth: u8,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            id: "master".to_owned(),
            name: "Master Agent".to_owned(),
            enabled: true,
            model: None,
            workspace: None,
            system_prompt: "Coordinate EduMind study workflows safely.".to_owned(),
            allowed_channels: vec!["desktop".to_owned()],
            allowed_tools: Vec::new(),
            allowed_subagents: Vec::new(),
            identity: "EduMind study coordinator".to_owned(),
            timeout_secs: 120,
            max_concurrent_runs: None,
            max_spawn_depth: 2,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelsConfig {
    pub desktop: DesktopChannelConfig,
    pub telegram: TelegramChannelConfig,
}

impl ChannelsConfig {
    /// Validates standalone channel settings before channel services apply them.
    pub fn validate(&self) -> Result<()> {
        if self.desktop.enabled && self.desktop.account_id.trim().is_empty() {
            return Err(EduMindError::ConfigValidation(
                "channels.desktop.account_id is required when channels.desktop.enabled=true"
                    .to_owned(),
            ));
        }
        if self.telegram.poll_timeout_secs == 0 {
            return Err(EduMindError::ConfigValidation(
                "channels.telegram.poll_timeout_secs must be greater than zero".to_owned(),
            ));
        }
        if self.telegram.enabled
            && is_blank(&self.telegram.token)
            && is_blank(&self.telegram.token_secret_name)
        {
            return Err(EduMindError::ConfigValidation(
                "channels.telegram.token or channels.telegram.token_secret_name is required when channels.telegram.enabled=true"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoutingConfig {
    pub file: PathBuf,
    #[serde(alias = "hotReload")]
    pub hot_reload: bool,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            file: PathBuf::from("edumind/routing.yaml"),
            hot_reload: true,
        }
    }
}

impl RoutingConfig {
    /// Validates the route-file location before a router tries to load it.
    pub fn validate(&self) -> Result<()> {
        if self.file.as_os_str().is_empty() {
            return Err(EduMindError::ConfigValidation(
                "routing.file must not be empty".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopChannelConfig {
    pub enabled: bool,
    #[serde(alias = "accountId")]
    pub account_id: String,
}

impl Default for DesktopChannelConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            account_id: "desktop".to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramChannelConfig {
    pub enabled: bool,
    pub token: Option<String>,
    #[serde(alias = "tokenSecretName")]
    pub token_secret_name: Option<String>,
    #[serde(alias = "allowedUserIds")]
    pub allowed_user_ids: Vec<i64>,
    #[serde(alias = "allowedChatIds")]
    pub allowed_chat_ids: Vec<i64>,
    pub streaming: bool,
    #[serde(alias = "pollTimeoutSecs")]
    pub poll_timeout_secs: u16,
}

impl Default for TelegramChannelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            token: None,
            token_secret_name: None,
            allowed_user_ids: Vec::new(),
            allowed_chat_ids: Vec::new(),
            streaming: true,
            poll_timeout_secs: 30,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    #[serde(alias = "dbPath")]
    pub db_path: PathBuf,
    pub embedding: EmbeddingConfig,
    pub vector: VectorConfig,
    pub hermes: HermesConfig,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("data/memory.db"),
            embedding: EmbeddingConfig::default(),
            vector: VectorConfig::default(),
            hermes: HermesConfig::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingConfig {
    pub provider: String,
    pub model: String,
    pub dimensions: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: "hash".to_owned(),
            model: "hash-v1".to_owned(),
            dimensions: 384,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VectorConfig {
    #[serde(alias = "indexType")]
    pub index_type: String,
    pub dimensions: usize,
    #[serde(alias = "candidateCount")]
    pub candidate_count: usize,
}

impl Default for VectorConfig {
    fn default() -> Self {
        Self {
            index_type: "exact".to_owned(),
            dimensions: 384,
            candidate_count: 64,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HermesConfig {
    pub enabled: bool,
    #[serde(alias = "maxCyclesPerDay")]
    pub max_cycles_per_day: u16,
    #[serde(alias = "cooldownSecs")]
    pub cooldown_secs: u64,
}

impl Default for HermesConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_cycles_per_day: 8,
            cooldown_secs: 300,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginsConfig {
    pub directory: PathBuf,
    pub enabled: Vec<String>,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            directory: PathBuf::from("plugins"),
            enabled: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    pub profile: ToolProfile,
    #[serde(alias = "enforceAllowlist")]
    pub enforce_allowlist: bool,
    #[serde(alias = "rateLimitPerMinute")]
    pub rate_limit_per_minute: u32,
    #[serde(alias = "auditLogCapacity")]
    pub audit_log_capacity: usize,
    #[serde(alias = "maxToolRounds")]
    pub max_tool_rounds: u16,
    #[serde(alias = "latexCompile")]
    pub latex_compile: ExternalToolConfig,
    #[serde(alias = "documentEngine")]
    pub document_engine: ExternalToolConfig,
    #[serde(alias = "slideEngine")]
    pub slide_engine: ExternalToolConfig,
    #[serde(alias = "imageEngine")]
    pub image_engine: ExternalToolConfig,
    pub notebooklm: NotebookLmConfig,
    #[serde(alias = "notebooklmPy")]
    pub notebooklm_py: NotebookLmConfig,
    pub ocr: OcrConfig,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            profile: ToolProfile::Safe,
            enforce_allowlist: true,
            rate_limit_per_minute: 60,
            audit_log_capacity: 1_000,
            max_tool_rounds: 8,
            latex_compile: ExternalToolConfig::default(),
            document_engine: ExternalToolConfig::default(),
            slide_engine: ExternalToolConfig::default(),
            image_engine: ExternalToolConfig::default(),
            notebooklm: NotebookLmConfig::default(),
            notebooklm_py: NotebookLmConfig::default(),
            ocr: OcrConfig::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolProfile {
    #[default]
    Safe,
    Balanced,
    Full,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExternalToolConfig {
    pub enabled: bool,
    pub command: Option<String>,
}

/// OCR-specific configuration for optional scanned-PDF extraction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OcrConfig {
    pub enabled: bool,
    pub command: Option<String>,
    pub language: String,
    #[serde(alias = "timeoutSecs")]
    pub timeout_secs: u64,
    #[serde(alias = "minTextChars")]
    pub min_text_chars: usize,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            command: Some("ocrmypdf".to_owned()),
            language: "eng".to_owned(),
            timeout_secs: 300,
            min_text_chars: 200,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotebookLmConfig {
    pub enabled: bool,
    pub endpoint: Option<String>,
    #[serde(alias = "timeoutSecs")]
    pub timeout_secs: u64,
    #[serde(alias = "preferForAsk")]
    pub prefer_for_ask: bool,
    #[serde(alias = "fallbackToMcp")]
    pub fallback_to_mcp: bool,
}

impl Default for NotebookLmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: None,
            timeout_secs: 900,
            prefer_for_ask: true,
            fallback_to_mcp: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MessagesConfig {
    #[serde(alias = "assistantName")]
    pub assistant_name: String,
    #[serde(alias = "welcomeMessage")]
    pub welcome_message: String,
}

impl Default for MessagesConfig {
    fn default() -> Self {
        Self {
            assistant_name: "EduMind".to_owned(),
            welcome_message: "What would you like to learn today?".to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    #[serde(alias = "maxMessages")]
    pub max_messages: usize,
    #[serde(alias = "maxAgeMinutes")]
    pub max_age_minutes: u32,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_messages: 100,
            max_age_minutes: 240,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WebConfig {
    pub searxng: SearxngConfig,
    pub tavily: TavilyConfig,
    pub literature: LiteratureConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearxngConfig {
    pub enabled: bool,
    pub endpoint: Option<String>,
}

impl Default for SearxngConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: Some("http://127.0.0.1:8080".to_owned()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TavilyConfig {
    pub enabled: bool,
    #[serde(alias = "apiKey")]
    pub api_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LiteratureConfig {
    pub enabled: bool,
    #[serde(alias = "maxResults")]
    pub max_results: usize,
    pub sources: Vec<LiteratureSourceConfig>,
}

impl Default for LiteratureConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_results: 20,
            sources: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LiteratureSourceConfig {
    pub id: String,
    pub enabled: bool,
    pub endpoint: Option<String>,
}

impl Default for LiteratureSourceConfig {
    fn default() -> Self {
        Self {
            id: "arxiv".to_owned(),
            enabled: true,
            endpoint: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JobsConfig {
    pub enabled: bool,
    #[serde(alias = "maxConcurrentJobs")]
    pub max_concurrent_jobs: u16,
    pub schedules: Vec<ScheduledJobConfig>,
}

impl Default for JobsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent_jobs: 1,
            schedules: Vec::new(),
        }
    }
}

impl JobsConfig {
    /// Validates scheduler settings that do not depend on the agent registry.
    pub fn validate(&self) -> Result<()> {
        if self.max_concurrent_jobs == 0 {
            return Err(EduMindError::ConfigValidation(
                "jobs.max_concurrent_jobs must be greater than zero".to_owned(),
            ));
        }
        validate_unique_ids(
            self.schedules.iter().map(|job| job.id.as_str()),
            "scheduled job",
        )?;
        for job in &self.schedules {
            if job.interval_secs == 0 || job.interval_secs > i64::MAX as u64 {
                return Err(EduMindError::ConfigValidation(format!(
                    "jobs.schedules[{}].interval_secs must be between 1 and {}",
                    job.id,
                    i64::MAX
                )));
            }
            if job.agent_id.trim().is_empty()
                || job.session_key.trim().is_empty()
                || job.message.trim().is_empty()
            {
                return Err(EduMindError::ConfigValidation(format!(
                    "jobs.schedules[{}] requires agent_id, session_key, and message",
                    job.id
                )));
            }
        }
        Ok(())
    }
}

/// A recurring agent prompt injected by the gateway scheduler.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduledJobConfig {
    pub id: String,
    pub enabled: bool,
    #[serde(alias = "intervalSecs")]
    pub interval_secs: u64,
    #[serde(alias = "agentId")]
    pub agent_id: String,
    #[serde(alias = "sessionKey")]
    pub session_key: String,
    pub message: String,
    #[serde(alias = "runOnStartup")]
    pub run_on_startup: bool,
}

impl Default for ScheduledJobConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            enabled: true,
            interval_secs: 3_600,
            agent_id: "master".to_owned(),
            session_key: "scheduled:default".to_owned(),
            message: "Run the scheduled EduMind workflow.".to_owned(),
            run_on_startup: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SecurityConfig {
    #[serde(alias = "allowedOrigins")]
    pub allowed_origins: Vec<String>,
    #[serde(alias = "actionPasswordRequired")]
    pub action_password_required: bool,
    #[serde(alias = "actionPasswordHashPath")]
    pub action_password_hash_path: PathBuf,
    #[serde(alias = "actionPasswordMinLength")]
    pub action_password_min_length: usize,
    #[serde(alias = "restrictToolWrites")]
    pub restrict_tool_writes: bool,
    #[serde(
        alias = "allowed_write_roots",
        alias = "allowedWriteRoots",
        alias = "allowedToolWriteRoots"
    )]
    pub allowed_tool_write_roots: Vec<PathBuf>,
    #[serde(alias = "toolDailyLimits")]
    pub tool_daily_limits: ToolDailyLimitsConfig,
    pub execution: ExecutionCapsConfig,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            allowed_origins: vec![
                "http://localhost".to_owned(),
                "tauri://localhost".to_owned(),
            ],
            action_password_required: true,
            action_password_hash_path: PathBuf::from("data/action-password.argon2"),
            action_password_min_length: 12,
            restrict_tool_writes: true,
            allowed_tool_write_roots: Vec::new(),
            tool_daily_limits: ToolDailyLimitsConfig::default(),
            execution: ExecutionCapsConfig::default(),
        }
    }
}

/// Per-agent daily limits applied to all tool invocations and high-risk classes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolDailyLimitsConfig {
    #[serde(alias = "maxTotalPerDay")]
    pub max_total_per_day: u32,
    #[serde(alias = "maxNetworkPerDay")]
    pub max_network_per_day: u32,
    #[serde(alias = "maxExecutionPerDay")]
    pub max_execution_per_day: u32,
}

impl Default for ToolDailyLimitsConfig {
    fn default() -> Self {
        Self {
            max_total_per_day: 1_000,
            max_network_per_day: 250,
            max_execution_per_day: 50,
        }
    }
}

/// Upper bounds applied to all external tool and process execution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExecutionCapsConfig {
    #[serde(alias = "maxToolTimeoutSecs")]
    pub max_tool_timeout_secs: u64,
    #[serde(alias = "maxOutputBytes")]
    pub max_output_bytes: usize,
    #[serde(alias = "maxWriteBytes")]
    pub max_write_bytes: usize,
    #[serde(alias = "processMemoryLimitMb")]
    pub process_memory_limit_mb: Option<u64>,
    #[serde(alias = "windowsJobObjects")]
    pub windows_job_objects: bool,
}

impl Default for ExecutionCapsConfig {
    fn default() -> Self {
        Self {
            max_tool_timeout_secs: 120,
            max_output_bytes: 1_048_576,
            max_write_bytes: 10_485_760,
            process_memory_limit_mb: Some(1_024),
            windows_job_objects: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthMode, EduMindConfig};

    #[test]
    fn defaults_are_valid_for_loopback_development() {
        assert!(EduMindConfig::default().validate().is_ok());
    }

    #[test]
    fn unauthenticated_non_loopback_config_is_rejected() {
        let mut config = EduMindConfig::default();
        config.gateway.bind_address = "0.0.0.0".to_owned();
        config.gateway.auth.mode = AuthMode::None;

        assert!(config.validate().is_err());
    }

    #[test]
    fn restart_required_changes_classifies_gateway_updates() {
        let current = EduMindConfig::default();
        let mut next = current.clone();
        next.gateway.port = 9000;

        assert_eq!(
            current.restart_required_changes(&next),
            vec!["gateway.bind_address_or_port"]
        );
    }

    #[test]
    fn rejects_unsafe_execution_caps() {
        let mut config = EduMindConfig::default();
        config.security.execution.max_output_bytes = 0;

        assert!(config.validate().is_err());
    }
}
