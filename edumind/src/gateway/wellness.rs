use axum::{Json, extract::State, http::StatusCode};
use edumind_core::wellness::{
    MealPlan, NutritionTargets, WELLNESS_DISCLAIMER, WellnessProfile, WellnessScheduleBlock,
    WorkoutPlan, estimate_nutrition_targets, generate_meal_plan, generate_workout_plan,
    workout_schedule_blocks,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::gateway::AppState;

type ApiError = (StatusCode, Json<Value>);
type ApiResult<T> = std::result::Result<Json<T>, ApiError>;

/// Request for a deterministic wellness plan built from an explicit profile.
#[derive(Debug, Deserialize)]
pub struct WellnessPlanRequest {
    pub profile: WellnessProfile,
    /// When true, reconcile workout blocks against the canonical class schedule.
    #[serde(default)]
    pub reconcile_with_planner: bool,
}

/// A workout block that overlaps an existing class-schedule commitment.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleConflict {
    pub day: String,
    pub workout_title: String,
    pub workout_start: String,
    pub workout_end: String,
    pub conflicts_with: String,
    pub class_start: String,
    pub class_end: String,
}

/// The full advisory wellness plan returned to the desktop app.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WellnessPlanResponse {
    pub nutrition: NutritionTargets,
    pub workout: WorkoutPlan,
    pub meals: MealPlan,
    pub schedule_blocks: Vec<WellnessScheduleBlock>,
    pub planner_conflicts: Vec<ScheduleConflict>,
    pub disclaimer: String,
}

/// Builds a deterministic, advisory workout and meal plan from a learner profile.
///
/// The plan is never persisted here. Callers must present the returned
/// disclaimer and route any schedule changes through the Routine Manager's
/// confirmation flow before writing planner state.
pub async fn plan(
    State(state): State<AppState>,
    Json(request): Json<WellnessPlanRequest>,
) -> ApiResult<WellnessPlanResponse> {
    let profile = request.profile;
    let nutrition = estimate_nutrition_targets(&profile);
    let workout = generate_workout_plan(&profile);
    let meals = generate_meal_plan(&nutrition);
    let schedule_blocks = workout_schedule_blocks(&profile, &workout);

    let planner_conflicts = if request.reconcile_with_planner {
        let schedule = state.student_pages().planner_schedule().map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {"code": "wellness_planner_unavailable", "message": error.to_string()},
                })),
            )
        })?;
        detect_conflicts(&schedule_blocks, &schedule)
    } else {
        Vec::new()
    };

    Ok(Json(WellnessPlanResponse {
        nutrition,
        workout,
        meals,
        schedule_blocks,
        planner_conflicts,
        disclaimer: WELLNESS_DISCLAIMER.to_owned(),
    }))
}

fn detect_conflicts(
    blocks: &[WellnessScheduleBlock],
    schedule: &crate::student::PlannerSchedule,
) -> Vec<ScheduleConflict> {
    let mut conflicts = Vec::new();
    for block in blocks {
        let Some(day) = schedule.days.iter().find(|day| day.day == block.day) else {
            continue;
        };
        for entry in &day.entries {
            if overlaps(&block.start, &block.end, &entry.start, &entry.end) {
                conflicts.push(ScheduleConflict {
                    day: block.day.clone(),
                    workout_title: block.title.clone(),
                    workout_start: block.start.clone(),
                    workout_end: block.end.clone(),
                    conflicts_with: entry.title.clone(),
                    class_start: entry.start.clone(),
                    class_end: entry.end.clone(),
                });
            }
        }
    }
    conflicts
}

fn overlaps(start_a: &str, end_a: &str, start_b: &str, end_b: &str) -> bool {
    let (Some(sa), Some(ea), Some(sb), Some(eb)) = (
        minutes(start_a),
        minutes(end_a),
        minutes(start_b),
        minutes(end_b),
    ) else {
        return false;
    };
    sa < eb && sb < ea
}

fn minutes(value: &str) -> Option<u32> {
    let (hours, mins) = value.split_once(':')?;
    Some(hours.parse::<u32>().ok()? * 60 + mins.parse::<u32>().ok()?)
}

#[cfg(test)]
mod tests {
    use axum::{Json, extract::State};
    use chrono::Utc;
    use edumind_core::wellness::{FitnessGoal, WellnessProfile};
    use serde_json::json;

    use super::{WellnessPlanRequest, plan};
    use crate::{config::EduMindConfig, gateway::AppState, student::StudentPageRecordInput};

    fn profile() -> WellnessProfile {
        WellnessProfile {
            goal: FitnessGoal::BuildStrength,
            training_days_per_week: 4,
            weight_kg: 75,
            height_cm: 180,
            preferred_session_start: Some("09:00".to_owned()),
            ..WellnessProfile::default()
        }
    }

    #[tokio::test]
    async fn builds_advisory_plan_without_persisting() {
        let state = AppState::in_memory(EduMindConfig::default()).unwrap();
        let Json(response) = plan(
            State(state),
            Json(WellnessPlanRequest {
                profile: profile(),
                reconcile_with_planner: false,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.workout.sessions.len(), 4);
        assert_eq!(response.meals.meals.len(), 4);
        assert!(response.nutrition.target_calories > 0);
        assert!(response.planner_conflicts.is_empty());
        assert!(!response.disclaimer.is_empty());
    }

    #[tokio::test]
    async fn reports_conflicts_with_class_schedule() {
        let state = AppState::in_memory(EduMindConfig::default()).unwrap();
        state
            .student_pages()
            .save_all(
                "planner",
                vec![StudentPageRecordInput::new(
                    "mon-lecture",
                    json!({
                        "kind": "schedule-block",
                        "day": "Monday",
                        "title": "Physics lecture",
                        "start": "09:30",
                        "end": "10:30"
                    }),
                    Utc::now(),
                )],
                "test",
                Utc::now(),
            )
            .unwrap();

        let Json(response) = plan(
            State(state),
            Json(WellnessPlanRequest {
                profile: profile(),
                reconcile_with_planner: true,
            }),
        )
        .await
        .unwrap();

        // The first workout lands Monday 09:00-10:00 and overlaps the lecture.
        assert!(
            response
                .planner_conflicts
                .iter()
                .any(|conflict| conflict.conflicts_with == "Physics lecture")
        );
    }
}
