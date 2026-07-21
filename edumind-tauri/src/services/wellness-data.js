export const WELLNESS_RECORD_PREFIX = "wellness.";
export const WELLNESS_LIBRARY_RECORD_KEY = "wellness.library.v1";
export const WELLNESS_PROFILE_RECORD_KEY = "wellness.profile.v1";
export const WELLNESS_MEAL_SLOTS = ["Breakfast", "Lunch", "Dinner", "Snack"];

const MAX_CATEGORIES = 24;
const MAX_LIBRARY_ITEMS = 160;
const MAX_DAILY_WORKOUTS = 12;
const MAX_ITEMS_PER_MEAL = 8;
const MAX_DAILY_CHECKS = 48;

export const DEFAULT_WELLNESS_PROFILE = Object.freeze({
  sex: "female",
  activity_level: "moderate",
  goal: "general_fitness",
  age_years: 20,
  weight_kg: 65,
  height_cm: 170,
  training_days_per_week: 3,
  preferred_session_start: "07:00"
});

export const DEFAULT_WELLNESS_LIBRARY = Object.freeze({
  version: 1,
  workout_categories: ["Strength", "Cardio", "Mobility"],
  food_categories: ["Protein", "Carbohydrates", "Fruit and vegetables", "Other"],
  protein_goal_grams: 0,
  workouts: [
    {
      id: "starter-bodyweight-squats",
      category: "Strength",
      name: "Bodyweight squats",
      sets: "3",
      reps: "10",
      load: "Bodyweight",
      notes: "Starter example"
    },
    {
      id: "starter-brisk-walk",
      category: "Cardio",
      name: "Brisk walk",
      sets: "",
      reps: "20 minutes",
      load: "Comfortable pace",
      notes: "Starter example"
    },
    {
      id: "starter-gentle-stretch",
      category: "Mobility",
      name: "Gentle stretch",
      sets: "",
      reps: "8 minutes",
      load: "Easy range",
      notes: "Starter example"
    }
  ],
  foods: [
    {
      id: "starter-eggs",
      category: "Protein",
      name: "Eggs",
      serving: "1 serving",
      protein_grams: 6,
      notes: "Starter example — adjust to your serving."
    },
    {
      id: "starter-oats",
      category: "Carbohydrates",
      name: "Oats",
      serving: "1 bowl",
      protein_grams: 3,
      notes: "Starter example — adjust to your serving."
    },
    {
      id: "starter-banana",
      category: "Fruit and vegetables",
      name: "Banana",
      serving: "1 medium",
      protein_grams: 1,
      notes: "Starter example — adjust to your serving."
    }
  ]
});

export function isWellnessRecordKey(key) {
  return cleanText(key, 160).startsWith(WELLNESS_RECORD_PREFIX);
}

export function wellnessDailyRecordKey(date = new Date()) {
  const dateKey = date instanceof Date ? localDateKey(date) : cleanText(date, 10);
  const normalizedDate = /^\d{4}-\d{2}-\d{2}$/.test(dateKey) ? dateKey : localDateKey();
  return `${WELLNESS_RECORD_PREFIX}daily.${normalizedDate}.v1`;
}

export function createWellnessId(prefix) {
  const safePrefix = cleanText(prefix, 30).replace(/[^a-z0-9-]+/gi, "-") || "item";
  const randomId = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return `${safePrefix}-${randomId}`;
}

export function createEmptyDailyWellness() {
  return {
    version: 1,
    workout_ids: [],
    meal_slots: emptyMealSlots(),
    completed_workout_ids: [],
    completed_meal_keys: []
  };
}

export function cloneStarterLibrary() {
  return normalizeWellnessLibrary(DEFAULT_WELLNESS_LIBRARY);
}

