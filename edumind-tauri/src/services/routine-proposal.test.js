import { describe, expect, it } from "vitest";

import {
  createPlannerImageRecordValue,
  createRoutineRecordValue,
  findRoutineProposalConflicts,
  mergeRoutineImageObservations,
  parseRoutineProposal,
  routineBlockDuration
} from "./routine-proposal";

describe("parseRoutineProposal", () => {
  it("keeps only validated optional routine blocks", () => {
    const response = parseRoutineProposal(JSON.stringify({
      title: "Calculus revision routine",
      summary: "Protect two short recall sessions.",
      assumptions: ["The commute is 15 minutes."],
      blocks: [
        {
          day: "Monday",
          start: "10:00",
          end: "10:45",
          title: "Limits retrieval practice",
          kind: "study",
          detail: "Work from your approved notes."
        },
        {
          day: "Monday",
          start: "11:00",
          end: "11:10",
          title: "Too short",
          kind: "study"
        },
        {
          day: "Monday",
          start: "12:00",
          end: "12:45",
          title: "Invented class",
          kind: "class"
        }
      ]
    }));

    expect(response.proposal?.title).toBe("Calculus revision routine");
    expect(response.proposal?.blocks).toHaveLength(1);
    expect(response.proposal?.blocks[0]).toMatchObject({
      day: "Monday",
      start: "10:00",
      end: "10:45",
      kind: "study"
    });
    expect(response.issues).toHaveLength(2);
    expect(routineBlockDuration(response.proposal?.blocks[0])).toBe(45);
  });

  it("rejects non-JSON output rather than guessing a planner mutation", () => {
    const response = parseRoutineProposal("Put a study session on Monday afternoon.");

    expect(response.proposal).toBeNull();
    expect(response.issues[0]).toMatch(/expected JSON format/i);
  });

  it("keeps validated timetable observations as a separate reviewable draft", () => {
    const response = parseRoutineProposal(JSON.stringify({
      title: "Image-assisted Monday routine",
      image_observations: [
        {
          day: "Monday",
          start: "11:30",
          end: "12:15",
          title: "Linear algebra tutorial",
          confidence: 0.88,
          detail: "Visible on the timetable image."
        },
        {
          day: "Monday",
          start: "13:00",
          end: "13:10",
          title: "Too short",
          confidence: 0.92
        }
      ],
      blocks: []
    }));

    expect(response.proposal?.blocks).toEqual([]);
    expect(response.proposal?.imageObservations).toEqual([
      expect.objectContaining({
        day: "Monday",
        start: "11:30",
        end: "12:15",
        confidence: 0.88
      })
    ]);
    expect(response.issues).toEqual(expect.arrayContaining([
      expect.stringMatching(/invalid timetable observation/i)
    ]));
  });
});

describe("findRoutineProposalConflicts", () => {
  it("reports canonical and proposal-to-proposal overlaps", () => {
    const blocks = [
      { id: "proposal-1", day: "Monday", start: "10:00", end: "10:45", title: "Limits recall", kind: "study" },
      { id: "proposal-2", day: "Monday", start: "10:30", end: "11:00", title: "Formula review", kind: "review" }
    ];
    const schedule = {
      days: [{
        day: "Monday",
        entries: [{ id: "class-1", title: "Calculus", start: "10:15", end: "11:15" }]
      }]
    };

    const conflicts = findRoutineProposalConflicts(blocks, schedule);

    expect(conflicts).toEqual(expect.arrayContaining([
      expect.objectContaining({ blockId: "proposal-1", existingId: "class-1", type: "overlap" }),
      expect.objectContaining({ blockId: "proposal-1", existingId: "proposal-2", type: "proposal-overlap" })
    ]));
  });

  it("tags applied planner values as AI-managed without changing fixed fields", () => {
    const value = createRoutineRecordValue({
      day: "Wednesday",
      start: "14:00",
      end: "14:45",
      title: "Physics recall",
      kind: "review"
    }, "plan-123");

    expect(value).toMatchObject({
      kind: "schedule-block",
      day: "Wednesday",
      start: "14:00",
      end: "14:45",
      routine_owner: "ai",
      routine_plan_id: "plan-123"
    });
  });

  it("keeps image detections in conflict checks until the learner confirms them", () => {
    const observations = [{
      id: "image-observation-1",
      day: "Monday",
      start: "11:30",
      end: "12:15",
      title: "Linear algebra tutorial",
      confidence: 0.88,
      detail: "Visible on the timetable image."
    }];
    const schedule = mergeRoutineImageObservations({
      days: [{ day: "Monday", entries: [{ id: "class-1", title: "Calculus", start: "09:00", end: "10:00" }] }]
    }, observations);
    const conflicts = findRoutineProposalConflicts([{
      id: "proposal-1",
      day: "Monday",
      start: "11:45",
      end: "12:30",
      title: "Tutorial prep",
      kind: "study"
    }], schedule);
    const imageValue = createPlannerImageRecordValue(observations[0], "import-456");

    expect(conflicts).toEqual(expect.arrayContaining([
      expect.objectContaining({ blockId: "proposal-1", existingId: "image-observation-1", type: "overlap" })
    ]));
    expect(imageValue).toMatchObject({
      source: "planner-image-ai",
      planner_owner: "image-import",
      planner_import_id: "import-456",
      planner_confidence: 0.88
    });
  });
});
