//! Deterministic, dependency-light wellness planning contracts for EduMind.
//!
//! These types and pure functions produce reproducible workout and meal
//! guidance from an explicit learner profile. They are intentionally advisory:
//! nothing here is medical, dietary, or clinical advice, and callers must
//! surface [`WELLNESS_DISCLAIMER`] and require user confirmation before any
//! generated block is persisted to a schedule.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Advisory notice callers must present alongside any generated wellness plan.
pub const WELLNESS_DISCLAIMER: &str = "This wellness guidance is general and educational, not medical, dietary, or clinical advice. Confirm with a qualified professional before acting on it.";

/// Ordered days of the week, aligned with the Student Planner projection.
const WEEK: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// Biological sex, required only for the resting-metabolic-rate equation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BiologicalSex {
    #[default]
    Female,
    Male,
}

/// Typical weekly physical-activity level used to scale energy needs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityLevel {
    Sedentary,
    #[default]
    Light,
    Moderate,
    Active,
    VeryActive,
}

impl ActivityLevel {
    /// Returns the activity multiplier scaled by 1000 to keep arithmetic integer-exact.
    #[must_use]
    const fn multiplier_milli(self) -> u32 {
        match self {
            Self::Sedentary => 1200,
            Self::Light => 1375,
            Self::Moderate => 1550,
            Self::Active => 1725,
            Self::VeryActive => 1900,
        }
    }
}

/// The learner's primary wellness objective.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FitnessGoal {
    LoseFat,
    #[default]
    GeneralFitness,
    BuildStrength,
    Maintain,
}

impl FitnessGoal {
    /// Daily calorie adjustment applied to maintenance energy for this goal.
    #[must_use]
    const fn calorie_adjustment(self) -> i32 {
        match self {
            Self::LoseFat => -500,
            Self::GeneralFitness | Self::Maintain => 0,
            Self::BuildStrength => 250,
        }
    }

    /// Protein target in grams per kilogram of body weight, scaled by 10.
    #[must_use]
    const fn protein_per_kg_deci(self) -> u32 {
        match self {
            Self::Maintain => 14,
            Self::GeneralFitness => 16,
            Self::LoseFat => 20,
            Self::BuildStrength => 18,
        }
    }
}

/// An explicit, user-owned wellness profile. All fields are learner-provided.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WellnessProfile {
    #[serde(default)]
    pub sex: BiologicalSex,
    #[serde(default)]
    pub activity_level: ActivityLevel,
    #[serde(default)]
    pub goal: FitnessGoal,
    #[serde(default)]
    pub age_years: u8,
    #[serde(default)]
    pub weight_kg: u16,
    #[serde(default)]
    pub height_cm: u16,
    /// Training days the learner wants each week, clamped to `0..=7`.
    #[serde(default)]
    pub training_days_per_week: u8,
    /// Preferred session start time in `HH:MM`, used only for schedule mapping.
    #[serde(default)]
    pub preferred_session_start: Option<String>,
}

impl Default for WellnessProfile {
    fn default() -> Self {
        Self {
            sex: BiologicalSex::default(),
            activity_level: ActivityLevel::default(),
            goal: FitnessGoal::default(),
            age_years: 20,
            weight_kg: 65,
            height_cm: 170,
            training_days_per_week: 3,
            preferred_session_start: None,
        }
    }
}

/// Deterministic daily energy and macronutrient targets.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NutritionTargets {
    pub maintenance_calories: u32,
    pub target_calories: u32,
    pub protein_grams: u32,
    pub fat_grams: u32,
    pub carbohydrate_grams: u32,
}

/// One planned workout session mapped to a specific weekday.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkoutSession {
    pub day: String,
    pub focus: String,
    pub duration_minutes: u16,
    pub intensity: String,
}

/// A full deterministic weekly training plan.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkoutPlan {
    pub goal: FitnessGoal,
    pub sessions: Vec<WorkoutSession>,
    pub weekly_minutes: u32,
}

