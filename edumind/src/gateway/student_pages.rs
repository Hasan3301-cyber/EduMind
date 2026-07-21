use axum::{Json, extract::State, http::StatusCode};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    gateway::AppState,
    infra::EduMindError,
    student::{PlannerSchedule, StudentPageRecordInput, StudentPageSnapshot},
};

type ApiError = (StatusCode, Json<Value>);
type ApiResult<T> = std::result::Result<Json<T>, ApiError>;

#[derive(Debug, Deserialize)]
pub struct GetStudentPageRequest {
    pub page: String,
}

#[derive(Debug, Deserialize)]
pub struct SaveStudentPageRequest {
    pub page: String,
    #[serde(default)]
    pub records: Vec<StudentPageRecordInput>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertStudentPageRecordRequest {
    pub page: String,
    pub record: StudentPageRecordInput,
    #[serde(default)]
    pub source: Option<String>,
}

/// Response returned after a canonical page save and semantic-index refresh.
#[derive(Clone, Debug, Serialize)]
pub struct SaveStudentPageResponse {
    pub snapshot: StudentPageSnapshot,
    pub indexed_memory_id: String,
}

/// Response returned after a single canonical record update and index refresh.
#[derive(Clone, Debug, Serialize)]
pub struct UpsertStudentPageRecordResponse {
    pub applied: bool,
    pub indexed_memory_id: String,
}

/// Loads a canonical Student OS or Student Planner state snapshot.
pub async fn get_page(
    State(state): State<AppState>,
    Json(request): Json<GetStudentPageRequest>,
) -> ApiResult<StudentPageSnapshot> {
    state
        .student_pages()
        .load(&request.page)
        .map(Json)
        .map_err(api_error)
}

/// Loads the canonical seven-day planner projection used by Routine workflows.
pub async fn planner_schedule(State(state): State<AppState>) -> ApiResult<PlannerSchedule> {
    state
        .student_pages()
        .planner_schedule()
        .map(Json)
        .map_err(api_error)
}

/// Saves a timestamp-aware canonical snapshot and refreshes its hybrid-memory representation.
pub async fn save_page(
    State(state): State<AppState>,
    Json(request): Json<SaveStudentPageRequest>,
) -> ApiResult<SaveStudentPageResponse> {
    let updated_at = request.updated_at.unwrap_or_else(Utc::now);
    let source = request.source.unwrap_or_else(|| "gateway".to_owned());
    let pages = state.student_pages();
    let snapshot = pages
        .save_all(&request.page, request.records, source, updated_at)
        .map_err(api_error)?;
    let indexed = pages
        .index_snapshot(&state.hybrid_memory(), &request.page, updated_at)
        .await
        .map_err(api_error)?;
    Ok(Json(SaveStudentPageResponse {
        snapshot,
        indexed_memory_id: indexed.id.to_string(),
    }))
}

/// Upserts one Student OS or Planner record without replacing the page snapshot.
pub async fn upsert_record(
    State(state): State<AppState>,
    Json(request): Json<UpsertStudentPageRecordRequest>,
) -> ApiResult<UpsertStudentPageRecordResponse> {
    let source = request.source.unwrap_or_else(|| "gateway".to_owned());
    let updated_at = request.record.updated_at;
    let pages = state.student_pages();
    let mutation = pages
        .upsert_record(&request.page, request.record, source)
        .map_err(api_error)?;
    let indexed = pages
        .index_snapshot(&state.hybrid_memory(), &request.page, updated_at)
        .await
        .map_err(api_error)?;
    Ok(Json(UpsertStudentPageRecordResponse {
        applied: mutation.applied,
        indexed_memory_id: indexed.id.to_string(),
    }))
}

fn api_error(error: EduMindError) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": {"code": "student_page_request_invalid", "message": error.to_string()},
        })),
    )
}

#[cfg(test)]
mod tests {
    use axum::{Json, extract::State};
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use super::{UpsertStudentPageRecordRequest, upsert_record};
    use crate::{config::EduMindConfig, gateway::AppState, student::StudentPageRecordInput};

    #[tokio::test]
    async fn upsert_preserves_other_canonical_page_records() {
        let state = AppState::in_memory(EduMindConfig::default()).unwrap();
        let updated_at = Utc.with_ymd_and_hms(2026, 7, 19, 10, 0, 0).unwrap();

        let Json(response) = upsert_record(
            State(state.clone()),
            Json(UpsertStudentPageRecordRequest {
                page: "student-os".to_owned(),
                record: StudentPageRecordInput::new(
                    "desktop-onboarding",
                    json!({"completed": true, "version": 1}),
                    updated_at,
                ),
                source: Some("desktop".to_owned()),
            }),
        )
        .await
        .unwrap();

        assert!(response.applied);
        assert!(!response.indexed_memory_id.is_empty());
        let snapshot = state.student_pages().load("student-os").unwrap();
        assert_eq!(snapshot.count, 1);
        assert_eq!(snapshot.records[0].key, "desktop-onboarding");
    }
}