export function normalizeWellnessProfile(value) {
  const source = isObject(value) ? value : {};
  return {
    sex: enumValue(source.sex, ["female", "male"], DEFAULT_WELLNESS_PROFILE.sex),
    activity_level: enumValue(source.activity_level, ["sedentary", "light", "moderate", "active", "very_active"], DEFAULT_WELLNESS_PROFILE.activity_level),
    goal: enumValue(source.goal, ["general_fitness", "lose_fat", "build_strength", "maintain"], DEFAULT_WELLNESS_PROFILE.goal),
    age_years: boundedNumber(source.age_years, 10, 120, DEFAULT_WELLNESS_PROFILE.age_years),
    weight_kg: boundedNumber(source.weight_kg, 20, 400, DEFAULT_WELLNESS_PROFILE.weight_kg),
    height_cm: boundedNumber(source.height_cm, 100, 250, DEFAULT_WELLNESS_PROFILE.height_cm),
    training_days_per_week: boundedNumber(source.training_days_per_week, 0, 7, DEFAULT_WELLNESS_PROFILE.training_days_per_week),
    preferred_session_start: validTime(source.preferred_session_start) ? source.preferred_session_start : DEFAULT_WELLNESS_PROFILE.preferred_session_start
  };
}

export function normalizeWellnessLibrary(value) {
  if (!isObject(value) || !Array.isArray(value.workouts) || !Array.isArray(value.foods)) {
    return cloneDefaultLibrary();
  }

  const workouts = normalizeWorkouts(value.workouts);
  const foods = normalizeFoods(value.foods);
  const workoutCategories = normalizeCategories(value.workout_categories, DEFAULT_WELLNESS_LIBRARY.workout_categories, workouts.map((workout) => workout.category));
  const foodCategories = normalizeCategories(value.food_categories, DEFAULT_WELLNESS_LIBRARY.food_categories, foods.map((food) => food.category));

  return {
    version: 1,
    workout_categories: workoutCategories,
    food_categories: foodCategories,
    protein_goal_grams: boundedNumber(value.protein_goal_grams, 0, 500, 0),
    workouts,
    foods
  };
}

export function normalizeDailyWellness(value) {
  const source = isObject(value) ? value : {};
  const mealSlots = isObject(source.meal_slots) ? source.meal_slots : {};

  return {
    version: 1,
    workout_ids: uniqueStrings(source.workout_ids, MAX_DAILY_WORKOUTS),
    meal_slots: Object.fromEntries(WELLNESS_MEAL_SLOTS.map((slot) => [
      slot,
      uniqueStrings(mealSlots[slot], MAX_ITEMS_PER_MEAL)
    ])),
    completed_workout_ids: uniqueStrings(source.completed_workout_ids, MAX_DAILY_CHECKS),
    completed_meal_keys: uniqueStrings(source.completed_meal_keys, MAX_DAILY_CHECKS)
  };
}

export function toggleId(ids, id, included) {
  const normalizedId = cleanText(id, 120);
  const values = uniqueStrings(ids, MAX_DAILY_CHECKS);
  if (!normalizedId) {
    return values;
  }
  if (included) {
    return values.includes(normalizedId) ? values : [...values, normalizedId];
  }
  return values.filter((value) => value !== normalizedId);
}

export function selectBalancedWorkouts(library, limit = 5) {
  const workouts = normalizeWellnessLibrary(library).workouts;
  const maximum = boundedNumber(limit, 1, MAX_DAILY_WORKOUTS, 5);
  const selected = [];
  const used = new Set();
  const groups = ["strength", "lower", "upper", "cardio", "mobility", "other"];

  for (const group of groups) {
    const item = workouts.find((workout) => !used.has(workout.id) && workoutGroup(workout) === group);
    if (item) {
      selected.push(item.id);
      used.add(item.id);
    }
    if (selected.length >= maximum) {
      return selected;
    }
  }

  for (const workout of workouts) {
    if (!used.has(workout.id)) {
      selected.push(workout.id);
      used.add(workout.id);
    }
    if (selected.length >= maximum) {
      break;
    }
  }
  return selected;
}

