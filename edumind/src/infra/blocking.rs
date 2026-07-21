use crate::infra::{EduMindError, Result};

/// Runs CPU-bound or blocking work without occupying Tokio's async workers.
pub async fn run_blocking<T, F>(operation: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| EduMindError::BlockingTask(error.to_string()))?
}

#[cfg(test)]
mod tests {
    use crate::infra::run_blocking;

    #[tokio::test]
    async fn returns_the_blocking_operation_result() {
        let value = run_blocking(|| Ok::<_, crate::infra::EduMindError>(42)).await;

        assert_eq!(value.unwrap(), 42);
    }
}
