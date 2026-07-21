import { useEffect, useMemo, useState } from "react";

import {
  DEFAULT_WELLNESS_PROFILE,
  WELLNESS_LIBRARY_RECORD_KEY,
  WELLNESS_MEAL_SLOTS,
  WELLNESS_PROFILE_RECORD_KEY,
  buildBalancedMealSlots,
  cloneStarterLibrary,
  createEmptyDailyWellness,
  createWellnessId,
  mealCheckKey,
  normalizeDailyWellness,
  normalizeWellnessLibrary,
  normalizeWellnessProfile,
  resolveTodayMeals,
  resolveTodayWorkouts,
  selectBalancedWorkouts,
  toggleId,
  wellnessDailyRecordKey,
  wellnessSummary
} from "../services/wellness-data";

const GOALS = [
  { value: "general_fitness", label: "General fitness" },
  { value: "lose_fat", label: "Lose fat" },
  { value: "build_strength", label: "Build strength" },
  { value: "maintain", label: "Maintain" }
];

const ACTIVITY_LEVELS = [
  { value: "sedentary", label: "Sedentary" },
  { value: "light", label: "Light" },
  { value: "moderate", label: "Moderate" },
  { value: "active", label: "Active" },
  { value: "very_active", label: "Very active" }
];

const SEXES = [
  { value: "female", label: "Female" },
  { value: "male", label: "Male" }
];

const EMPTY_WORKOUT_FORM = Object.freeze({
  category: "",
  name: "",
  sets: "",
  reps: "",
  load: "",
  notes: ""
});

const EMPTY_FOOD_FORM = Object.freeze({
  category: "",
  name: "",
  serving: "",
  protein_grams: "",
  notes: ""
});

