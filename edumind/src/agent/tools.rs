use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::infra::{EduMindError, Result};

/// The execution capability needed by a tool definition.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolClass {
    Read,
    Write,
    Network,
    Execution,
}

/// JSON-schema-like metadata for a tool that an agent can request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub class: ToolClass,
}

impl ToolDef {
    /// Creates a generic object-argument tool definition.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        class: ToolClass,
        required_arguments: &[&str],
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters: json!({
                "type": "object",
                "additionalProperties": true,
                "required": required_arguments,
            }),
            class,
        }
    }
}

/// A structured model-requested tool call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    #[serde(default = "default_arguments")]
    pub arguments: Value,
}

impl ToolCall {
    /// Creates a call with the provided JSON object arguments.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, arguments: Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments,
        }
    }

    /// Ensures the call has a stable ID, tool name, and object-shaped arguments.
    pub fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() || self.name.trim().is_empty() {
            return Err(EduMindError::Tool(
                "tool calls require non-empty id and name fields".to_owned(),
            ));
        }
        if !self.arguments.is_object() {
            return Err(EduMindError::Tool(format!(
                "tool call `{}` arguments must be a JSON object",
                self.name
            )));
        }
        Ok(())
    }
}

/// Deterministic registry of tools known to the agent runtime.
#[derive(Clone, Debug, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, ToolDef>,
}

impl ToolRegistry {
    /// Builds the complete Phase 5 tool-name contract.
    #[must_use]
    pub fn standard() -> Self {
        Self {
            tools: standard_definitions()
                .into_iter()
                .map(|definition| (definition.name.clone(), definition))
                .collect(),
        }
    }

    /// Registers an additional tool while rejecting malformed or duplicate definitions.
    pub fn register(&mut self, definition: ToolDef) -> Result<()> {
        if definition.name.trim().is_empty() {
            return Err(EduMindError::Tool(
                "tool definitions require a non-empty name".to_owned(),
            ));
        }
        if !definition.parameters.is_object() {
            return Err(EduMindError::Tool(format!(
                "tool definition `{}` parameters must be a JSON object",
                definition.name
            )));
        }
        if self.tools.contains_key(&definition.name) {
            return Err(EduMindError::Tool(format!(
                "tool `{}` is already registered",
                definition.name
            )));
        }
        self.tools.insert(definition.name.clone(), definition);
        Ok(())
    }

    /// Returns a copy of one tool definition by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<ToolDef> {
        self.tools.get(name).cloned()
    }

    /// Returns all definitions in deterministic tool-name order.
    #[must_use]
    pub fn all(&self) -> Vec<ToolDef> {
        self.tools.values().cloned().collect()
    }
}

fn default_arguments() -> Value {
    Value::Object(serde_json::Map::new())
}

