use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::Utc;
use edumind_core::{PipelineRunId, RunBudget, RunCheckpoint, RunTimelineEvent, RunVerification};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    gateway::{AppState, EventFrame},
    infra::EduMindError,
};

type ApiError = (StatusCode, Json<Value>);
type ApiResult<T> = std::result::Result<Json<T>, ApiError>;

/// Response returned after a run cancellation is durably accepted.
#[derive(Clone, Debug, Serialize)]
pub struct RunCancellationResponse {
    pub run_id: PipelineRunId,
    pub cancelled: bool,
}

/// Durable evidence and recovery records for one pipeline run.
#[derive(Clone, Debug, Serialize)]
pub struct RunEvidenceResponse {
    pub run_id: PipelineRunId,
    pub budget: Option<RunBudget>,
    pub checkpoints: Vec<RunCheckpoint>,
    pub verifications: Vec<RunVerification>,
}

/// Cancels a pending or active run before it can start another stage.
pub async fn cancel_run(
    State(state): State<AppState>,
    Path(raw_run_id): Path<String>,
) -> ApiResult<RunCancellationResponse> {
    let run_id = parse_run_id(&raw_run_id)?;
    state
        .run_cancellations()
        .cancel(run_id)
        .map_err(api_error)?;
    let event = RunTimelineEvent::new(
        run_id,
        "run_cancelled",
        "Cancellation requested by the user.",
        Utc::now(),
    );
    state
        .run_store()
        .append_timeline_event(&event)
        .map_err(api_error)?;
    state.publish(EventFrame::new(
        "run.cancelled",
        json!({"run_id": run_id, "timestamp": event.at}),
    ));
    Ok(Json(RunCancellationResponse {
        run_id,
        cancelled: true,
    }))
}

/// Returns durable timeline records for a run, including a completed cancellation request.
pub async fn timeline(
    State(state): State<AppState>,
    Path(raw_run_id): Path<String>,
) -> ApiResult<Vec<RunTimelineEvent>> {
    let run_id = parse_run_id(&raw_run_id)?;
    state
        .run_store()
        .timeline(run_id)
        .map(Json)
        .map_err(api_error)
}

/// Returns stage checkpoints and verification evidence for a run.
pub async fn evidence(
    State(state): State<AppState>,
    Path(raw_run_id): Path<String>,
) -> ApiResult<RunEvidenceResponse> {
    let run_id = parse_run_id(&raw_run_id)?;
    let store = state.run_store();
    Ok(Json(RunEvidenceResponse {
        run_id,
        budget: store.budget(run_id).map_err(api_error)?,
        checkpoints: store.checkpoints(run_id).map_err(api_error)?,
        verifications: store.verifications(run_id).map_err(api_error)?,
    }))
}

fn parse_run_id(value: &str) -> std::result::Result<PipelineRunId, ApiError> {
    Uuid::parse_str(value).map(PipelineRunId).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "invalid_run_id",
                    "message": format!("The run ID is invalid: {}", error),
                }
            })),
        )
    })
}

fn api_error(error: EduMindError) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": {
                "code": "run_request_failed",
                "message": error.to_string(),
            }
        })),
    )
}

#[cfg(test)]
mod tests {
    use axum::extract::{Path, State};
    use edumind_core::PipelineRunId;

    use crate::{
        config::EduMindConfig,
        gateway::{
            AppState,
            runs::{cancel_run, timeline},
        },
    };

    #[tokio::test]
    async fn cancellation_is_persisted_and_broadcast() {
        let state = AppState::in_memory(EduMindConfig::default()).unwrap();
        let run_id = PipelineRunId::new();
        let mut events = state.subscribe();

        let response = cancel_run(State(state.clone()), Path(run_id.0.to_string()))
            .await
            .unwrap()
            .0;
        assert!(response.cancelled);

        let timeline = timeline(State(state), Path(run_id.0.to_string()))
            .await
            .unwrap()
            .0;
        assert_eq!(timeline.len(), 1);
        assert_eq!(events.recv().await.unwrap().event, "run.cancelled");
    }
}