export function WellnessPanel({ client, connectionState }) {
  const [library, setLibrary] = useState(() => cloneStarterLibrary());
  const [daily, setDaily] = useState(() => createEmptyDailyWellness());
  const [profile, setProfile] = useState(() => ({ ...DEFAULT_WELLNESS_PROFILE }));
  const [libraryPersisted, setLibraryPersisted] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [isBuilding, setIsBuilding] = useState(false);
  const [status, setStatus] = useState("Loading your private wellness tracker…");
  const [error, setError] = useState(null);
  const [plan, setPlan] = useState(null);
  const [showWorkoutForm, setShowWorkoutForm] = useState(false);
  const [showFoodForm, setShowFoodForm] = useState(false);
  const [showWorkoutCategoryForm, setShowWorkoutCategoryForm] = useState(false);
  const [showFoodCategoryForm, setShowFoodCategoryForm] = useState(false);
  const [editingWorkoutId, setEditingWorkoutId] = useState(null);
  const [editingFoodId, setEditingFoodId] = useState(null);
  const [workoutForm, setWorkoutForm] = useState(EMPTY_WORKOUT_FORM);
  const [foodForm, setFoodForm] = useState(EMPTY_FOOD_FORM);
  const [newWorkoutCategory, setNewWorkoutCategory] = useState("");
  const [newFoodCategory, setNewFoodCategory] = useState("");
  const [proteinGoalDraft, setProteinGoalDraft] = useState("0");

  const todayKey = useMemo(() => wellnessDailyRecordKey(), []);
  const formattedToday = useMemo(() => new Intl.DateTimeFormat("en-US", {
    weekday: "long",
    month: "short",
    day: "numeric"
  }).format(new Date()), []);
  const todayWorkouts = useMemo(() => resolveTodayWorkouts(library, daily), [library, daily]);
  const todayMeals = useMemo(() => resolveTodayMeals(library, daily), [library, daily]);
  const summary = useMemo(() => wellnessSummary(library, daily), [library, daily]);
  const busy = isLoading || isSaving || isBuilding;
  const connected = Boolean(client && connectionState === "connected");

  useEffect(() => {
    let active = true;

    async function load() {
      if (!client) {
        if (active) {
          setIsLoading(false);
          setStatus("Offline preview is ready. Launch the desktop gateway to save your private wellness data.");
        }
        return;
      }

      try {
        const snapshot = await client.studentPage("student-os");
        if (!active) {
          return;
        }
        const records = Array.isArray(snapshot?.records) ? snapshot.records : [];
        const storedLibrary = activeRecordValue(records, WELLNESS_LIBRARY_RECORD_KEY);
        const storedDaily = activeRecordValue(records, todayKey);
        const storedProfile = activeRecordValue(records, WELLNESS_PROFILE_RECORD_KEY);
        const nextLibrary = normalizeWellnessLibrary(storedLibrary);
        setLibrary(nextLibrary);
        setDaily(normalizeDailyWellness(storedDaily));
        setProfile(normalizeWellnessProfile(storedProfile));
        setLibraryPersisted(Boolean(storedLibrary));
        setProteinGoalDraft(String(nextLibrary.protein_goal_grams));
        setStatus(storedLibrary || storedDaily
          ? "Your wellness library and today’s checklists are saved locally."
          : "Your private starter wellness library is ready. Nothing is saved until you make a change.");
      } catch (reason) {
        if (active) {
          setError(reason.message ?? "EduMind could not load the wellness tracker.");
          setStatus("Starter wellness items are available in this session.");
        }
      } finally {
        if (active) {
          setIsLoading(false);
        }
      }
    }

    void load();
    return () => {
      active = false;
    };
  }, [client, todayKey]);

  async function persistLibrary(nextLibrary, successMessage) {
    if (isSaving) {
      return false;
    }
    const previous = library;
    const normalized = normalizeWellnessLibrary(nextLibrary);
    setLibrary(normalized);
    setProteinGoalDraft(String(normalized.protein_goal_grams));
    setIsSaving(true);
    setError(null);
    try {
      if (!client) {
        setStatus("Updated in this preview session. Launch the desktop gateway to save it privately.");
        return true;
      }
      await client.upsertStudentPageRecord("student-os", WELLNESS_LIBRARY_RECORD_KEY, normalized, { source: "wellness" });
      setLibraryPersisted(true);
      setStatus(successMessage);
      return true;
    } catch (reason) {
      setLibrary(previous);
      setProteinGoalDraft(String(previous.protein_goal_grams));
      setError(reason.message ?? "EduMind could not save the wellness library.");
      return false;
    } finally {
      setIsSaving(false);
    }
  }

  async function persistDaily(nextDaily, successMessage) {
    if (isSaving) {
      return false;
    }
    const previous = daily;
    const normalized = normalizeDailyWellness(nextDaily);
    setDaily(normalized);
    setIsSaving(true);
    setError(null);
    try {
      if (!client) {
        setStatus("Updated in this preview session. Launch the desktop gateway to save today’s tracker privately.");
        return true;
      }
      if (!libraryPersisted) {
        await client.upsertStudentPageRecord("student-os", WELLNESS_LIBRARY_RECORD_KEY, library, { source: "wellness" });
        setLibraryPersisted(true);
      }
      await client.upsertStudentPageRecord("student-os", todayKey, normalized, { source: "wellness" });
      setStatus(successMessage);
      return true;
    } catch (reason) {
      setDaily(previous);
      setError(reason.message ?? "EduMind could not save today’s wellness tracker.");
      return false;
    } finally {
      setIsSaving(false);
    }
  }

  async function saveProfile() {
    if (isSaving) {
      return;
    }
    const normalized = normalizeWellnessProfile(profile);
    const previous = profile;
    setProfile(normalized);
    setIsSaving(true);
    setError(null);
    try {
      if (!client) {
        setStatus("Updated in this preview session. Launch the desktop gateway to save the profile locally.");
        return;
      }
      await client.upsertStudentPageRecord("student-os", WELLNESS_PROFILE_RECORD_KEY, normalized, { source: "wellness" });
      setStatus("Your wellness profile is saved locally for future advisory plans.");
    } catch (reason) {
      setProfile(previous);
      setError(reason.message ?? "EduMind could not save the wellness profile.");
    } finally {
      setIsSaving(false);
    }
  }

  function updateProfileField(field, value) {
    setProfile((current) => ({ ...current, [field]: value }));
  }

  function updateProfileNumber(field, value) {
    const parsed = Number(value);
    updateProfileField(field, Number.isFinite(parsed) ? parsed : 0);
  }

  async function buildAdvisoryPlan(event) {
    event.preventDefault();
    if (isBuilding) {
      return;
    }
    if (!connected) {
      setStatus("Launch the desktop app and connect its local gateway to build an advisory wellness plan.");
      return;
    }
    setIsBuilding(true);
    setError(null);
    try {
      const response = await client.wellnessPlan(normalizeWellnessProfile(profile), { reconcileWithPlanner: true });
      setPlan(response);
      setStatus("Advisory plan ready. Review it before making any change to your routine or planner.");
    } catch (reason) {
      setError(reason.message ?? "EduMind could not build the wellness plan.");
    } finally {
      setIsBuilding(false);
    }
  }

  function openWorkoutEditor(workout = null) {
    setEditingWorkoutId(workout?.id ?? null);
    setWorkoutForm(workout
      ? {
        category: workout.category,
        name: workout.name,
        sets: workout.sets,
        reps: workout.reps,
        load: workout.load,
        notes: workout.notes
      }
      : { ...EMPTY_WORKOUT_FORM, category: library.workout_categories[0] ?? "Other" });
    setShowWorkoutForm(true);
  }

  function closeWorkoutEditor() {
    setShowWorkoutForm(false);
    setEditingWorkoutId(null);
    setWorkoutForm(EMPTY_WORKOUT_FORM);
  }

  async function saveWorkout(event) {
    event.preventDefault();
    const name = workoutForm.name.trim();
    if (!name) {
      setStatus("An exercise name is required.");
      return;
    }
    const workout = {
      id: editingWorkoutId ?? createWellnessId("workout"),
      category: workoutForm.category.trim() || library.workout_categories[0] || "Other",
      name,
      sets: workoutForm.sets.trim(),
      reps: workoutForm.reps.trim(),
      load: workoutForm.load.trim(),
      notes: workoutForm.notes.trim()
    };
    const nextWorkouts = editingWorkoutId
      ? library.workouts.map((item) => item.id === editingWorkoutId ? workout : item)
      : [...library.workouts, workout];
    const saved = await persistLibrary({ ...library, workouts: nextWorkouts }, editingWorkoutId ? "Updated your workout library." : "Added an exercise to your workout library.");
    if (saved) {
      closeWorkoutEditor();
    }
  }

  async function removeWorkout(workout) {
    if (!window.confirm(`Remove ${workout.name} from your workout library?`)) {
      return;
    }
    const saved = await persistLibrary({
      ...library,
      workouts: library.workouts.filter((item) => item.id !== workout.id)
    }, `Removed ${workout.name} from your workout library.`);
    if (saved && editingWorkoutId === workout.id) {
      closeWorkoutEditor();
    }
  }

  async function addWorkoutCategory(event) {
    event.preventDefault();
    const category = newWorkoutCategory.trim();
    if (!category) {
      return;
    }
    if (hasCategory(library.workout_categories, category)) {
      setStatus("That workout category already exists.");
      return;
    }
    const saved = await persistLibrary({
      ...library,
      workout_categories: [...library.workout_categories, category]
    }, `Added the ${category} workout category.`);
    if (saved) {
      setNewWorkoutCategory("");
      setShowWorkoutCategoryForm(false);
    }
  }

  function openFoodEditor(food = null) {
    setEditingFoodId(food?.id ?? null);
    setFoodForm(food
      ? {
        category: food.category,
        name: food.name,
        serving: food.serving,
        protein_grams: String(food.protein_grams),
        notes: food.notes
      }
      : { ...EMPTY_FOOD_FORM, category: library.food_categories[0] ?? "Other" });
    setShowFoodForm(true);
  }

  function closeFoodEditor() {
    setShowFoodForm(false);
    setEditingFoodId(null);
    setFoodForm(EMPTY_FOOD_FORM);
  }

  async function saveFood(event) {
    event.preventDefault();
    const name = foodForm.name.trim();
    if (!name) {
      setStatus("A food name is required.");
      return;
    }
    const protein = Number(foodForm.protein_grams);
    const food = {
      id: editingFoodId ?? createWellnessId("food"),
      category: foodForm.category.trim() || library.food_categories[0] || "Other",
      name,
      serving: foodForm.serving.trim(),
      protein_grams: Number.isFinite(protein) ? Math.max(0, Math.min(500, protein)) : 0,
      notes: foodForm.notes.trim()
    };
    const nextFoods = editingFoodId
      ? library.foods.map((item) => item.id === editingFoodId ? food : item)
      : [...library.foods, food];
    const saved = await persistLibrary({ ...library, foods: nextFoods }, editingFoodId ? "Updated your food library." : "Added a food to your library.");
    if (saved) {
      closeFoodEditor();
    }
  }

  async function removeFood(food) {
    if (!window.confirm(`Remove ${food.name} from your food library?`)) {
      return;
    }
    const saved = await persistLibrary({
      ...library,
      foods: library.foods.filter((item) => item.id !== food.id)
    }, `Removed ${food.name} from your food library.`);
    if (saved && editingFoodId === food.id) {
      closeFoodEditor();
    }
  }

  async function addFoodCategory(event) {
    event.preventDefault();
    const category = newFoodCategory.trim();
    if (!category) {
      return;
    }
    if (hasCategory(library.food_categories, category)) {
      setStatus("That food category already exists.");
      return;
    }
    const saved = await persistLibrary({
      ...library,
      food_categories: [...library.food_categories, category]
    }, `Added the ${category} food category.`);
    if (saved) {
      setNewFoodCategory("");
      setShowFoodCategoryForm(false);
    }
  }

  async function saveProteinGoal(event) {
    event.preventDefault();
    const goal = Number(proteinGoalDraft);
    const saved = await persistLibrary({
      ...library,
      protein_goal_grams: Number.isFinite(goal) ? Math.max(0, Math.min(500, goal)) : 0
    }, "Saved your optional personal protein goal.");
    if (!saved) {
      setProteinGoalDraft(String(library.protein_goal_grams));
    }
  }

  async function useBalancedWorkouts() {
    const workoutIds = selectBalancedWorkouts(library);
    if (!workoutIds.length) {
      setStatus("Add an exercise to your library before creating today’s workout list.");
      return;
    }
    await persistDaily({
      ...daily,
      workout_ids: workoutIds,
      completed_workout_ids: []
    }, "Saved a balanced workout list for today. Review it before you begin.");
  }

  async function toggleWorkoutSelection(workoutId, selected) {
    await persistDaily({
      ...daily,
      workout_ids: toggleId(daily.workout_ids, workoutId, selected),
      completed_workout_ids: selected ? daily.completed_workout_ids : toggleId(daily.completed_workout_ids, workoutId, false)
    }, "Updated today’s workout list.");
  }

  async function toggleWorkoutCompletion(workoutId, completed) {
    await persistDaily({
      ...daily,
      completed_workout_ids: toggleId(daily.completed_workout_ids, workoutId, completed)
    }, "Updated today’s workout progress.");
  }

  async function useBalancedMeals() {
    const mealSlots = buildBalancedMealSlots(library);
    const itemCount = Object.values(mealSlots).reduce((total, items) => total + items.length, 0);
    if (!itemCount) {
      setStatus("Add foods to your library before creating today’s meal plan.");
      return;
    }
    await persistDaily({
      ...daily,
      meal_slots: mealSlots,
      completed_meal_keys: []
    }, "Saved a balanced meal checklist for today. Adjust every slot to match your own needs.");
  }

  async function toggleMealSelection(slot, foodId, selected) {
    const nextSlot = toggleId(daily.meal_slots[slot], foodId, selected);
    const nextChecks = selected
      ? daily.completed_meal_keys
      : toggleId(daily.completed_meal_keys, mealCheckKey(slot, foodId), false);
    await persistDaily({
      ...daily,
      meal_slots: { ...daily.meal_slots, [slot]: nextSlot },
      completed_meal_keys: nextChecks
    }, `Updated the ${slot.toLowerCase()} meal slot.`);
  }

  async function toggleMealCompletion(slot, foodId, completed) {
    await persistDaily({
      ...daily,
      completed_meal_keys: toggleId(daily.completed_meal_keys, mealCheckKey(slot, foodId), completed)
    }, "Updated today’s meal progress.");
  }

  async function resetProgress() {
    if (!window.confirm("Clear today’s completion checks? Your selected workout and meal plan will remain.")) {
      return;
    }
    await persistDaily({
      ...daily,
      completed_workout_ids: [],
      completed_meal_keys: []
    }, "Cleared today’s completion checks.");
  }

  async function clearTodayPlan() {
    if (!window.confirm("Clear today’s selected workout and meal plan? This cannot be undone.")) {
      return;
    }
    await persistDaily(createEmptyDailyWellness(), "Cleared today’s wellness plan.");
  }

  return (
    <section className="wellness-panel premium-wellness-panel">
      <header className="wellness-hero">
        <div>
          <p className="eyebrow">Student wellness</p>
          <h1>Keep workouts, meals, and study energy in one calm daily tracker.</h1>
          <p>Build a private library, choose what fits today, and track only the progress you want to keep.</p>
          <span className="wellness-hero-date"><i className="fa-regular fa-calendar" aria-hidden="true" /> {formattedToday}</span>
        </div>
        <aside>
          <i className="fa-solid fa-heart-pulse" aria-hidden="true" />
          <strong>Local and student-owned</strong>
          <span>Wellness data stays in canonical Student OS state. Nothing changes your academic planner automatically.</span>
        </aside>
      </header>

      <p className="wellness-status" role="status" aria-live="polite">{isLoading ? "Loading local wellness records…" : status}</p>
      {error && <p className="error-message" role="alert">{error}</p>}

      <section className="wellness-summary-grid" aria-label="Today’s wellness summary">
        <MetricCard icon="fa-dumbbell" label="Workout progress" value={`${summary.workout_percent}%`} detail={summary.workout_total ? `${summary.workout_completed} of ${summary.workout_total} selected items complete` : "Choose a workout list for today"} tone="mint" />
        <MetricCard icon="fa-utensils" label="Meal progress" value={`${summary.meal_percent}%`} detail={summary.meal_total ? `${summary.meal_completed} of ${summary.meal_total} meal items checked` : "Build meal slots from your food library"} tone="gold" />
        <MetricCard icon="fa-drumstick-bite" label="Protein logged" value={`${summary.protein_logged_grams} g`} detail={summary.protein_goal_grams ? `${summary.protein_percent}% of your personal ${summary.protein_goal_grams} g goal` : "Set an optional goal in Food library"} tone="coral" />
        <MetricCard icon="fa-box-open" label="Your library" value={`${library.workouts.length + library.foods.length}`} detail={`${library.workouts.length} exercises · ${library.foods.length} foods`} tone="sky" />
      </section>

      <section className="wellness-today-grid" aria-label="Today’s wellness plan">
        <article className="wellness-card wellness-today-card">
          <div className="wellness-card-heading">
            <div>
              <p className="eyebrow">Today</p>
              <h2>Workout checklist</h2>
            </div>
            <span>{summary.workout_completed}/{summary.workout_total || 0}</span>
          </div>
          <ProgressBar value={summary.workout_percent} label="Workout progress" />
          <div className="wellness-card-actions">
            <button type="button" onClick={() => void useBalancedWorkouts()} disabled={busy}>Use balanced pick</button>
            <button type="button" className="secondary-button" onClick={() => void resetProgress()} disabled={busy || (!daily.completed_workout_ids.length && !daily.completed_meal_keys.length)}>Reset progress</button>
          </div>
          {todayWorkouts.length ? (
            <div className="wellness-check-list">
              {todayWorkouts.map((workout) => {
                const completed = daily.completed_workout_ids.includes(workout.id);
                return (
                  <label key={workout.id} className={completed ? "is-complete" : ""}>
                    <input type="checkbox" checked={completed} onChange={(event) => void toggleWorkoutCompletion(workout.id, event.target.checked)} disabled={busy} aria-label={`Mark ${workout.name} complete today`} />
                    <span>
                      <strong>{workout.name}</strong>
                      <small>{workout.category}{workout.sets ? ` · ${workout.sets} sets` : ""}{workout.reps ? ` · ${workout.reps}` : ""}{workout.load ? ` · ${workout.load}` : ""}</small>
                    </span>
                  </label>
                );
              })}
            </div>
          ) : <EmptyWellnessState icon="fa-person-walking" text="No exercises are selected for today. Start with a balanced pick, then make it your own." />}
          <details className="wellness-picker">
            <summary>Choose exercises for today</summary>
            <div>
              {library.workouts.map((workout) => (
                <label key={workout.id}>
                  <input type="checkbox" checked={daily.workout_ids.includes(workout.id)} onChange={(event) => void toggleWorkoutSelection(workout.id, event.target.checked)} disabled={busy} aria-label={`Include ${workout.name} in today’s workout`} />
                  <span><strong>{workout.name}</strong><small>{workout.category}</small></span>
                </label>
              ))}
            </div>
          </details>
        </article>

        <article className="wellness-card wellness-today-card wellness-meal-today-card">
          <div className="wellness-card-heading">
            <div>
              <p className="eyebrow">Today</p>
              <h2>Meal checklist</h2>
            </div>
            <span>{summary.meal_completed}/{summary.meal_total || 0}</span>
          </div>
          <ProgressBar value={summary.meal_percent} label="Meal progress" />
          <div className="wellness-card-actions">
            <button type="button" onClick={() => void useBalancedMeals()} disabled={busy}>Build balanced meals</button>
            <button type="button" className="secondary-button" onClick={() => void clearTodayPlan()} disabled={busy || (!daily.workout_ids.length && summary.meal_total === 0)}>Clear today</button>
          </div>
          <div className="wellness-meal-grid">
            {WELLNESS_MEAL_SLOTS.map((slot) => (
              <article key={slot}>
                <h3>{slot}</h3>
                {todayMeals[slot].length ? todayMeals[slot].map((food) => {
                  const checkKey = mealCheckKey(slot, food.id);
                  const completed = daily.completed_meal_keys.includes(checkKey);
                  return (
                    <label key={food.id} className={completed ? "is-complete" : ""}>
                      <input type="checkbox" checked={completed} onChange={(event) => void toggleMealCompletion(slot, food.id, event.target.checked)} disabled={busy} aria-label={`Mark ${food.name} eaten for ${slot}`} />
                      <span><strong>{food.name}</strong><small>{food.protein_grams ? `${food.protein_grams} g protein per serving` : "No protein amount entered"}</small></span>
                    </label>
                  );
                }) : <p>Choose items below.</p>}
              </article>
            ))}
          </div>
          <details className="wellness-picker wellness-meal-picker">
            <summary>Choose foods for meal slots</summary>
            <div>
              {WELLNESS_MEAL_SLOTS.map((slot) => (
                <fieldset key={slot}>
                  <legend>{slot}</legend>
                  {library.foods.map((food) => (
                    <label key={food.id}>
                      <input type="checkbox" checked={daily.meal_slots[slot].includes(food.id)} onChange={(event) => void toggleMealSelection(slot, food.id, event.target.checked)} disabled={busy} aria-label={`Include ${food.name} for ${slot}`} />
                      <span><strong>{food.name}</strong><small>{food.category}{food.protein_grams ? ` · ${food.protein_grams} g` : ""}</small></span>
                    </label>
                  ))}
                </fieldset>
              ))}
            </div>
          </details>
        </article>

        <aside className="wellness-rhythm-card" aria-label="Daily wellness rhythm">
          <div>
            <p className="eyebrow">Keep it realistic</p>
            <h2>A rhythm that fits study</h2>
          </div>
          <ol>
            <li><span>Morning</span><strong>Hydrate and choose food for your first study block.</strong></li>
            <li><span>Movement</span><strong>Use today’s workout checklist, not an automatic schedule.</strong></li>
            <li><span>Meals</span><strong>Log only the items you actually eat.</strong></li>
            <li><span>Evening</span><strong>Review energy and reset tomorrow with your own choices.</strong></li>
          </ol>
          <p><i className="fa-solid fa-shield-heart" aria-hidden="true" /> This tracker is not medical advice. Adapt it to your clinician, culture, budget, access, and recovery needs.</p>
        </aside>
      </section>

      <section className="wellness-library-grid" aria-label="Wellness libraries">
        <article className="wellness-card wellness-library-card">
          <div className="wellness-card-heading">
            <div>
              <p className="eyebrow">Your exercise library</p>
              <h2>Workouts</h2>
            </div>
            <div className="wellness-card-actions compact">
              <button type="button" className="secondary-button" onClick={() => setShowWorkoutCategoryForm((visible) => !visible)} disabled={busy}>Add category</button>
              <button type="button" onClick={() => openWorkoutEditor()} disabled={busy}>Add workout</button>
            </div>
          </div>
          {showWorkoutCategoryForm && (
            <form className="wellness-category-form" onSubmit={(event) => void addWorkoutCategory(event)}>
              <label><span>New workout category</span><input value={newWorkoutCategory} onChange={(event) => setNewWorkoutCategory(event.target.value)} maxLength="80" autoFocus /></label>
              <button type="submit" disabled={busy}>Save category</button>
              <button type="button" className="text-button" onClick={() => setShowWorkoutCategoryForm(false)} disabled={busy}>Cancel</button>
            </form>
          )}
          {showWorkoutForm && (
            <form className="wellness-editor-form" onSubmit={(event) => void saveWorkout(event)}>
              <label><span>Exercise</span><input value={workoutForm.name} onChange={(event) => setWorkoutForm((current) => ({ ...current, name: event.target.value }))} maxLength="120" required autoFocus /></label>
              <label><span>Category</span><select value={workoutForm.category} onChange={(event) => setWorkoutForm((current) => ({ ...current, category: event.target.value }))}>{library.workout_categories.map((category) => <option key={category} value={category}>{category}</option>)}</select></label>
              <label><span>Sets</span><input value={workoutForm.sets} onChange={(event) => setWorkoutForm((current) => ({ ...current, sets: event.target.value }))} maxLength="40" placeholder="3" /></label>
              <label><span>Reps or duration</span><input value={workoutForm.reps} onChange={(event) => setWorkoutForm((current) => ({ ...current, reps: event.target.value }))} maxLength="60" placeholder="10 or 20 min" /></label>
              <label><span>Load or intensity</span><input value={workoutForm.load} onChange={(event) => setWorkoutForm((current) => ({ ...current, load: event.target.value }))} maxLength="80" placeholder="Bodyweight" /></label>
              <label><span>Notes</span><input value={workoutForm.notes} onChange={(event) => setWorkoutForm((current) => ({ ...current, notes: event.target.value }))} maxLength="320" placeholder="Optional" /></label>
              <div><button type="submit" disabled={busy}>{editingWorkoutId ? "Update workout" : "Save workout"}</button><button type="button" className="text-button" onClick={closeWorkoutEditor} disabled={busy}>Cancel</button></div>
            </form>
          )}
          <div className="wellness-category-grid">
            {library.workout_categories.map((category) => <WorkoutCategory key={category} category={category} workouts={library.workouts.filter((workout) => workout.category === category)} busy={busy} onEdit={openWorkoutEditor} onRemove={removeWorkout} />)}
          </div>
        </article>

        <article className="wellness-card wellness-library-card">
          <div className="wellness-card-heading">
            <div>
              <p className="eyebrow">Your food library</p>
              <h2>Meals</h2>
            </div>
            <div className="wellness-card-actions compact">
              <button type="button" className="secondary-button" onClick={() => setShowFoodCategoryForm((visible) => !visible)} disabled={busy}>Add category</button>
              <button type="button" onClick={() => openFoodEditor()} disabled={busy}>Add food</button>
            </div>
          </div>
          <form className="wellness-protein-goal" onSubmit={(event) => void saveProteinGoal(event)}>
            <label><span>Optional personal protein goal (g)</span><input type="number" min="0" max="500" value={proteinGoalDraft} onChange={(event) => setProteinGoalDraft(event.target.value)} disabled={busy} /></label>
            <button type="submit" className="secondary-button" disabled={busy}>Save goal</button>
          </form>
          {showFoodCategoryForm && (
            <form className="wellness-category-form" onSubmit={(event) => void addFoodCategory(event)}>
              <label><span>New food category</span><input value={newFoodCategory} onChange={(event) => setNewFoodCategory(event.target.value)} maxLength="80" autoFocus /></label>
              <button type="submit" disabled={busy}>Save category</button>
              <button type="button" className="text-button" onClick={() => setShowFoodCategoryForm(false)} disabled={busy}>Cancel</button>
            </form>
          )}
          {showFoodForm && (
            <form className="wellness-editor-form" onSubmit={(event) => void saveFood(event)}>
              <label><span>Food</span><input value={foodForm.name} onChange={(event) => setFoodForm((current) => ({ ...current, name: event.target.value }))} maxLength="120" required autoFocus /></label>
              <label><span>Category</span><select value={foodForm.category} onChange={(event) => setFoodForm((current) => ({ ...current, category: event.target.value }))}>{library.food_categories.map((category) => <option key={category} value={category}>{category}</option>)}</select></label>
              <label><span>Serving</span><input value={foodForm.serving} onChange={(event) => setFoodForm((current) => ({ ...current, serving: event.target.value }))} maxLength="120" placeholder="1 serving" /></label>
              <label><span>Protein per serving (g)</span><input type="number" min="0" max="500" step="0.1" value={foodForm.protein_grams} onChange={(event) => setFoodForm((current) => ({ ...current, protein_grams: event.target.value }))} /></label>
              <label><span>Notes</span><input value={foodForm.notes} onChange={(event) => setFoodForm((current) => ({ ...current, notes: event.target.value }))} maxLength="320" placeholder="Optional" /></label>
              <div><button type="submit" disabled={busy}>{editingFoodId ? "Update food" : "Save food"}</button><button type="button" className="text-button" onClick={closeFoodEditor} disabled={busy}>Cancel</button></div>
            </form>
          )}
          <div className="wellness-category-grid food-categories">
            {library.food_categories.map((category) => <FoodCategory key={category} category={category} foods={library.foods.filter((food) => food.category === category)} busy={busy} onEdit={openFoodEditor} onRemove={removeFood} />)}
          </div>
        </article>
      </section>

      <details className="wellness-plan-assistant">
        <summary><span><i className="fa-solid fa-compass" aria-hidden="true" /> Optional wellness plan assistant</span><small>Advisory only · planner changes always stay separate</small></summary>
        <div className="wellness-plan-assistant-body">
          <p>Use your own profile for a deterministic workout and meal preview. It checks your saved class schedule but never creates or changes planner blocks.</p>
          <form className="wellness-form" onSubmit={(event) => void buildAdvisoryPlan(event)}>
            <label><span>Goal</span><select value={profile.goal} onChange={(event) => updateProfileField("goal", event.target.value)}>{GOALS.map((goal) => <option value={goal.value} key={goal.value}>{goal.label}</option>)}</select></label>
            <label><span>Activity level</span><select value={profile.activity_level} onChange={(event) => updateProfileField("activity_level", event.target.value)}>{ACTIVITY_LEVELS.map((level) => <option value={level.value} key={level.value}>{level.label}</option>)}</select></label>
            <label><span>Sex (for energy estimate)</span><select value={profile.sex} onChange={(event) => updateProfileField("sex", event.target.value)}>{SEXES.map((sex) => <option value={sex.value} key={sex.value}>{sex.label}</option>)}</select></label>
            <label><span>Age</span><input type="number" min="10" max="120" value={profile.age_years} onChange={(event) => updateProfileNumber("age_years", event.target.value)} /></label>
            <label><span>Weight (kg)</span><input type="number" min="20" max="400" value={profile.weight_kg} onChange={(event) => updateProfileNumber("weight_kg", event.target.value)} /></label>
            <label><span>Height (cm)</span><input type="number" min="100" max="250" value={profile.height_cm} onChange={(event) => updateProfileNumber("height_cm", event.target.value)} /></label>
            <label><span>Training days / week</span><input type="number" min="0" max="7" value={profile.training_days_per_week} onChange={(event) => updateProfileNumber("training_days_per_week", event.target.value)} /></label>
            <label><span>Preferred start</span><input type="time" value={profile.preferred_session_start} onChange={(event) => updateProfileField("preferred_session_start", event.target.value)} /></label>
            <div className="wellness-actions"><button type="button" className="secondary-button" onClick={() => void saveProfile()} disabled={busy}>Save profile locally</button><button type="submit" disabled={busy}>{isBuilding ? "Building…" : "Build advisory plan"}</button></div>
          </form>
          {plan && <AdvisoryPlan plan={plan} />}
        </div>
      </details>
    </section>
  );
}

