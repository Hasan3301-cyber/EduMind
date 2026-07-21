use std::{
    ffi::OsString,
    fs,
    io::ErrorKind,
    path::{Component, Path, PathBuf},
};

use crate::{
    config::types::SecurityConfig,
    infra::{EduMindError, Result},
};

/// Resolves and enforces the configured roots permitted for tool-created files.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteSandbox {
    restrict_writes: bool,
    allowed_roots: Vec<PathBuf>,
    max_write_bytes: usize,
}

impl WriteSandbox {
    /// Creates a sandbox from active security configuration without creating any directories.
    #[must_use]
    pub fn from_config(config: &SecurityConfig) -> Self {
        Self {
            restrict_writes: config.restrict_tool_writes,
            allowed_roots: config.allowed_tool_write_roots.clone(),
            max_write_bytes: config.execution.max_write_bytes,
        }
    }

    /// Validates a destination and payload length, resolving existing symlinks before allowing it.
    pub fn authorize(&self, target: impl AsRef<Path>, payload_len: usize) -> Result<PathBuf> {
        if payload_len > self.max_write_bytes {
            return Err(EduMindError::Security(format!(
                "tool write exceeds the {} byte limit",
                self.max_write_bytes
            )));
        }
        let target = normalize_absolute(target.as_ref())?;
        if !self.restrict_writes {
            return Ok(target);
        }
        let target = resolve_existing_ancestor(&target)?;
        let mut permitted = false;
        for root in &self.allowed_roots {
            let root = resolve_existing_ancestor(&normalize_absolute(root)?)?;
            if target.starts_with(root) {
                permitted = true;
                break;
            }
        }
        if permitted {
            Ok(target)
        } else {
            Err(EduMindError::Security(
                "tool write destination is outside the allowed write roots".to_owned(),
            ))
        }
    }

    /// Creates parent directories and writes bytes only after two sandbox checks.
    pub fn write(&self, target: impl AsRef<Path>, contents: &[u8]) -> Result<PathBuf> {
        let target = self.authorize(target, contents.len())?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| EduMindError::StorageIo {
                path: parent.to_path_buf(),
                source: error,
            })?;
        }
        let target = self.authorize(&target, contents.len())?;
        fs::write(&target, contents).map_err(|error| EduMindError::StorageIo {
            path: target.clone(),
            source: error,
        })?;
        Ok(target)
    }
}

fn normalize_absolute(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                EduMindError::Security(format!("failed to resolve current directory: {error}"))
            })?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(EduMindError::Security(
                        "tool path escapes its filesystem root".to_owned(),
                    ));
                }
            }
            Component::Normal(segment) => normalized.push(segment),
        }
    }
    Ok(normalized)
}

fn resolve_existing_ancestor(path: &Path) -> Result<PathBuf> {
    let mut existing = path.to_path_buf();
    let mut missing = Vec::<OsString>::new();
    loop {
        match fs::symlink_metadata(&existing) {
            Ok(_) => break,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let segment = existing.file_name().map(OsString::from).ok_or_else(|| {
                    EduMindError::Security(
                        "tool path has no resolvable existing ancestor".to_owned(),
                    )
                })?;
                missing.push(segment);
                if !existing.pop() {
                    return Err(EduMindError::Security(
                        "tool path has no resolvable existing ancestor".to_owned(),
                    ));
                }
            }
            Err(error) => {
                return Err(EduMindError::Security(format!(
                    "failed to inspect tool path: {error}"
                )));
            }
        }
    }
    let mut resolved = fs::canonicalize(&existing).map_err(|error| {
        EduMindError::Security(format!("failed to resolve tool path symlinks: {error}"))
    })?;
    for segment in missing.iter().rev() {
        resolved.push(segment);
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use uuid::Uuid;

    use super::WriteSandbox;
    use crate::config::types::{ExecutionCapsConfig, SecurityConfig};

    fn sandbox(base: &std::path::Path, max_write_bytes: usize) -> WriteSandbox {
        let config = SecurityConfig {
            allowed_tool_write_roots: vec![base.join("allowed")],
            execution: ExecutionCapsConfig {
                max_write_bytes,
                ..ExecutionCapsConfig::default()
            },
            ..SecurityConfig::default()
        };
        WriteSandbox::from_config(&config)
    }

    #[test]
    fn permits_allowed_writes_and_rejects_traversal_or_oversized_payloads() {
        let base = env::temp_dir().join(format!("edumind-write-sandbox-{}", Uuid::new_v4()));
        let sandbox = sandbox(&base, 4);
        let allowed = base.join("allowed").join("nested").join("note.txt");

        sandbox.write(&allowed, b"note").unwrap();
        assert_eq!(fs::read_to_string(&allowed).unwrap(), "note");
        assert!(
            sandbox
                .authorize(base.join("allowed").join("..").join("escape.txt"), 1)
                .is_err()
        );
        assert!(sandbox.authorize(&allowed, 5).is_err());

        fs::remove_dir_all(base).unwrap();
    }
}