export function buildBalancedMealSlots(library) {
  const foods = normalizeWellnessLibrary(library).foods;
  const byGroup = {
    protein: foods.filter((food) => foodGroup(food) === "protein"),
    carbohydrates: foods.filter((food) => foodGroup(food) === "carbohydrates"),
    fruit: foods.filter((food) => foodGroup(food) === "fruit"),
    other: foods.filter((food) => foodGroup(food) === "other")
  };
  const used = new Set();
  const take = (...groups) => {
    for (const group of groups) {
      const item = (byGroup[group] ?? []).find((food) => !used.has(food.id));
      if (item) {
        used.add(item.id);
        return item.id;
      }
    }
    return null;
  };

  return {
    Breakfast: [take("carbohydrates", "protein", "other"), take("protein", "fruit", "other")].filter(Boolean),
    Lunch: [take("protein", "other"), take("carbohydrates", "fruit", "other"), take("fruit", "other")].filter(Boolean),
    Dinner: [take("protein", "other"), take("fruit", "carbohydrates", "other")].filter(Boolean),
    Snack: [take("protein", "fruit", "carbohydrates", "other")].filter(Boolean)
  };
}

export function resolveTodayWorkouts(library, daily) {
  const workoutsById = new Map(normalizeWellnessLibrary(library).workouts.map((workout) => [workout.id, workout]));
  return normalizeDailyWellness(daily).workout_ids.map((id) => workoutsById.get(id)).filter(Boolean);
}

export function resolveTodayMeals(library, daily) {
  const foodsById = new Map(normalizeWellnessLibrary(library).foods.map((food) => [food.id, food]));
  const state = normalizeDailyWellness(daily);
  return Object.fromEntries(WELLNESS_MEAL_SLOTS.map((slot) => [
    slot,
    state.meal_slots[slot].map((id) => foodsById.get(id)).filter(Boolean)
  ]));
}

export function wellnessSummary(library, daily) {
  const normalizedLibrary = normalizeWellnessLibrary(library);
  const state = normalizeDailyWellness(daily);
  const workoutItems = resolveTodayWorkouts(normalizedLibrary, state);
  const meals = resolveTodayMeals(normalizedLibrary, state);
  const completedWorkoutIds = new Set(state.completed_workout_ids);
  const completedMealKeys = new Set(state.completed_meal_keys);
  const workoutCompleted = workoutItems.filter((workout) => completedWorkoutIds.has(workout.id)).length;
  let mealTotal = 0;
  let mealCompleted = 0;
  let proteinLogged = 0;

  for (const slot of WELLNESS_MEAL_SLOTS) {
    for (const food of meals[slot]) {
      mealTotal += 1;
      if (completedMealKeys.has(mealCheckKey(slot, food.id))) {
        mealCompleted += 1;
        proteinLogged += food.protein_grams;
      }
    }
  }

  const proteinGoal = normalizedLibrary.protein_goal_grams;
  return {
    workout_total: workoutItems.length,
    workout_completed: workoutCompleted,
    workout_percent: percentage(workoutCompleted, workoutItems.length),
    meal_total: mealTotal,
    meal_completed: mealCompleted,
    meal_percent: percentage(mealCompleted, mealTotal),
    protein_logged_grams: proteinLogged,
    protein_goal_grams: proteinGoal,
    protein_percent: proteinGoal ? percentage(proteinLogged, proteinGoal) : null
  };
}

export function mealCheckKey(slot, foodId) {
  return `${cleanText(slot, 40)}:${cleanText(foodId, 120)}`;
}

function cloneDefaultLibrary() {
  return {
    version: 1,
    workout_categories: [...DEFAULT_WELLNESS_LIBRARY.workout_categories],
    food_categories: [...DEFAULT_WELLNESS_LIBRARY.food_categories],
    protein_goal_grams: DEFAULT_WELLNESS_LIBRARY.protein_goal_grams,
    workouts: DEFAULT_WELLNESS_LIBRARY.workouts.map((workout) => ({ ...workout })),
    foods: DEFAULT_WELLNESS_LIBRARY.foods.map((food) => ({ ...food }))
  };
}

function normalizeWorkouts(value) {
  return uniqueById(value, MAX_LIBRARY_ITEMS, (item) => {
    if (!isObject(item)) {
      return null;
    }
    const id = cleanText(item.id, 120);
    const name = cleanText(item.name, 120);
    if (!id || !name) {
      return null;
    }
    return {
      id,
      category: cleanText(item.category, 80) || "Other",
      name,
      sets: cleanText(item.sets, 40),
      reps: cleanText(item.reps, 60),
      load: cleanText(item.load, 80),
      notes: cleanText(item.notes, 320)
    };
  });
}