function MetricCard({ icon, label, value, detail, tone }) {
  return (
    <article className={`wellness-metric-card tone-${tone}`}>
      <i className={`fa-solid ${icon}`} aria-hidden="true" />
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{detail}</small>
    </article>
  );
}

function ProgressBar({ value, label }) {
  return <div className="wellness-progress" aria-label={label}><span style={{ width: `${Math.max(0, Math.min(100, value))}%` }} /></div>;
}

function EmptyWellnessState({ icon, text }) {
  return <div className="wellness-empty"><i className={`fa-solid ${icon}`} aria-hidden="true" /><p>{text}</p></div>;
}

function WorkoutCategory({ category, workouts, busy, onEdit, onRemove }) {
  return (
    <article className="wellness-category-card">
      <div><h3>{category}</h3><span>{workouts.length} {workouts.length === 1 ? "exercise" : "exercises"}</span></div>
      {workouts.length ? <ul>{workouts.map((workout) => <li key={workout.id}><div><strong>{workout.name}</strong><small>{workout.sets ? `${workout.sets} sets` : ""}{workout.reps ? `${workout.sets ? " · " : ""}${workout.reps}` : ""}{workout.load ? `${workout.sets || workout.reps ? " · " : ""}${workout.load}` : ""}{workout.notes ? ` · ${workout.notes}` : ""}</small></div><span><button type="button" className="text-button" onClick={() => onEdit(workout)} disabled={busy}>Edit</button><button type="button" className="text-button danger-text" onClick={() => void onRemove(workout)} disabled={busy}>Remove</button></span></li>)}</ul> : <p>Add an exercise when this category fits your routine.</p>}
    </article>
  );
}

