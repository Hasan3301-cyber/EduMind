use std::env;

/// Resolves a portable Python executable without assuming a platform-specific path.
#[must_use]
pub fn python_executable() -> String {
    env::var("EDUMIND_PYTHON")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "python".to_owned()
            } else {
                "python3".to_owned()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::python_executable;

    #[test]
    fn resolves_a_non_empty_default() {
        assert!(!python_executable().trim().is_empty());
    }
}