function normalizeFoods(value) {
  return uniqueById(value, MAX_LIBRARY_ITEMS, (item) => {
    if (!isObject(item)) {
      return null;
    }
    const id = cleanText(item.id, 120);
    const name = cleanText(item.name, 120);
    if (!id || !name) {
      return null;
    }
    return {
      id,
      category: cleanText(item.category, 80) || "Other",
      name,
      serving: cleanText(item.serving, 120),
      protein_grams: boundedNumber(item.protein_grams, 0, 500, 0),
      notes: cleanText(item.notes, 320)
    };
  });
}

function normalizeCategories(value, defaults, itemCategories) {
  const candidates = Array.isArray(value) ? value : defaults;
  const normalized = uniqueStrings(candidates, MAX_CATEGORIES, 80);
  return uniqueStrings([...normalized, ...itemCategories], MAX_CATEGORIES, 80);
}

function uniqueById(value, limit, mapper) {
  const source = Array.isArray(value) ? value : [];
  const seen = new Set();
  const result = [];
  for (const item of source) {
    const normalized = mapper(item);
    if (!normalized || seen.has(normalized.id)) {
      continue;
    }
    result.push(normalized);
    seen.add(normalized.id);
    if (result.length >= limit) {
      break;
    }
  }
  return result;
}

function uniqueStrings(value, limit, maxLength = 120) {
  const source = Array.isArray(value) ? value : [];
  const seen = new Set();
  const result = [];
  for (const item of source) {
    const normalized = cleanText(item, maxLength);
    if (!normalized || seen.has(normalized)) {
      continue;
    }
    result.push(normalized);
    seen.add(normalized);
    if (result.length >= limit) {
      break;
    }
  }
  return result;
}

function emptyMealSlots() {
  return Object.fromEntries(WELLNESS_MEAL_SLOTS.map((slot) => [slot, []]));
}

function workoutGroup(workout) {
  const text = `${workout.category} ${workout.name}`.toLowerCase();
  if (/\b(cardio|walk|run|cycle|swim|dance)\b/.test(text)) {
    return "cardio";
  }
  if (/\b(mobility|stretch|yoga|flexibility)\b/.test(text)) {
    return "mobility";
  }
  if (/\b(lower|leg|squat|lunge|glute|calf)\b/.test(text)) {
    return "lower";
  }
  if (/\b(upper|push|pull|press|row|shoulder|chest|arm)\b/.test(text)) {
    return "upper";
  }
  if (/\b(strength|resistance|weight)\b/.test(text)) {
    return "strength";
  }
  return "other";
}

function foodGroup(food) {
  const text = `${food.category} ${food.name} ${food.notes}`.toLowerCase();
  if (/\b(protein|egg|chicken|fish|beef|milk|yogurt|lentil|dal|paneer|tofu|bean)\b/.test(text) || food.protein_grams >= 7) {
    return "protein";
  }
  if (/\b(carb|oat|rice|bread|potato|pasta|banana|fruit)\b/.test(text)) {
    return "carbohydrates";
  }
  if (/\b(fruit|vegetable|veg|salad|green|spinach|carrot)\b/.test(text)) {
    return "fruit";
  }
  return "other";
}

function percentage(completed, total) {
  return total ? Math.round((completed / total) * 100) : 0;
}

function enumValue(value, allowed, fallback) {
  return allowed.includes(value) ? value : fallback;
}

function boundedNumber(value, minimum, maximum, fallback) {
  const number = Number(value);
  return Number.isFinite(number) ? Math.min(maximum, Math.max(minimum, number)) : fallback;
}

function validTime(value) {
  return /^([01]\d|2[0-3]):[0-5]\d$/.test(String(value ?? ""));
}

function cleanText(value, maximum) {
  return typeof value === "string" ? value.trim().slice(0, maximum) : "";
}

function isObject(value) {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function localDateKey(date = new Date()) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}
