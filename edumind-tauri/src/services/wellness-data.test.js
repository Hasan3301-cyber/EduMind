import { describe, expect, it } from "vitest";

import {
  buildBalancedMealSlots,
  cloneStarterLibrary,
  normalizeDailyWellness,
  normalizeWellnessLibrary,
  selectBalancedWorkouts,
  wellnessDailyRecordKey,
  wellnessSummary
} from "./wellness-data";

describe("wellness data", () => {
  it("keeps an intentionally empty saved library empty", () => {
    const library = normalizeWellnessLibrary({
      workout_categories: ["Custom movement"],
      food_categories: ["Custom food"],
      workouts: [],
      foods: [],
      protein_goal_grams: 80
    });

    expect(library.workouts).toEqual([]);
    expect(library.foods).toEqual([]);
    expect(library.protein_goal_grams).toBe(80);
  });

  it("builds bounded balanced picks from a local starter library", () => {
    const library = cloneStarterLibrary();
    const workouts = selectBalancedWorkouts(library);
    const meals = buildBalancedMealSlots(library);

    expect(workouts).toEqual(expect.arrayContaining([
      "starter-bodyweight-squats",
      "starter-brisk-walk",
      "starter-gentle-stretch"
    ]));
    expect(workouts.length).toBeLessThanOrEqual(5);
    expect(Object.values(meals).flat()).toEqual(expect.arrayContaining([
      "starter-eggs",
      "starter-oats"
    ]));
  });

  it("counts only completed meal items toward protein progress", () => {
    const library = normalizeWellnessLibrary({
      workout_categories: ["Strength"],
      food_categories: ["Protein"],
      protein_goal_grams: 20,
      workouts: [{ id: "squat", category: "Strength", name: "Squat" }],
      foods: [
        { id: "eggs", category: "Protein", name: "Eggs", protein_grams: 6 },
        { id: "yogurt", category: "Protein", name: "Yogurt", protein_grams: 10 }
      ]
    });
    const daily = normalizeDailyWellness({
      workout_ids: ["squat"],
      completed_workout_ids: ["squat"],
      meal_slots: { Breakfast: ["eggs"], Lunch: ["yogurt"] },
      completed_meal_keys: ["Breakfast:eggs"]
    });

    expect(wellnessSummary(library, daily)).toMatchObject({
      workout_total: 1,
      workout_completed: 1,
      meal_total: 2,
      meal_completed: 1,
      protein_logged_grams: 6,
      protein_percent: 30
    });
  });

  it("creates a date-scoped daily record key", () => {
    expect(wellnessDailyRecordKey(new Date(2026, 6, 20))).toBe("wellness.daily.2026-07-20.v1");
  });
});