/// One meal in a day's plan with an approximate energy share.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Meal {
    pub name: String,
    pub calories: u32,
    pub protein_grams: u32,
}

/// A deterministic single-day meal plan aligned with [`NutritionTargets`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MealPlan {
    pub target_calories: u32,
    pub meals: Vec<Meal>,
}

/// A schedule block suitable for reconciliation with the Student Planner.
///
/// The shape intentionally matches the planner's `schedule-block` records so
/// the Routine Manager can reconcile wellness time against class time.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WellnessScheduleBlock {
    pub day: String,
    pub title: String,
    pub start: String,
    pub end: String,
}

/// Estimates maintenance and goal-adjusted daily energy plus macros.
///
/// Uses the Mifflin-St Jeor resting-metabolic-rate equation with an activity
/// multiplier. Missing weight or height yields all-zero targets rather than a
/// misleading estimate.
#[must_use]
pub fn estimate_nutrition_targets(profile: &WellnessProfile) -> NutritionTargets {
    if profile.weight_kg == 0 || profile.height_cm == 0 {
        return NutritionTargets {
            maintenance_calories: 0,
            target_calories: 0,
            protein_grams: 0,
            fat_grams: 0,
            carbohydrate_grams: 0,
        };
    }

    let weight = i32::from(profile.weight_kg);
    let height = i32::from(profile.height_cm);
    let age = i32::from(profile.age_years);
    let sex_offset = match profile.sex {
        BiologicalSex::Male => 5,
        BiologicalSex::Female => -161,
    };
    // BMR (Mifflin-St Jeor): 10*kg + 6.25*cm - 5*age + sex_offset.
    // Scale height by 625/100 with integer math to stay reproducible.
    let bmr = (10 * weight) + (625 * height / 100) - (5 * age) + sex_offset;
    let bmr = bmr.max(0) as u32;
    let maintenance_calories = bmr.saturating_mul(profile.activity_level.multiplier_milli()) / 1000;

    let target_calories = {
        let adjusted = maintenance_calories as i64 + i64::from(profile.goal.calorie_adjustment());
        adjusted.clamp(1200, i64::from(u32::MAX)) as u32
    };

    let protein_grams =
        (u32::from(profile.weight_kg).saturating_mul(profile.goal.protein_per_kg_deci())) / 10;
    // Fat at ~25% of target calories (9 kcal/g); carbohydrates fill the remainder.
    let fat_grams = (target_calories.saturating_mul(25) / 100) / 9;
    let protein_calories = protein_grams.saturating_mul(4);
    let fat_calories = fat_grams.saturating_mul(9);
    let carbohydrate_calories = target_calories.saturating_sub(protein_calories + fat_calories);
    let carbohydrate_grams = carbohydrate_calories / 4;

    NutritionTargets {
        maintenance_calories,
        target_calories,
        protein_grams,
        fat_grams,
        carbohydrate_grams,
    }
}

/// Builds a deterministic weekly workout plan from the profile.
///
/// Training days are clamped to `0..=7` and spread across the week using a
/// fixed spacing so identical profiles always produce identical plans.
#[must_use]
pub fn generate_workout_plan(profile: &WellnessProfile) -> WorkoutPlan {
    let days = profile.training_days_per_week.min(7) as usize;
    let (duration_minutes, intensity) = match profile.goal {
        FitnessGoal::LoseFat => (45u16, "moderate"),
        FitnessGoal::GeneralFitness => (40, "moderate"),
        FitnessGoal::BuildStrength => (60, "high"),
        FitnessGoal::Maintain => (35, "easy"),
    };

    let focuses = goal_focuses(profile.goal);
    let selected_days = spread_days(days);
    let sessions = selected_days
        .into_iter()
        .enumerate()
        .map(|(index, day_index)| WorkoutSession {
            day: WEEK[day_index].to_owned(),
            focus: focuses[index % focuses.len()].to_owned(),
            duration_minutes,
            intensity: intensity.to_owned(),
        })
        .collect::<Vec<_>>();

    let weekly_minutes = sessions.iter().fold(0u32, |total, session| {
        total.saturating_add(u32::from(session.duration_minutes))
    });

    WorkoutPlan {
        goal: profile.goal,
        sessions,
        weekly_minutes,
    }
}

