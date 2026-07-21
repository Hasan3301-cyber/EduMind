import { describe, expect, it } from "vitest";

import {
  PROJECT_NOTE_RECORD_PREFIX,
  createProjectNoteValue,
  isProjectNoteRecordKey,
  projectNoteStats,
  projectNotesFromRecords,
  safeHttpUrl,
  validateProjectNoteDraft
} from "./project-notes";

describe("project note records", () => {
  it("keeps prefixed project notes separate from other Student OS records", () => {
    const records = [
      {
        key: `${PROJECT_NOTE_RECORD_PREFIX}physics`,
        value: createProjectNoteValue({ title: "Physics lab", notes: "Measure uncertainty." }, "2026-07-21T09:00:00Z"),
        updated_at: "2026-07-21T11:00:00Z",
        deleted: false
      },
      {
        key: "weekly-priority",
        value: { title: "Review formulas" },
        updated_at: "2026-07-21T12:00:00Z",
        deleted: false
      },
      {
        key: `${PROJECT_NOTE_RECORD_PREFIX}removed`,
        value: { title: "Archived", notes: "Old." },
        updated_at: "2026-07-21T13:00:00Z",
        deleted: true
      }
    ];

    const projectNotes = projectNotesFromRecords(records);

    expect(isProjectNoteRecordKey("weekly-priority")).toBe(false);
    expect(projectNotes).toHaveLength(1);
    expect(projectNotes[0].value.title).toBe("Physics lab");
  });

  it("validates a project note before it becomes canonical state", () => {
    expect(validateProjectNoteDraft({ title: "", notes: "Observation" })).toMatchObject({ valid: false });
    expect(validateProjectNoteDraft({ title: "Chemistry report", notes: "Compare the control sample.", resource_url: "file:///private" })).toMatchObject({ valid: false });

    const result = validateProjectNoteDraft({
      title: "Chemistry report",
      notes: "Compare the control sample.",
      status: "complete",
      resource_url: "https://example.test/source"
    });

    expect(result).toMatchObject({ valid: true, value: { status: "complete" } });
    expect(safeHttpUrl("javascript:alert(1)")).toBe("");
  });

  it("summarizes current project work from valid note records", () => {
    const stats = projectNoteStats(projectNotesFromRecords([
      { key: `${PROJECT_NOTE_RECORD_PREFIX}one`, value: { title: "One", notes: "A", status: "active", resource_url: "https://example.test/a" }, updated_at: "2026-07-21T10:00:00Z" },
      { key: `${PROJECT_NOTE_RECORD_PREFIX}two`, value: { title: "Two", notes: "B", status: "complete" }, updated_at: "2026-07-21T11:00:00Z" }
    ]));

    expect(stats).toEqual({ total: 2, active: 1, complete: 1, resources: 1 });
  });
});