fn standard_definitions() -> Vec<ToolDef> {
    vec![
        definition(
            "bash",
            "Run a sandboxed shell command.",
            ToolClass::Execution,
            &["command"],
        ),
        definition("read", "Read a sandboxed file.", ToolClass::Read, &["path"]),
        definition(
            "write",
            "Write a sandboxed file.",
            ToolClass::Write,
            &["path", "content"],
        ),
        definition(
            "memory_search",
            "Search persistent memory.",
            ToolClass::Read,
            &["query"],
        ),
        definition(
            "memory_get",
            "Get one persistent memory record.",
            ToolClass::Read,
            &["id"],
        ),
        definition(
            "memory_store",
            "Store a persistent memory record.",
            ToolClass::Write,
            &["content"],
        ),
        definition(
            "module_memory_search",
            "Search module memory.",
            ToolClass::Read,
            &["module_id", "query"],
        ),
        definition(
            "module_memory_get",
            "Get a module memory record.",
            ToolClass::Read,
            &["module_id", "id"],
        ),
        definition(
            "module_memory_store",
            "Store module memory.",
            ToolClass::Write,
            &["module_id", "content"],
        ),
        definition(
            "module_memory_summary",
            "Summarize module memory.",
            ToolClass::Read,
            &["module_id"],
        ),
        definition(
            "srs_card_create",
            "Create a spaced-repetition card.",
            ToolClass::Write,
            &["front", "back"],
        ),
        definition(
            "srs_generate_from_notes",
            "Generate cards from notes.",
            ToolClass::Write,
            &["notes"],
        ),
        definition(
            "srs_due",
            "List due spaced-repetition cards.",
            ToolClass::Read,
            &[],
        ),
        definition(
            "srs_review",
            "Record a spaced-repetition review.",
            ToolClass::Write,
            &["card_id", "rating"],
        ),
        definition(
            "srs_stats",
            "Read spaced-repetition statistics.",
            ToolClass::Read,
            &[],
        ),
        definition(
            "wiki_search",
            "Search the local or remote knowledge wiki.",
            ToolClass::Read,
            &["query"],
        ),
        definition(
            "graph_search",
            "Search the knowledge graph.",
            ToolClass::Read,
            &["query"],
        ),
        definition(
            "graph_neighbors",
            "Read knowledge-graph neighbors.",
            ToolClass::Read,
            &["node_id"],
        ),
        definition(
            "student_page_get",
            "Get Student OS or Planner state.",
            ToolClass::Read,
            &["page"],
        ),
        definition(
            "student_planner_schedule",
            "Read the canonical seven-day Student Planner schedule for Routine workflows instead of OCR.",
            ToolClass::Read,
            &[],
        ),
        definition(
            "student_page_upsert",
            "Create or update Student OS or Planner state.",
            ToolClass::Write,
            &["page", "key", "value"],
        ),
        definition(
            "student_page_delete",
            "Delete Student OS or Planner state.",
            ToolClass::Write,
            &["page", "key"],
        ),
        definition(
            "message_send",
            "Send a message through an approved channel.",
            ToolClass::Network,
            &["channel", "message"],
        ),
        definition(
            "run_subagent",
            "Schedule a configured subagent run.",
            ToolClass::Write,
            &["agent_id", "session_key"],
        ),
        definition(
            "sessions_spawn",
            "Create a child conversation session.",
            ToolClass::Write,
            &["agent_id", "session_key"],
        ),
        definition(
            "notebooklm_ask",
            "Ask an authorized NotebookLM notebook.",
            ToolClass::Network,
            &["question"],
        ),
        definition(
            "notebooklm_setup_auth",
            "Start NotebookLM authentication.",
            ToolClass::Network,
            &[],
        ),
        definition(
            "notebooklm_add_notebook",
            "Register a NotebookLM notebook.",
            ToolClass::Network,
            &["notebook_id"],
        ),
        definition(
            "notebooklm_add_source",
            "Add a source to NotebookLM.",
            ToolClass::Network,
            &["source"],
        ),
        definition(
            "notebooklm_list_notebooks",
            "List authorized NotebookLM notebooks.",
            ToolClass::Network,
            &[],
        ),
        definition(
            "notebooklm_select_notebook",
            "Select a NotebookLM notebook.",
            ToolClass::Network,
            &["notebook_id"],
        ),
        definition(
            "notebooklm_get_health",
            "Get NotebookLM integration health.",
            ToolClass::Network,
            &[],
        ),
        definition(
            "web_search",
            "Search approved web providers.",
            ToolClass::Network,
            &["query"],
        ),
        definition(
            "scholar_search",
            "Search approved academic providers.",
            ToolClass::Network,
            &["query"],
        ),
        definition(
            "pdf_extract_text",
            "Extract text from a PDF.",
            ToolClass::Read,
            &["path"],
        ),
        definition(
            "pdf_analyze",
            "Analyze a PDF or extracted text.",
            ToolClass::Read,
            &["path"],
        ),
        definition(
            "research_run",
            "Run a scoped research workflow.",
            ToolClass::Network,
            &["query"],
        ),
        definition(
            "research_validate_claims",
            "Validate research claims.",
            ToolClass::Read,
            &["claims"],
        ),
        definition(
            "research_literature_graph",
            "Build a literature graph.",
            ToolClass::Read,
            &["request"],
        ),
        definition(
            "research_project_ask",
            "Ask a persisted research project.",
            ToolClass::Read,
            &["project_id", "question"],
        ),
        definition(
            "research_ingest",
            "Ingest research material.",
            ToolClass::Write,
            &["project_id", "source"],
        ),
        definition(
            "research_deep_ask",
            "Ask grounded questions over full text.",
            ToolClass::Read,
            &["project_id", "question"],
        ),
        definition(
            "research_gaps",
            "Find stated research gaps.",
            ToolClass::Read,
            &["project_id"],
        ),
        definition(
            "research_supervise",
            "Build a research supervision report.",
            ToolClass::Read,
            &["project_id"],
        ),
        definition(
            "doc_create",
            "Create a document.",
            ToolClass::Write,
            &["title"],
        ),
        definition("doc_view", "View a document.", ToolClass::Read, &["id"]),
        definition("doc_list", "List documents.", ToolClass::Read, &[]),
        definition(
            "doc_modify",
            "Modify a document.",
            ToolClass::Write,
            &["id"],
        ),
        definition(
            "doc_convert",
            "Convert a document.",
            ToolClass::Execution,
            &["id", "format"],
        ),
        definition(
            "doc_restore",
            "Restore a document version.",
            ToolClass::Write,
            &["id", "version"],
        ),
        definition(
            "slide_create",
            "Create a slide deck.",
            ToolClass::Write,
            &["title"],
        ),
        definition("slide_read", "Read a slide deck.", ToolClass::Read, &["id"]),
        definition(
            "slide_delete",
            "Delete a slide.",
            ToolClass::Write,
            &["id", "slide"],
        ),
        definition("slide_insert", "Insert a slide.", ToolClass::Write, &["id"]),
        definition(
            "slide_theme",
            "Apply a slide theme.",
            ToolClass::Write,
            &["id", "theme"],
        ),
        definition(
            "slide_screenshot",
            "Render a slide screenshot.",
            ToolClass::Execution,
            &["id"],
        ),
        definition(
            "slide_check_overflow",
            "Check deck overflow.",
            ToolClass::Read,
            &["id"],
        ),
        definition(
            "slide_check",
            "Check a slide deck.",
            ToolClass::Read,
            &["id"],
        ),
        definition(
            "slide_restore_snapshot",
            "Restore a slide snapshot.",
            ToolClass::Write,
            &["id", "snapshot"],
        ),
        definition(
            "slide_thumbnail_grid",
            "Build a slide thumbnail grid.",
            ToolClass::Read,
            &["id"],
        ),
        definition("slide_list", "List slide decks.", ToolClass::Read, &[]),
        definition(
            "slide_build_pptx",
            "Build a PPTX artifact.",
            ToolClass::Execution,
            &["id"],
        ),
        definition(
            "image_search",
            "Search image sources.",
            ToolClass::Network,
            &["query"],
        ),
        definition(
            "image_download",
            "Download an approved image.",
            ToolClass::Network,
            &["url"],
        ),
        definition(
            "image_ensure_raster",
            "Convert image data to raster.",
            ToolClass::Execution,
            &["path"],
        ),
        definition(
            "image_generate",
            "Generate an image.",
            ToolClass::Network,
            &["prompt"],
        ),
        definition(
            "latex_compile",
            "Compile a LaTeX document.",
            ToolClass::Execution,
            &["path"],
        ),
    ]
}

fn definition(
    name: &str,
    description: &str,
    class: ToolClass,
    required_arguments: &[&str],
) -> ToolDef {
    ToolDef::new(name, description, class, required_arguments)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ToolCall, ToolClass, ToolDef, ToolRegistry};

    #[test]
    fn standard_registry_contains_the_required_agent_tools() {
        let registry = ToolRegistry::standard();

        assert_eq!(registry.get("bash").unwrap().class, ToolClass::Execution);
        assert!(registry.get("run_subagent").is_some());
        assert!(registry.get("sessions_spawn").is_some());
        assert!(registry.get("notebooklm_ask").is_some());
        assert!(registry.get("student_planner_schedule").is_some());
    }

    #[test]
    fn rejects_duplicate_definitions_and_non_object_arguments() {
        let mut registry = ToolRegistry::standard();
        assert!(
            registry
                .register(ToolDef::new("read", "duplicate", ToolClass::Read, &[]))
                .is_err()
        );
        assert!(
            ToolCall::new("call-1", "read", json!("invalid"))
                .validate()
                .is_err()
        );
    }
}
