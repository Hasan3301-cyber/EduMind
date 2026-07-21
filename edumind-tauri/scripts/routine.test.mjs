import assert from "node:assert/strict";
import test from "node:test";

import {
  findScheduleConflicts,
  isValidTimeRange,
  normalizeWeeklySchedule,
  parseRoutineText,
  upsertPlannerRecord
} from "../src/services/routine.js";

test("normalizes weekly blocks into deterministic day and time order", () => {
  const schedule = normalizeWeeklySchedule([
    { key: "second", value: { day: "Monday", title: "Later", start: "11:00", end: "12:00" } },
    { key: "first", value: { day: "Monday", title: "Earlier", start: "09:00", end: "10:00" } }
  ]);

  assert.equal(schedule[0].day, "monday");
  assert.deepEqual(schedule[0].entries.map((entry) => entry.title), ["Earlier", "Later"]);
  assert.equal(schedule.at(-1).day, "sunday");
});

test("upserts planner records without mutating the input list", () => {
  const records = [{ key: "block", value: { title: "Old" } }];
  const next = upsertPlannerRecord(records, "block", { title: "New" }, "2026-01-01T00:00:00Z");

  assert.equal(records[0].value.title, "Old");
  assert.equal(next[0].value.title, "New");
});

test("parses timetable text into canonical planner blocks", () => {
  const parsed = parseRoutineText([
    "Monday 9:00-10:00 Calculus",
    "Wed | 14:00–15:30 | Physics lab",
    "not a timetable row"
  ].join("\n"));

  assert.deepEqual(parsed.blocks, [
    {
      kind: "schedule-block",
      day: "Monday",
      title: "Calculus",
      start: "09:00",
      end: "10:00",
      source: "timetable-import"
    },
    {
      kind: "schedule-block",
      day: "Wednesday",
      title: "Physics lab",
      start: "14:00",
      end: "15:30",
      source: "timetable-import"
    }
  ]);
  assert.deepEqual(parsed.unparsedLines, ["not a timetable row"]);
});

test("flags invalid ranges and overlapping planner blocks", () => {
  const records = [
    { key: "first", value: { day: "Monday", title: "Calculus", start: "09:00", end: "10:30" } },
    { key: "second", value: { day: "Monday", title: "Physics", start: "10:00", end: "11:00" } }
  ];

  assert.equal(isValidTimeRange("09:00", "10:00"), true);
  assert.equal(isValidTimeRange("10:00", "10:00"), false);
  assert.deepEqual(findScheduleConflicts(records), [
    { day: "monday", firstId: "first", secondId: "second" }
  ]);
});
