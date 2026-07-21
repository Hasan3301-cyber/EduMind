//! Config-driven agent execution, sessions, tool policy, and audit primitives.

pub mod audit;
pub mod capabilities;
pub mod model;
pub mod registry;
pub mod run_engine;
pub mod runner;
pub mod sandbox;
pub mod session;
pub mod subagent;
pub mod tool_policy;
pub mod tools;

pub use audit::{ToolAuditEntry, ToolAuditLog, ToolAuditOutcome, ToolRateLimiter};
pub use capabilities::{
    CapabilityAuthority, CapabilityCheck, CapabilityGrant, CapabilityGrantRequest,
};
pub use model::{ModelReference, ModelResolver, ResolvedModel};
pub use registry::{AgentProfile, AgentRegistry, AgentRunLimiter};
pub use run_engine::{
    CancellationRegistry, PlanExecuteVerifyCommitEngine, RunStage, RunStageResult,
};
pub use runner::{
    AgentModel, AgentRunRequest, AgentRunResult, AgentRunner, BuiltinAgentToolExecutor,
    ModelRequest, ModelResponse, ToolExecution, ToolExecutionContext, ToolExecutor, TransientImage,
};
pub use session::{ChatRole, Session, SessionManager, SessionMessage};
pub use subagent::{SubagentRegistry, SubagentTicket};
pub use tool_policy::{ToolPolicy, ToolPolicyDecision, ToolPolicyDenial};
pub use tools::{ToolCall, ToolClass, ToolDef, ToolRegistry};
