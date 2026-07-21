import { describe, expect, it } from "vitest";

import {
  canonicalWeeklySchedule,
  formatScheduleTime,
  hasCanonicalSchedule,
  scheduleEntryCount
} from "./weekly-schedule";

describe("canonical weekly schedule", () => {
  it("maps planner output into a stable Monday-to-Sunday dashboard view", () => {
    const scheduleDays = canonicalWeeklySchedule({
      days: [
        {
          day: "Wed",
          entries: [
            { id: "lab", title: "Physics lab", start: "14:00", end: "15:30", source: "planner" },
            { id: "review", title: "Formula review", start: "09:00", end: "09:30", source: "routine" }
          ]
        },
        { day: "Monday", entries: [{ id: "lecture", title: "Calculus lecture", start: "09:00", end: "10:00" }] },
        { day: "invalid", entries: [{ id: "hidden", title: "Ignore this", start: "08:00" }] }
      ]
    }, new Date(2026, 6, 21, 12));

    expect(scheduleDays.map((day) => day.day)).toEqual(["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]);
    expect(scheduleDays[0]).toMatchObject({ dateKey: "2026-07-20", entries: [{ title: "Calculus lecture" }] });
    expect(scheduleDays[2].entries.map((entry) => entry.title)).toEqual(["Formula review", "Physics lab"]);
    expect(scheduleEntryCount(scheduleDays)).toBe(3);
  });

  it("exposes only a real canonical response and keeps invalid times safe", () => {
    expect(hasCanonicalSchedule(null)).toBe(false);
    expect(hasCanonicalSchedule({ days: [] })).toBe(true);
    expect(formatScheduleTime("18:05")).toBe("6:05 PM");
    expect(formatScheduleTime("not-a-time")).toBe("Time TBD");
  });
});