function FoodCategory({ category, foods, busy, onEdit, onRemove }) {
  return (
    <article className="wellness-category-card">
      <div><h3>{category}</h3><span>{foods.length} {foods.length === 1 ? "food" : "foods"}</span></div>
      {foods.length ? <ul>{foods.map((food) => <li key={food.id}><div><strong>{food.name}</strong><small>{food.serving || "Serving not entered"}{food.protein_grams ? ` · ${food.protein_grams} g protein` : ""}{food.notes ? ` · ${food.notes}` : ""}</small></div><span><button type="button" className="text-button" onClick={() => onEdit(food)} disabled={busy}>Edit</button><button type="button" className="text-button danger-text" onClick={() => void onRemove(food)} disabled={busy}>Remove</button></span></li>)}</ul> : <p>Add a food when it belongs in this category.</p>}
    </article>
  );
}

function AdvisoryPlan({ plan }) {
  const nutrition = plan?.nutrition ?? {};
  const workout = plan?.workout ?? {};
  const meals = plan?.meals?.meals ?? [];
  const sessions = plan?.workout?.sessions ?? [];
  const conflicts = Array.isArray(plan?.plannerConflicts) ? plan.plannerConflicts : [];
  return (
    <section className="wellness-advisory-results" aria-live="polite">
      {plan.disclaimer && <p className="wellness-disclaimer" role="note">{plan.disclaimer}</p>}
      <article><h3>Daily nutrition preview</h3><ul><li><strong>{nutrition.targetCalories ?? "—"}</strong><span>Target kcal</span></li><li><strong>{nutrition.proteinGrams ?? "—"} g</strong><span>Protein</span></li><li><strong>{nutrition.carbohydrateGrams ?? "—"} g</strong><span>Carbs</span></li><li><strong>{nutrition.fatGrams ?? "—"} g</strong><span>Fat</span></li></ul></article>
      <article><h3>Workout preview</h3><p>{workout.weeklyMinutes ?? 0} planned minutes across the advisory week.</p><ul className="wellness-advisory-list">{sessions.map((session) => <li key={`${session.day}-${session.focus}`}><strong>{session.day}</strong><span>{session.focus} · {session.durationMinutes} min · {session.intensity}</span></li>)}</ul></article>
      <article><h3>Sample meals</h3><ul className="wellness-advisory-list">{meals.map((meal) => <li key={meal.name}><strong>{meal.name}</strong><span>{meal.calories} kcal · {meal.proteinGrams} g protein</span></li>)}</ul></article>
      <article><h3>Schedule check</h3>{conflicts.length ? <ul className="wellness-advisory-list">{conflicts.map((conflict, index) => <li key={`${conflict.day}-${index}`}><strong>{conflict.day}</strong><span>{conflict.workoutTitle} overlaps {conflict.conflictsWith}.</span></li>)}</ul> : <p>No class-schedule conflicts were found in this advisory preview.</p>}<small>Use Routine Coach if you decide to turn any workout block into a planner proposal.</small></article>
    </section>
  );
}

function activeRecordValue(records, key) {
  return records.find((record) => record?.key === key && !record.deleted)?.value;
}

function hasCategory(categories, candidate) {
  return categories.some((category) => category.toLowerCase() === candidate.toLowerCase());
}
