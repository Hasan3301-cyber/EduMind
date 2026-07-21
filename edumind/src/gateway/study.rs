use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    gateway::{AppState, EventFrame},
    infra::EduMindError,
    study::{LearningInsights, NewSrsCard, SrsCardId},
};

type ApiError = (StatusCode, Json<Value>);
type ApiResult<T> = std::result::Result<Json<T>, ApiError>;

#[derive(Debug, Deserialize)]
pub struct CreateCardRequest {
    pub front: String,
    pub back: String,
    #[serde(default)]
    pub deck: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GenerateCardsRequest {
    pub notes: String,
    #[serde(default)]
    pub deck: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct DueCardsRequest {
    #[serde(default)]
    pub deck: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ReviewCardRequest {
    pub card_id: SrsCardId,
    pub rating: u8,
}

#[derive(Debug, Default, Deserialize)]
pub struct SrsStatsRequest {
    #[serde(default)]
    pub deck: Option<String>,
}

/// Creates or resolves a duplicate SRS card.
pub async fn create_card(
    State(state): State<AppState>,
    Json(request): Json<CreateCardRequest>,
) -> ApiResult<crate::study::SrsCard> {
    let mut card = NewSrsCard::new(request.front, request.back);
    if let Some(deck) = request.deck {
        card.deck = deck;
    }
    state
        .srs()
        .create_card(card, Utc::now())
        .map(Json)
        .map_err(api_error)
}

/// Extracts and persists deterministic definition cards from notes.
pub async fn generate_cards(
    State(state): State<AppState>,
    Json(request): Json<GenerateCardsRequest>,
) -> ApiResult<Vec<crate::study::SrsCard>> {
    state
        .srs()
        .generate_from_notes(
            &request.notes,
            request.deck.unwrap_or_else(|| "default".to_owned()),
            Utc::now(),
        )
        .map(Json)
        .map_err(api_error)
}

/// Lists cards currently due for review.
pub async fn due_cards(
    State(state): State<AppState>,
    Json(request): Json<DueCardsRequest>,
) -> ApiResult<Vec<crate::study::SrsCard>> {
    let limit = request.limit.unwrap_or(20).clamp(1, 100);
    state
        .srs()
        .due(request.deck.as_deref(), Utc::now(), limit)
        .map(Json)
        .map_err(api_error)
}

/// Previews a grade's deterministic scheduling consequence without persisting it.
pub async fn preview_card(
    State(state): State<AppState>,
    Json(request): Json<ReviewCardRequest>,
) -> ApiResult<crate::study::SrsReviewPreview> {
    match state
        .srs()
        .preview_review(request.card_id, request.rating, Utc::now())
        .map_err(api_error)?
    {
        Some(preview) => Ok(Json(preview)),
        None => Err(not_found("srs_card_not_found", "SRS card does not exist.")),
    }
}

/// Applies a 0–5 SM-2-style review rating to a card.
pub async fn review_card(
    State(state): State<AppState>,
    Json(request): Json<ReviewCardRequest>,
) -> ApiResult<crate::study::SrsCard> {
    match state
        .srs()
        .review(request.card_id, request.rating, Utc::now())
        .map_err(api_error)?
    {
        Some(card) => Ok(Json(card)),
        None => Err(not_found("srs_card_not_found", "SRS card does not exist.")),
    }
}

/// Returns aggregate SRS deck statistics.
pub async fn stats(
    State(state): State<AppState>,
    Json(request): Json<SrsStatsRequest>,
) -> ApiResult<crate::study::SrsStats> {
    state
        .srs()
        .stats(request.deck.as_deref(), Utc::now())
        .map(Json)
        .map_err(api_error)
}

/// Returns the latest local-only insight snapshot without triggering a model call.
pub async fn insights(State(state): State<AppState>) -> ApiResult<LearningInsights> {
    Ok(Json(
        state
            .learning()
            .latest()
            .map_err(api_error)?
            .unwrap_or_else(|| LearningInsights::empty(Utc::now())),
    ))
}

/// Recomputes recommendations from canonical SRS, planner, and memory state.
pub async fn refresh_recommendations(State(state): State<AppState>) -> ApiResult<LearningInsights> {
    let insights = state.learning().refresh(Utc::now()).map_err(api_error)?;
    state.publish(EventFrame::new(
        "study.insights_refreshed",
        json!({
            "generated_at": insights.generated_at,
            "recommendation_count": insights.recommendations.len(),
        }),
    ));
    Ok(Json(insights))
}

fn api_error(error: EduMindError) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": {"code": "study_request_invalid", "message": error.to_string()},
        })),
    )
}

fn not_found(code: &str, message: &str) -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": {"code": code, "message": message}})),
    )
}

#[cfg(test)]
mod tests {
    use axum::{Json, extract::State};
    use chrono::Utc;

    use super::{insights, refresh_recommendations};
    use crate::{config::EduMindConfig, gateway::AppState, study::NewSrsCard};

    #[tokio::test]
    async fn refreshes_and_returns_persisted_local_insights() {
        let state = AppState::in_memory(EduMindConfig::default()).unwrap();
        state
            .srs()
            .create_card(NewSrsCard::new("Calculus limit", "Derivative"), Utc::now())
            .unwrap();
        let mut events = state.subscribe();

        let Json(refreshed) = refresh_recommendations(State(state.clone())).await.unwrap();
        let Json(restored) = insights(State(state)).await.unwrap();

        assert_eq!(refreshed, restored);
        assert_eq!(
            events.recv().await.unwrap().event,
            "study.insights_refreshed"
        );
    }
}
