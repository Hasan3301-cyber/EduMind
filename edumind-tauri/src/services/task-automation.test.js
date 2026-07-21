import { describe, expect, it } from "vitest";

import {
  AUTOMATION_EXECUTIONS_RECORD_KEY,
  AUTOMATION_RECORD_PREFIX,
  AUTOMATION_DEFINITIONS_RECORD_KEY,
  buildAutomationDefinition,
  buildAutomationQueue,
  createAutomationDraft,
  createAutomationExecution,
  formatAutomationSchedule,
  isAutomationDueOnDate,
  isAutomationRecordKey,
  normalizeAutomationDefinitions,
  normalizeAutomationExecutions,
  updateAutomationExecution,
  upsertAutomationExecution
} from "./task-automation";

describe("task automation data", () => {
  it("keeps automation records separate from general Student OS cards", () => {
    expect(AUTOMATION_RECORD_PREFIX).toBe("automation.");
    expect(AUTOMATION_DEFINITIONS_RECORD_KEY).toBe("automation.definitions.v1");
    expect(AUTOMATION_EXECUTIONS_RECORD_KEY).toBe("automation.executions.v1");
    expect(isAutomationRecordKey("automation.executions.v1")).toBe(true);
    expect(isAutomationRecordKey("weekly-priority")).toBe(false);
  });

  it("creates only due weekly and interval occurrences", () => {
    const weekly = buildAutomationDefinition({
      ...createAutomationDraft({ title: "Planner review", action_id: "review-planner", cadence: "weekly", weekdays: ["monday"], start_date: "2026-07-20" }),
      title: "Planner review"
    }, { id: "planner-review", now: new Date("2026-07-20T10:00:00Z") });
    const interval = buildAutomationDefinition({
      ...createAutomationDraft({ title: "Research check", action_id: "research-gaps", cadence: "interval", interval_days: 3, start_date: "2026-07-20" }),
      title: "Research check"
    }, { id: "research-check", now: new Date("2026-07-20T10:00:00Z") });

    expect(isAutomationDueOnDate(weekly, "2026-07-20")).toBe(true);
    expect(isAutomationDueOnDate(weekly, "2026-07-21")).toBe(false);
    expect(isAutomationDueOnDate(interval, "2026-07-23")).toBe(true);
    expect(isAutomationDueOnDate(interval, "2026-07-22")).toBe(false);
    expect(buildAutomationQueue({ definitions: [weekly, interval] }, { executions: [] }, "2026-07-20")).toHaveLength(2);
  });

  it("bounds untrusted definitions and retains only allow-listed actions", () => {
    const state = normalizeAutomationDefinitions({
      definitions: [
        { id: "safe", title: "Study", action_id: "review-study-insights", cadence: "daily", start_date: "2026-07-20" },
        { id: "unsafe", title: "Run shell", action_id: "run-any-command", cadence: "daily", start_date: "2026-07-20" }
      ]
    }, "2026-07-20");

    expect(state.definitions).toHaveLength(1);
    expect(state.definitions[0].action_id).toBe("review-study-insights");
  });

  it("keeps a short explicit audit trail for a completed execution", () => {
    const definition = buildAutomationDefinition({
      ...createAutomationDraft({ title: "Study refresh", action_id: "refresh-study-recommendations", cadence: "daily", start_date: "2026-07-20" }),
      title: "Study refresh"
    }, { id: "study-refresh", now: new Date("2026-07-20T10:00:00Z") });
    const occurrence = buildAutomationQueue({ definitions: [definition] }, { executions: [] }, "2026-07-20")[0];
    const running = createAutomationExecution(occurrence, { status: "running", message: "Confirmed by the student.", now: new Date("2026-07-20T10:00:00Z") });
    const completed = updateAutomationExecution(running, {
      status: "completed",
      message: "Completed without changing the planner.",
      summary: "2 study priorities and 45 available minutes reviewed.",
      now: new Date("2026-07-20T10:01:00Z")
    });
    const saved = upsertAutomationExecution({ executions: [] }, completed, "2026-07-20");

    expect(normalizeAutomationExecutions(saved, "2026-07-20").executions[0]).toMatchObject({
      status: "completed",
      logs: expect.arrayContaining([expect.objectContaining({ message: "Confirmed by the student." })])
    });
    expect(formatAutomationSchedule(definition)).toContain("Every day");
  });
});
