use axum::{Json, extract::State, http::StatusCode};
use serde_json::{Value, json};

use crate::{gateway::AppState, infra::EduMindError, runtime_tools::RuntimeToolsStatus};

type ApiError = (StatusCode, Json<Value>);
type ApiResult<T> = std::result::Result<Json<T>, ApiError>;

/// Returns safe local runtime capability status for desktop administration.
pub async fn status(State(state): State<AppState>) -> ApiResult<RuntimeToolsStatus> {
    Ok(Json(state.runtime_tools().status()))
}

/// Runs the configured NotebookLM health probe through its preferred local integration.
pub async fn notebooklm_health(State(state): State<AppState>) -> ApiResult<Value> {
    state
        .runtime_tools()
        .notebooklm_health()
        .await
        .map(Json)
        .map_err(api_error)
}

fn api_error(error: EduMindError) -> ApiError {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "runtime_unavailable",
            "message": error.to_string(),
        })),
    )
}