/// Distributes a day's target calories across a fixed four-meal structure.
#[must_use]
pub fn generate_meal_plan(targets: &NutritionTargets) -> MealPlan {
    let calories = targets.target_calories;
    let protein = targets.protein_grams;
    // Fixed percentage split keeps the plan reproducible and easy to reason about.
    let structure = [
        ("Breakfast", 25u32),
        ("Lunch", 35),
        ("Dinner", 30),
        ("Snack", 10),
    ];
    let mut meals = structure
        .iter()
        .map(|(name, share)| Meal {
            name: (*name).to_owned(),
            calories: calories.saturating_mul(*share) / 100,
            protein_grams: protein.saturating_mul(*share) / 100,
        })
        .collect::<Vec<_>>();
    // Integer shares floor independently, so hand any rounding remainder to the
    // final meal. This keeps meal totals an exact reconstruction of the targets.
    if let Some(last) = meals.last_mut() {
        let calorie_sum: u32 = structure
            .iter()
            .map(|(_, share)| calories.saturating_mul(*share) / 100)
            .sum();
        let protein_sum: u32 = structure
            .iter()
            .map(|(_, share)| protein.saturating_mul(*share) / 100)
            .sum();
        last.calories = last
            .calories
            .saturating_add(calories.saturating_sub(calorie_sum));
        last.protein_grams = last
            .protein_grams
            .saturating_add(protein.saturating_sub(protein_sum));
    }
    MealPlan {
        target_calories: calories,
        meals,
    }
}

/// Maps a workout plan into planner-compatible schedule blocks.
///
/// When the profile omits a preferred start time, `07:00` is used. Blocks whose
/// computed end time would exceed `23:59` are dropped rather than wrapped.
#[must_use]
pub fn workout_schedule_blocks(
    profile: &WellnessProfile,
    plan: &WorkoutPlan,
) -> Vec<WellnessScheduleBlock> {
    let start_minutes = profile
        .preferred_session_start
        .as_deref()
        .and_then(parse_hh_mm)
        .unwrap_or(7 * 60);

    plan.sessions
        .iter()
        .filter_map(|session| {
            let end_minutes = start_minutes + u32::from(session.duration_minutes);
            if end_minutes > (23 * 60 + 59) {
                return None;
            }
            Some(WellnessScheduleBlock {
                day: session.day.clone(),
                title: format!("Workout: {}", session.focus),
                start: format_hh_mm(start_minutes),
                end: format_hh_mm(end_minutes),
            })
        })
        .collect()
}

fn goal_focuses(goal: FitnessGoal) -> &'static [&'static str] {
    match goal {
        FitnessGoal::LoseFat => &[
            "Cardio + core",
            "Full body circuit",
            "Intervals",
            "Mobility",
        ],
        FitnessGoal::GeneralFitness => {
            &["Full body", "Cardio", "Strength basics", "Active recovery"]
        }
        FitnessGoal::BuildStrength => &[
            "Upper body",
            "Lower body",
            "Push",
            "Pull",
            "Legs",
            "Accessory",
            "Core",
        ],
        FitnessGoal::Maintain => &["Light full body", "Walk or cardio", "Mobility"],
    }
}

