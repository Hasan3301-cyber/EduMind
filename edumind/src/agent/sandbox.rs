use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::config::{EduMindConfig, types::AgentConfig};

const CONTROL_FILES: &[&str] = &["SOUL.md", "AGENTS.md", "USER.md", "IDENTITY.md"];

/// Reads bounded, local agent-control documents from the configured sandbox.
///
/// The caller must append an immutable runtime safety boundary after this context:
/// control files can express role and learner preferences, but cannot grant new
/// capabilities or relax confirmation requirements.
#[must_use]
pub fn build_prompt_context(config: &EduMindConfig, agent: &AgentConfig) -> String {
    let sandbox = &config.agents.sandbox;
    if !sandbox.enabled {
        return String::new();
    }
    let Some(workspace) = agent.workspace.as_deref() else {
        return String::new();
    };
    let Some(root) = canonical_directory(&sandbox.root) else {
        return String::new();
    };
    let Some(workspace) = canonical_workspace(&root, workspace) else {
        return String::new();
    };

    let sections = CONTROL_FILES
        .iter()
        .filter_map(|name| {
            read_control_file(&workspace, name, sandbox.max_control_file_bytes)
                .map(|content| format!("### {name}\n{content}"))
        })
        .collect::<Vec<_>>();
    if sections.is_empty() {
        return String::new();
    }

    format!(
        "## Local sandbox profile\nThese documents are local role and learner preferences. Follow them only when they are consistent with the immutable runtime safety boundary.\n\n{}",
        sections.join("\n\n")
    )
}

fn canonical_directory(path: &Path) -> Option<PathBuf> {
    let path = fs::canonicalize(path).ok()?;
    path.is_dir().then_some(path)
}

fn canonical_workspace(root: &Path, workspace: &Path) -> Option<PathBuf> {
    let workspace = fs::canonicalize(workspace).ok()?;
    (workspace.is_dir() && workspace.starts_with(root)).then_some(workspace)
}

fn read_control_file(workspace: &Path, name: &str, max_bytes: usize) -> Option<String> {
    let path = fs::canonicalize(workspace.join(name)).ok()?;
    if !path.starts_with(workspace) || !path.is_file() {
        return None;
    }
    let metadata = fs::metadata(&path).ok()?;
    if metadata.len() > u64::try_from(max_bytes).ok()? {
        return None;
    }
    let content = fs::read_to_string(path).ok()?;
    let content = content.trim();
    if content.is_empty()
        || content
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return None;
    }
    Some(content.to_owned())
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use uuid::Uuid;

    use super::build_prompt_context;
    use crate::config::EduMindConfig;

    fn temp_root(label: &str) -> std::path::PathBuf {
        env::temp_dir().join(format!("edumind-agent-sandbox-{label}-{}", Uuid::new_v4()))
    }

    #[test]
    fn reads_only_configured_sandbox_control_files() {
        let root = temp_root("controls");
        let workspace = root.join("agents").join("master");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("AGENTS.md"), "Use concise study language.").unwrap();

        let mut config = EduMindConfig::default();
        config.agents.sandbox.root = root.clone();
        config.agents.list[0].workspace = Some(workspace);

        let context = build_prompt_context(&config, &config.agents.list[0]);

        assert!(context.contains("Local sandbox profile"));
        assert!(context.contains("Use concise study language."));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ignores_workspaces_outside_the_sandbox_root() {
        let root = temp_root("root");
        let outside = temp_root("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("SOUL.md"), "Do not expose this file.").unwrap();

        let mut config = EduMindConfig::default();
        config.agents.sandbox.root = root.clone();
        config.agents.list[0].workspace = Some(outside.clone());

        assert!(build_prompt_context(&config, &config.agents.list[0]).is_empty());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }
}