/// Selects `count` weekday indices spread as evenly as possible across the week.
fn spread_days(count: usize) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }
    if count >= 7 {
        return (0..7).collect();
    }
    let mut days = BTreeMap::new();
    for slot in 0..count {
        // Even spacing across 7 days; stable and deterministic.
        let index = (slot * 7) / count;
        days.insert(index.min(6), ());
    }
    // If rounding collided, backfill from the front to preserve the requested count.
    let mut selected: Vec<usize> = days.into_keys().collect();
    let mut candidate = 0usize;
    while selected.len() < count && candidate < 7 {
        if !selected.contains(&candidate) {
            selected.push(candidate);
        }
        candidate += 1;
    }
    selected.sort_unstable();
    selected
}

fn parse_hh_mm(value: &str) -> Option<u32> {
    let value = value.trim();
    let (hours, minutes) = value.split_once(':')?;
    let hours: u32 = hours.parse().ok()?;
    let minutes: u32 = minutes.parse().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(hours * 60 + minutes)
}

fn format_hh_mm(total_minutes: u32) -> String {
    let hours = (total_minutes / 60).min(23);
    let minutes = total_minutes % 60;
    format!("{hours:02}:{minutes:02}")
}

#[cfg(test)]
mod tests {
    use super::{
        ActivityLevel, BiologicalSex, FitnessGoal, WellnessProfile, estimate_nutrition_targets,
        generate_meal_plan, generate_workout_plan, workout_schedule_blocks,
    };

    fn strength_profile() -> WellnessProfile {
        WellnessProfile {
            sex: BiologicalSex::Male,
            activity_level: ActivityLevel::Moderate,
            goal: FitnessGoal::BuildStrength,
            age_years: 22,
            weight_kg: 75,
            height_cm: 180,
            training_days_per_week: 4,
            preferred_session_start: Some("07:00".to_owned()),
        }
    }

    #[test]
    fn nutrition_targets_are_deterministic_and_goal_adjusted() {
        let profile = strength_profile();
        let first = estimate_nutrition_targets(&profile);
        let second = estimate_nutrition_targets(&profile);
        assert_eq!(first, second);
        // BuildStrength adds a surplus above maintenance.
        assert!(first.target_calories > first.maintenance_calories);
        // Protein target reflects 1.8 g/kg for 75 kg.
        assert_eq!(first.protein_grams, 135);
    }

    #[test]
    fn missing_body_metrics_yield_zeroed_targets() {
        let profile = WellnessProfile {
            weight_kg: 0,
            ..WellnessProfile::default()
        };
        let targets = estimate_nutrition_targets(&profile);
        assert_eq!(targets.target_calories, 0);
        assert_eq!(targets.protein_grams, 0);
    }

    #[test]
    fn workout_plan_matches_requested_days_and_is_stable() {
        let profile = strength_profile();
        let plan = generate_workout_plan(&profile);
        let repeat = generate_workout_plan(&profile);
        assert_eq!(plan, repeat);
        assert_eq!(plan.sessions.len(), 4);
        assert_eq!(plan.weekly_minutes, 240);
    }

    #[test]
    fn workout_days_are_clamped_to_week() {
        let profile = WellnessProfile {
            training_days_per_week: 12,
            ..WellnessProfile::default()
        };
        let plan = generate_workout_plan(&profile);
        assert_eq!(plan.sessions.len(), 7);
    }

    #[test]
    fn meal_plan_shares_sum_to_target() {
        let profile = strength_profile();
        let targets = estimate_nutrition_targets(&profile);
        let plan = generate_meal_plan(&targets);
        assert_eq!(plan.meals.len(), 4);
        let total: u32 = plan.meals.iter().map(|meal| meal.calories).sum();
        // Percentage split sums to 100%, so meal calories reconstruct the target.
        assert_eq!(total, targets.target_calories);
    }

    #[test]
    fn schedule_blocks_use_preferred_start() {
        let profile = strength_profile();
        let plan = generate_workout_plan(&profile);
        let blocks = workout_schedule_blocks(&profile, &plan);
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[0].start, "07:00");
        assert_eq!(blocks[0].end, "08:00");
        assert!(blocks[0].title.starts_with("Workout:"));
    }
}
