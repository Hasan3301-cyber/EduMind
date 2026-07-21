import { expect, test } from "@playwright/test";

const RUN_ID = "11111111-1111-4111-8111-111111111111";
const TEST_GATEWAY = {
  baseUrl: "http://127.0.0.1:1421/mock-gateway",
  token: "test-token"
};
const TIMETABLE_IMAGE = {
  name: "monday-timetable.png",
  mimeType: "image/png",
  buffer: Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScL0bQAAAABJRU5ErkJggg==", "base64")
};
const RESEARCH_PROJECT_ID = "research-project-retrieval";
const RESEARCH_PROJECT = {
  id: RESEARCH_PROJECT_ID,
  topic: "Retrieval practice and retention",
  papers: [{
    id: "paper-retrieval",
    title: "Retrieval Practice Improves Long-Term Retention",
    authors: ["A. Researcher"],
    abstract_text: "A controlled study finds that repeated retrieval practice improves delayed retention outcomes.",
    year: 2025,
    citation_count: 42,
    open_access_url: "https://example.test/retrieval-practice.pdf",
    source_url: "https://example.test/retrieval-practice.pdf",
    source: "arxiv"
  }]
};

const INSIGHTS = {
  generated_at: "2026-07-19T10:00:00Z",
  available_minutes: 90,
  module_memory_records: 2,
  planner_conflicts: [],
  mastery: [
    {
      concept_id: "calculus: limits",
      mastery_percent: 38,
      retention_risk: "high",
      attempts: 4,
      correct: 2,
      days_since_review: 8
    }
  ],
  recommendations: [
    {
      concept_id: "calculus: limits",
      retention_risk: "high",
      priority_score: 92,
      recommended_minutes: 20,
      rationale: "The overdue card has a high retention risk."
    }
  ]
};

const PLANNER_SNAPSHOT = {
  page: "student-planner",
  records: [{
    key: "class-calculus",
    value: {
      kind: "schedule-block",
      day: "Monday",
      title: "Calculus lecture",
      start: "09:00",
      end: "10:00"
    },
    source: "desktop-planner",
    deleted: false,
    updated_at: "2026-07-19T10:00:00Z"
  }],
  count: 1,
  updated_at: "2026-07-19T10:00:00Z"
};

const PLANNER_SCHEDULE = {
  days: [
    { day: "Monday", entries: [{ id: "class-calculus", title: "Calculus lecture", start: "09:00", end: "10:00", source: "desktop-planner" }] },
    { day: "Tuesday", entries: [] },
    { day: "Wednesday", entries: [] },
    { day: "Thursday", entries: [] },
    { day: "Friday", entries: [] },
    { day: "Saturday", entries: [] },
    { day: "Sunday", entries: [] }
  ],
  ignored_records: 0,
  updated_at: "2026-07-19T10:00:00Z"
};

async function continueOffline(page) {
  await page.goto("/");
  await page.getByRole("button", { name: "Continue offline" }).click();
}

async function openMockWorkspace(page) {
  await installMockGateway(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Continue offline" }).click();
}

async function installMockGateway(page) {
  let cancelled = false;
  let srsReviewed = false;
  let indexedResearchDocuments = [];
  let researchProject = JSON.parse(JSON.stringify(RESEARCH_PROJECT));
  let groupStudyRoom = null;
  let groupStudyMessages = [];
  let groupStudyResources = [];
  await page.addInitScript((endpoint) => {
    window.__EDUMIND_TEST_GATEWAY_ENDPOINT__ = endpoint;
  }, TEST_GATEWAY);
  await page.route("**/mock-gateway/**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    const method = request.method();

    if (path.endsWith("/health")) {
      return fulfillJson(route, { status: "ok" });
    }
    if (path.endsWith("/group-study/groups") && method === "GET") {
      return fulfillJson(route, groupStudyRoom ? [groupStudyRoom] : []);
    }
    if (path.endsWith("/group-study/groups") && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      if (!body.title || !body.topic || !body.member_name) {
        return fulfillJson(route, { error: { message: "Fixture expected a room title, topic, and member name." } }, 400);
      }
      groupStudyRoom = {
        id: "group-study-fixture",
        title: body.title,
        topic: body.topic,
        invite_code: "STUDY-FIXTURECALCULUS",
        owner_name: body.member_name,
        members: [{ name: body.member_name, joined_at: "2026-07-20T12:00:00Z" }],
        created_at: "2026-07-20T12:00:00Z",
        updated_at: "2026-07-20T12:00:00Z"
      };
      groupStudyMessages = [];
      groupStudyResources = [];
      return fulfillJson(route, { group: groupStudyRoom, messages: groupStudyMessages, resources: groupStudyResources });
    }
    if (path.endsWith("/group-study/join") && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      if (!groupStudyRoom || body.invite_code !== groupStudyRoom.invite_code || !body.member_name) {
        return fulfillJson(route, { error: { message: "Fixture expected a valid Group Study invite code." } }, 404);
      }
      if (!groupStudyRoom.members.some((member) => member.name === body.member_name)) {
        groupStudyRoom.members.push({ name: body.member_name, joined_at: "2026-07-20T12:02:00Z" });
      }
      return fulfillJson(route, { group: groupStudyRoom, messages: groupStudyMessages, resources: groupStudyResources });
    }
    if (groupStudyRoom && path.endsWith(`/group-study/groups/${groupStudyRoom.id}`) && method === "GET") {
      return fulfillJson(route, { group: groupStudyRoom, messages: groupStudyMessages, resources: groupStudyResources });
    }
    if (groupStudyRoom && path.endsWith(`/group-study/groups/${groupStudyRoom.id}/ai`) && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      if (!body.member_name || !body.question) {
        return fulfillJson(route, { error: { message: "Fixture expected a member name and an AI question." } }, 400);
      }
      const question = {
        id: `group-message-${groupStudyMessages.length + 1}`,
        author: body.member_name,
        role: "student",
        content: body.question,
        created_at: "2026-07-20T12:03:00Z"
      };
      const response = {
        id: `group-message-${groupStudyMessages.length + 2}`,
        author: "EduMind AI",
        role: "ai",
        content: "The group agrees on the squeeze theorem goal. Verify one worked example, then spend 25 minutes comparing each step aloud.",
        created_at: "2026-07-20T12:03:10Z"
      };
      groupStudyMessages.push(question, response);
      return fulfillJson(route, { question, response, model: "fixture-study-model" });
    }
    if (groupStudyRoom && path.endsWith(`/group-study/groups/${groupStudyRoom.id}/messages`) && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      const message = {
        id: `group-message-${groupStudyMessages.length + 1}`,
        author: body.member_name,
        role: "student",
        content: body.content,
        created_at: "2026-07-20T12:03:00Z"
      };
      groupStudyMessages.push(message);
      return fulfillJson(route, message);
    }
    if (groupStudyRoom && path.endsWith(`/group-study/groups/${groupStudyRoom.id}/resources`) && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      const resource = {
        id: `group-resource-${groupStudyResources.length + 1}`,
        author: body.member_name,
        title: body.title,
        url: body.url,
        description: body.description || null,
        created_at: "2026-07-20T12:04:00Z"
      };
      groupStudyResources.push(resource);
      return fulfillJson(route, resource);
    }
    if (path.endsWith("/agent/run") && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      if (body.moduleId === "student-os" && body.image) {
        return fulfillJson(route, {
          content: JSON.stringify({
            title: "Imported class timetable",
            summary: "One legible fixed class was found in the selected timetable image.",
            assumptions: [],
            tradeoffs: [],
            image_observations: [
              { day: "Monday", start: "11:30", end: "12:15", title: "Linear algebra tutorial", confidence: 0.91, detail: "Visible on the attached timetable." }
            ],
            blocks: []
          }),
          agentId: "master",
          sessionKey: "desktop:student-planner:session",
          model: "fixture-study-model",
          toolsUsed: []
        });
      }
      if (body.moduleId === "exam-practice") {
        return fulfillJson(route, {
          content: JSON.stringify({
            title: "Limits practice set",
            topic: "Limits and continuity",
            questions: [{
              id: "q1",
              objective: "Explain the meaning of a function limit",
              difficulty: "foundational",
              prompt: "What does lim x→a f(x) = L mean?",
              answer: "The values of f(x) approach L as x approaches a.",
              explanation: "A limit describes nearby behavior and does not require f(a) to equal L.",
              evidence: [{ source: "supplied-material", excerpt: "A limit describes the value a function approaches." }],
              official: false
            }]
          }),
          agentId: "master",
          sessionKey: "desktop:exam-practice:session",
          model: "fixture-study-model",
          toolsUsed: []
        });
      }
      if (body.moduleId === "routine") {
        return fulfillJson(route, {
          content: JSON.stringify({
            title: "Calculus recovery routine",
            summary: "Two short review blocks fit after your lecture.",
            assumptions: ["The class time is fixed."],
            tradeoffs: ["Keep the evening open for recovery."],
            image_observations: body.image ? [
              { day: "Monday", start: "11:30", end: "12:15", title: "Linear algebra tutorial", confidence: 0.91, detail: "Visible on the attached timetable." }
            ] : [],
            blocks: [
              { day: "Monday", start: "10:20", end: "11:00", title: "Limits retrieval practice", kind: "study", detail: "Use your approved notes." },
              { day: "Monday", start: "15:00", end: "15:30", title: "Formula review", kind: "review", detail: "Mark one uncertainty to revisit." }
            ]
          }),
          agentId: "master",
          sessionKey: "desktop:routine:session",
          model: "fixture-study-model",
          toolsUsed: ["student_planner_schedule"]
        });
      }
      return fulfillJson(route, {
        content: "Start with a 20-minute limits review, then write down one uncertainty.",
        agentId: "master",
        sessionKey: "desktop:fixture:session",
        model: "fixture-study-model",
        toolsUsed: []
      });
    }
    if (path.endsWith("/agents/status")) {
      return fulfillJson(route, {
        defaultAgent: "master",
        defaultModel: "fixture-study-model",
        toolProfile: "safe",
        agents: [{
          id: "master",
          name: "Master Agent",
          enabled: true,
          model: "fixture-study-model",
          allowedTools: ["module_memory_search", "student_planner_schedule"]
        }]
      });
    }
    if (path.endsWith("/chat/completions") && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      if (String(body.messages?.[0]?.content || "").includes("Group Study facilitator")) {
        return fulfillJson(route, {
          content: "The group agrees on the squeeze theorem goal. Verify one worked example, then spend 25 minutes comparing each step aloud.",
          model: "fixture-study-model"
        });
      }
      return fulfillJson(route, {
        content: "Start with a 20-minute limits review, then write down one uncertainty.",
        model: "fixture-study-model"
      });
    }
    if (path.endsWith("/runtime/status")) {
      return fulfillJson(route, {
        output_root: "OUTPUT",
        latex_enabled: false,
        document_converter_enabled: false,
        slide_converter_enabled: false,
        image_converter_enabled: false,
        notebooklm_enabled: false,
        local_model_configured: true
      });
    }
    if (path.endsWith("/class-notes/export") && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      if (body.destination !== "Semester 1/Calculus" || body.format !== "html") {
        return fulfillJson(route, { error: { message: "Fixture expected an explicit safe Class Notes export destination." } }, 400);
      }
      return fulfillJson(route, {
        document_id: "class-notes-document",
        format: "html",
        output_directory: "OUTPUT\\ClassNotes\\Semester 1\\Calculus",
        source_html_path: "OUTPUT\\ClassNotes\\Semester 1\\Calculus\\sources\\ClassNotes_limits_2026-07-20.html",
        artifact_path: "OUTPUT\\ClassNotes\\Semester 1\\Calculus\\ClassNotes_limits_2026-07-20.html"
      });
    }
    if (path.endsWith("/student/page-state/get") && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      if (body.page === "student-planner") {
        return fulfillJson(route, PLANNER_SNAPSHOT);
      }
      return fulfillJson(route, {
        page: "student-os",
        records: [],
        count: 0,
        updated_at: null
      });
    }
    if (path.endsWith("/student/page-state/save") && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      return fulfillJson(route, {
        snapshot: {
          ...PLANNER_SNAPSHOT,
          page: body.page,
          records: Array.isArray(body.records) ? body.records : []
        },
        indexed_memory_id: "fixture-memory"
      });
    }
    if (path.endsWith("/student/page-state/upsert") && method === "POST") {
      return fulfillJson(route, { applied: true, indexed_memory_id: "fixture-memory" });
    }
    if (path.endsWith("/student/planner/schedule") && method === "GET") {
      return fulfillJson(route, PLANNER_SCHEDULE);
    }
    if (path.endsWith("/study/insights") || path.endsWith("/study/recommendations/refresh")) {
      return fulfillJson(route, INSIGHTS);
    }
    if (path.endsWith("/study/srs/due") && method === "POST") {
      return fulfillJson(route, srsReviewed ? [] : [{
        id: "srs-card-limits",
        front: "What is a limit?",
        back: "The value a function approaches.",
        deck: "class-notes",
        due_at: "2026-07-20T10:00:00Z",
        interval_days: 0,
        ease_factor: 2.5,
        repetitions: 0,
        lapses: 0
      }]);
    }
    if (path.endsWith("/study/srs/preview") && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      const intervals = { 0: 1, 3: 1, 5: 1 };
      return fulfillJson(route, {
        card_id: body.card_id,
        rating: body.rating,
        due_at: "2026-07-21T10:00:00Z",
        interval_days: intervals[body.rating] || 1,
        ease_factor: body.rating === 0 ? 1.7 : body.rating === 5 ? 2.6 : 2.36,
        repetitions: body.rating < 3 ? 0 : 1,
        lapses: body.rating < 3 ? 1 : 0
      });
    }
    if (path.endsWith("/study/srs/review") && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      srsReviewed = true;
      return fulfillJson(route, { id: body.card_id, due_at: "2026-07-21T10:00:00Z", interval_days: 1 });
    }
    if (path.endsWith("/research/projects") && method === "GET") {
      return fulfillJson(route, [researchProject]);
    }
    if (path.endsWith(`/research/projects/${RESEARCH_PROJECT_ID}/documents`) && method === "GET") {
      return fulfillJson(route, indexedResearchDocuments);
    }
    if (path.endsWith(`/research/projects/${RESEARCH_PROJECT_ID}/ingest`) && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      if (body.paper_id !== "paper-retrieval" || body.source) {
        return fulfillJson(route, { error: { message: "Fixture expected a selected open-access paper download." } }, 400);
      }
      const document = {
        project_id: RESEARCH_PROJECT_ID,
        paper_id: "paper-retrieval",
        title: "Retrieval Practice Improves Long-Term Retention",
        source: "https://example.test/retrieval-practice.pdf",
        ocr: false,
        char_count: 4200,
        chunk_count: 4,
        extracted_at: "2026-07-19T10:00:00Z"
      };
      indexedResearchDocuments = [document];
      return fulfillJson(route, document);
    }
    if (path.endsWith(`/research/projects/${RESEARCH_PROJECT_ID}/deep-ask`) && method === "POST") {
      return fulfillJson(route, {
        project_id: RESEARCH_PROJECT_ID,
        question: "What evidence supports retrieval practice?",
        answer: "Full-text evidence supports delayed-retention benefits when students repeatedly retrieve learned material.",
        passages: [{
          paper_id: "paper-retrieval",
          title: "Retrieval Practice Improves Long-Term Retention",
          chunk_index: 1,
          score: 0.91,
          text: "Students who practised retrieval retained more material on the delayed assessment."
        }],
        warnings: [],
        grounded_answer: {
          citations: [{ source_id: "paper-retrieval#1", title: "Retrieval Practice Improves Long-Term Retention", locator: "chunk 1" }]
        }
      });
    }
    if (path.endsWith(`/research/projects/${RESEARCH_PROJECT_ID}/supervise`) && method === "POST") {
      return fulfillJson(route, {
        topic: RESEARCH_PROJECT.topic,
        corpus_health: { total_papers: 1, ingested_documents: indexedResearchDocuments.length, newest_paper_year: 2025, advisories: [] },
        reading_plan: [{ priority: 1, paper_id: "paper-retrieval", title: "Retrieval Practice Improves Long-Term Retention", citation_count: 42, ingested: true, rationale: "Read the indexed full text before generalizing the finding." }],
        stated_gaps: [],
        open_questions: ["Does the benefit transfer across different subjects?"],
        next_steps: ["Compare the indexed trial with a second open-access study before drawing a broad conclusion."]
      });
    }
    if (path.endsWith(`/research/projects/${RESEARCH_PROJECT_ID}/gaps`) && method === "POST") {
      return fulfillJson(route, {
        project_id: RESEARCH_PROJECT_ID,
        topic: RESEARCH_PROJECT.topic,
        corpus_health: { total_papers: 1, ingested_documents: indexedResearchDocuments.length },
        stated_gaps: []
      });
    }
    if (path.endsWith("/research/validate-claims") && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      return fulfillJson(route, {
        claims: (body.claims || []).map((claim, index) => ({
          claim,
          support: index === 0 ? "supported" : "unsupported",
          support_score: index === 0 ? 0.92 : 0.08,
          evidence_ids: index === 0 ? ["paper-retrieval"] : []
        })),
        hallucinations: body.claims?.length > 1 ? [{ claim_index: 1, code: "unsupported_claim", message: "No supplied project evidence supports this universal claim." }] : [],
        citation_errors: [],
        logical_issues: [],
        bias_flags: [],
        overall_score: body.claims?.length > 1 ? 0.5 : 0.92
      });
    }
    if (path.endsWith(`/research/projects/${RESEARCH_PROJECT_ID}/scope`) && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      researchProject = { ...researchProject, scope: body.scope, updated_at: "2026-07-20T12:10:00Z" };
      return fulfillJson(route, researchProject);
    }
    if (path.endsWith(`/research/projects/${RESEARCH_PROJECT_ID}/note`) && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      researchProject = {
        ...researchProject,
        notes: [...(researchProject.notes || []), { id: `note-${(researchProject.notes || []).length + 1}`, content: body.content, created_at: "2026-07-20T12:11:00Z", updated_at: "2026-07-20T12:11:00Z" }]
      };
      return fulfillJson(route, researchProject);
    }
    if (path.endsWith(`/research/projects/${RESEARCH_PROJECT_ID}/question`) && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      researchProject = { ...researchProject, questions: [...new Set([...(researchProject.questions || []), body.question])] };
      return fulfillJson(route, researchProject);
    }
    if (path.endsWith(`/research/projects/${RESEARCH_PROJECT_ID}/export`) && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      return fulfillJson(route, {
        format: body.format,
        content: body.format === "ris" ? "TY  - JOUR\nTI  - Retrieval Practice Improves Long-Term Retention\nER  -" : "@article{researcher2025retrieval,\n  title = {Retrieval Practice Improves Long-Term Retention}\n}"
      });
    }
    if (path.endsWith(`/research/projects/${RESEARCH_PROJECT_ID}`) && method === "GET") {
      return fulfillJson(route, researchProject);
    }
    if (path.endsWith("/memory/summary") && method === "POST") {
      return fulfillJson(route, {
        module_id: "class-notes",
        record_count: 0,
        scope_counts: {},
        content_type_counts: {},
        recent_memories: []
      });
    }
    if (path.includes("/modules/class-notes/memory/search") && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      if (body.content_type !== "transcript") {
        return fulfillJson(route, { error: { message: "Fixture expected deterministic transcript-only retrieval." } }, 400);
      }
      return fulfillJson(route, [{
        record: {
          id: "transcript-calculus-1",
          module_id: "class-notes",
          content: "The lecturer explained that a limit describes the value a function approaches near an input.",
          content_type: "transcript",
          metadata: { topic: "limits" }
        },
        scope: "module",
        score: 0.91
      }]);
    }
    if (path.endsWith("/memory/search") && method === "POST") {
      return fulfillJson(route, []);
    }
    if (path.endsWith("/memory/store") && method === "POST") {
      return fulfillJson(route, { memory_id: "fixture-memory" });
    }
    if (path.endsWith("/study/srs/card") && method === "POST") {
      const body = JSON.parse(request.postData() || "{}");
      return fulfillJson(route, { id: `practice-card-${body.front.length}`, ...body, due_at: "2026-07-20T10:00:00Z", interval_days: 0, ease_factor: 2.5, repetitions: 0, lapses: 0 });
    }
    if (path.endsWith("/study/srs/generate") && method === "POST") {
      return fulfillJson(route, []);
    }
    if (path.endsWith(`/runs/${RUN_ID}/timeline`)) {
      const events = [
        {
          id: "created",
          event_type: "run_initialized",
          message: "Initialized run budget and recovery state.",
          at: "2026-07-19T10:00:00Z"
        }
      ];
      if (cancelled) {
        events.push({
          id: "cancelled",
          event_type: "run_cancelled",
          message: "Cancellation requested by the user.",
          at: "2026-07-19T10:01:00Z"
        });
      }
      return fulfillJson(route, events);
    }
    if (path.endsWith(`/runs/${RUN_ID}/evidence`)) {
      return fulfillJson(route, {
        run_id: RUN_ID,
        budget: {
          max_tool_calls: 2,
          max_output_bytes: 500,
          max_elapsed_secs: 30,
          tool_calls_used: 1,
          output_bytes_used: 120,
          elapsed_secs_used: 4
        },
        checkpoints: [{ id: "checkpoint-1" }],
        verifications: [{
          id: "verification-1",
          passed: true,
          stage: "execute",
          summary: "Fixture evidence passed."
        }]
      });
    }
    if (path.endsWith(`/runs/${RUN_ID}/cancel`) && method === "POST") {
      cancelled = true;
      return fulfillJson(route, { run_id: RUN_ID, cancelled: true });
    }

    return fulfillJson(route, { error: { message: `Unhandled fixture path: ${path}` } }, 404);
  });
}

async function fulfillJson(route, payload, status = 200) {
  await route.fulfill({
    status,
    contentType: "application/json",
    headers: { "access-control-allow-origin": "*" },
    body: JSON.stringify(payload)
  });
}

test("opens the command palette and routes to local study insights", async ({ page }) => {
  await continueOffline(page);
  await page.keyboard.press("Control+K");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await expect(palette).toBeVisible();
  await palette.getByRole("button", { name: /Open Review/i }).click();

  await expect(page.getByRole("heading", { name: "Next best study actions" })).toBeVisible();
  await expect(page.getByText("Launch the desktop app to load local study insights.")).toBeVisible();
});

test("sends an authenticated study chat through the local gateway", async ({ page }) => {
  await openMockWorkspace(page);
  await page.getByRole("button", { name: "AI Tutor", exact: true }).click();
  await page.getByLabel("Message").fill("How should I prepare for my limits quiz?");
  await page.getByRole("button", { name: "Send message" }).click();

  await expect(page.getByText("Start with a 20-minute limits review, then write down one uncertainty.")).toBeVisible();
  await expect(page.getByText("Answered by fixture-study-model")).toBeVisible();
});

test("shows a local-first daily study dashboard", async ({ page }) => {
  await openMockWorkspace(page);

  await expect(page.getByRole("heading", { name: "Your study dashboard, built for today." })).toBeVisible();
  await expect(page.getByRole("heading", { name: /Today.?s focus/ })).toBeVisible();
  await expect(page.getByRole("heading", { name: "calculus: limits" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Study timer" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Start timer" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Final weekly schedule" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Open Calculus lecture in Planner" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "What needs your attention" })).toBeVisible();
  await expect(page.getByText("1 research project")).toBeVisible();
  await expect(page.getByText("1 enabled agent under the safe tool profile.")).toBeVisible();

  await page.getByRole("button", { name: "Open Research" }).click();
  await expect(page.getByRole("heading", { name: "Discover papers, inspect their evidence, and get a defensible next step." })).toBeVisible();
});

test("keeps workout and meal tracking in student-owned wellness records", async ({ page }) => {
  await openMockWorkspace(page);
  await page.getByRole("button", { name: "Wellbeing", exact: true }).click();

  await expect(page.getByRole("heading", { name: "Keep workouts, meals, and study energy in one calm daily tracker." })).toBeVisible();
  await expect(page.getByLabel("Wellness libraries").getByText("Bodyweight squats")).toBeVisible();

  await page.getByRole("button", { name: "Use balanced pick" }).click();
  await page.getByLabel("Mark Bodyweight squats complete today").check();
  await expect(page.getByText("1 of 3 selected items complete")).toBeVisible();

  await page.getByRole("button", { name: "Build balanced meals" }).click();
  await page.getByLabel("Mark Eggs eaten for Breakfast").check();
  await expect(page.locator(".wellness-metric-card").filter({ hasText: "Protein logged" }).getByText("6 g", { exact: true })).toBeVisible();

  await page.getByRole("button", { name: "Add food", exact: true }).click();
  await page.getByLabel("Food").fill("Greek yogurt");
  await page.getByLabel("Protein per serving (g)").fill("10");
  await page.getByRole("button", { name: "Save food" }).click();
  await expect(page.getByLabel("Wellness libraries").getByText("Greek yogurt")).toBeVisible();
});

test("runs a confirmation-gated task automation with a compact local audit", async ({ page }) => {
  await openMockWorkspace(page);
  await page.getByRole("button", { name: "Smart Tasks", exact: true }).click();

  await expect(page.getByRole("heading", { name: "Keep every study workspace moving, while you stay in control." })).toBeVisible();
  await page.getByLabel("Automation name").fill("Daily study reset");
  await page.getByRole("button", { name: "Create automation" }).click();

  await expect(page.getByRole("heading", { name: "Review queue" })).toBeVisible();
  await expect(page.getByLabel("Review queue").getByRole("heading", { name: "Daily study reset" })).toBeVisible();
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Run safe step" }).click();

  await expect(page.getByLabel("Review queue").getByText("1 study priorities and 90 available minutes reviewed.", { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Recent runs" })).toBeVisible();
  await expect(page.getByLabel("Review queue").locator(".automation-state-pill.status-completed")).toBeVisible();
});

test("drafts AI routine blocks and requires confirmation before applying", async ({ page }) => {
  await openMockWorkspace(page);
  await page.getByRole("button", { name: "Routine Coach", exact: true }).click();

  await expect(page.getByRole("heading", { name: "Let AI draft a routine you stay in control of." })).toBeVisible();
  await page.getByLabel("Day").selectOption("Monday");
  await page.getByLabel("Focus goal").fill("Prepare for my calculus quiz with focused review.");
  await page.getByRole("button", { name: "Generate AI routine" }).click();

  await expect(page.getByText("Limits retrieval practice")).toBeVisible();
  await expect(page.getByText("Read-only context used: student_planner_schedule.")).toBeVisible();
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Apply 2 safe blocks" }).click();

  await expect(page.getByText("Added 2 AI-managed blocks to the canonical planner.")).toBeVisible();
});

test("analyzes and saves a timetable image only from Student Planner", async ({ page }) => {
  await openMockWorkspace(page);
  await page.getByRole("button", { name: "Planner", exact: true }).click();

  await page.getByLabel("Timetable image", { exact: true }).setInputFiles(TIMETABLE_IMAGE);
  await expect(page.getByText("monday-timetable.png")).toBeVisible();
  await page.getByRole("button", { name: "Analyze timetable image" }).click();

  await expect(page.getByText("Linear algebra tutorial")).toBeVisible();
  await expect(page.getByText("Planner draft", { exact: true })).toBeVisible();
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Add 1 safe timetable block" }).click();
  await expect(page.getByText("Added 1 detected timetable block to the canonical planner.")).toBeVisible();

  await page.getByRole("button", { name: "Routine Coach", exact: true }).click();
  await expect(page.getByLabel("Timetable image", { exact: true })).toHaveCount(0);
  await expect(page.getByText("Planner stays canonical")).toBeVisible();
});

test("keeps agent sandbox management inside the installed desktop app", async ({ page }) => {
  await openMockWorkspace(page);
  await page.getByRole("button", { name: "Settings", exact: true }).click();

  await expect(page.getByRole("heading", { name: "Local Agent Sandbox" })).toBeVisible();
  await expect(page.getByText("Agent management and sandbox documents open only in the installed EduMind desktop app.")).toBeVisible();
});

test("continues a Group Study room with invite, chat, shared resource, and contextual AI", async ({ page }) => {
  await openMockWorkspace(page);
  await page.getByRole("button", { name: "Study Groups", exact: true }).click();

  await expect(page.getByRole("heading", { name: "Keep your study crew focused, connected, and evidence-aware." })).toBeVisible();
  await page.getByLabel("Your name").fill("Amina");
  await page.getByLabel("Room name").fill("Calculus review crew");
  await page.getByLabel("Study focus").fill("Limits before Thursday's quiz");
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Create study room" }).click();

  await expect(page.getByRole("heading", { name: "Calculus review crew" })).toBeVisible();
  await expect(page.getByText("STUDY-FIXTURECALCULUS")).toBeVisible();
  await page.getByLabel("Your name").fill("Rafi");
  await page.getByLabel("Invite code").fill("STUDY-FIXTURECALCULUS");
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Join study room" }).click();
  await expect(page.getByLabel("Group members")).toContainText("Rafi");

  await page.getByLabel("Message to Calculus review crew").fill("Can we compare the squeeze theorem examples?");
  await page.getByRole("button", { name: "Share message" }).click();
  await expect(page.getByText("Can we compare the squeeze theorem examples?")).toBeVisible();

  await page.getByLabel("Resource title").fill("Open calculus notes");
  await page.getByLabel("HTTPS resource link").fill("https://example.edu/calculus/limits");
  await page.getByLabel("Why it matters optional").fill("Read section 2 before the group call.");
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Share resource" }).click();
  await expect(page.getByRole("link", { name: "Open calculus notes" })).toBeVisible();

  await page.getByLabel("Message to Calculus review crew").fill("Help us choose one focused next step.");
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Ask AI in chat" }).click();
  await expect(page.getByText("The group agrees on the squeeze theorem goal. Verify one worked example, then spend 25 minutes comparing each step aloud.")).toBeVisible();
});

test("keeps a student project note in the canonical Student OS page", async ({ page }) => {
  await openMockWorkspace(page);
  await page.getByRole("button", { name: "Student Hub", exact: true }).click();
  await page.getByRole("tab", { name: /Projects & notes/i }).click();

  await expect(page.getByRole("heading", { name: "Projects & Notes" })).toBeVisible();
  await page.getByLabel("Project name").fill("Physics lab report");
  await page.getByLabel("Project status").selectOption("active");
  await page.getByLabel("Technology or course").fill("Physics 101");
  await page.getByLabel("Goal or idea").fill("Estimate gravitational acceleration from measured timing data.");
  await page.getByLabel("Project notes").fill("The first trial drifted. Recheck the release point before averaging results.");
  await page.getByLabel("What I learnt").fill("Control variables before comparing measurements.");
  await page.getByLabel("Resource link").fill("https://example.test/physics-lab");
  await page.getByRole("button", { name: "Save project note" }).click();

  await expect(page.getByRole("heading", { name: "Physics lab report" })).toBeVisible();
  await expect(page.getByText("Saved the project note in your canonical Student OS page.")).toBeVisible();
  await page.getByRole("button", { name: "Edit Physics lab report" }).click();
  await page.getByLabel("Project notes").fill("The release point is now controlled; average the clean trials only.");
  await page.getByRole("button", { name: "Update project note" }).click();

  await expect(page.locator(".project-note-card .project-note-copy").filter({ hasText: "The release point is now controlled; average the clean trials only." })).toBeVisible();
});

test("opens real class-notes and exam-practice workspaces", async ({ page }) => {
  await openMockWorkspace(page);
  await page.getByRole("button", { name: "Notes", exact: true }).click();

  await expect(page.getByRole("heading", { name: "Turn course material into evidence-led notes." })).toBeVisible();
  await expect(page.getByText("The Supabase inbox opens in the installed EduMind desktop app.")).toBeVisible();
  await page.getByLabel("Course material").fill("A limit describes the value a function approaches.");
  await page.getByRole("button", { name: "Create structured notes" }).click();
  await expect(page.getByRole("heading", { name: "Your next study artifact" })).toBeVisible();
  await expect(page.getByText("Auto-prefetched transcript evidence: transcript-calculus-1.")).toBeVisible();
  await page.getByLabel("Destination folder name").fill("Semester 1/Calculus");
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Save notes as HTML" }).click();
  await expect(page.getByText(/HTML saved: OUTPUT\\ClassNotes\\Semester 1\\Calculus/i)).toBeVisible();
  await expect(page.getByRole("button", { name: "Export notes as PDF" })).toBeDisabled();

  await page.getByRole("button", { name: "Exam Practice", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Build practice from approved course material." })).toBeVisible();
  await page.getByLabel("Topic").fill("Limits and continuity");
  await page.getByLabel("Approved study material").fill("A limit describes the value a function approaches.");
  await page.getByRole("button", { name: "Create practice set" }).click();
  await expect(page.getByRole("heading", { name: "Limits practice set" })).toBeVisible();
  await expect(page.getByText("What does lim x→a f(x) = L mean?")).toBeVisible();
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Save approved practice set" }).click();
  await expect(page.getByText("Saved the validated practice set to local Exam Practice memory.", { exact: false })).toBeVisible();
});

test("keeps the graph accessible when WebGL is explicitly unavailable", async ({ page }) => {
  await page.addInitScript(() => {
    window.__EDUMIND_DISABLE_WEBGL = true;
  });
  await continueOffline(page);
  await page.getByRole("button", { name: "Learning Map", exact: true }).click();

  await expect(page.getByText(/WebGL is unavailable/i)).toBeVisible();
  await expect(page.getByRole("heading", { name: "Accessible node list" })).toBeVisible();
});

test("persists an explicit first-run onboarding choice through the gateway", async ({ page }) => {
  await openMockWorkspace(page);

  await expect(page.getByRole("heading", { name: "Set up your local learning loop." })).toBeVisible();
  await page.getByLabel("Keep study preferences local").check();
  await page.getByRole("button", { name: "Preview deterministic offline demo" }).click();
  await expect(page.getByRole("heading", { name: "Deterministic offline demo dataset" })).toBeVisible();
  await page.getByRole("button", { name: "Save local setup and continue" }).click();

  await expect(page.getByRole("heading", { name: "Next best study actions" })).toBeVisible();
});

test("accepts a recommendation without silently changing the planner", async ({ page }) => {
  await openMockWorkspace(page);
  await page.getByRole("button", { name: "Review", exact: true }).click();

  await expect(page.getByRole("heading", { name: "calculus: limits" })).toBeVisible();
  await expect(page.getByRole("button", { name: /Again: Next review in 1 day/i })).toBeVisible();
  await expect(page.getByRole("button", { name: /Good: Next review in 1 day/i })).toBeVisible();
  await expect(page.getByRole("button", { name: /Easy: Next review in 1 day/i })).toBeVisible();
  await page.getByRole("button", { name: /Good: Next review in 1 day/i }).click();
  await expect(page.getByText("No cards are due right now.")).toBeVisible();
  await page.getByRole("button", { name: "Accept focus block" }).click();
  await expect(page.getByText("Accepted as the next local focus. No planner block was changed.")).toBeVisible();
});

test("downloads a chosen open-access paper and supervises its full-text analysis", async ({ page }) => {
  await openMockWorkspace(page);
  await page.getByRole("button", { name: "Research", exact: true }).click();

  await page.getByRole("button", { name: /Retrieval practice and retention/i }).click();
  await expect(page.getByRole("heading", { name: "Choose a paper to download and analyze" })).toBeVisible();
  await page.getByLabel("Canonical scope").fill("College students; delayed retention outcomes; exclude non-learning interventions.");
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Save project scope" }).click();
  await expect(page.getByText("Saved the project scope.", { exact: false })).toBeVisible();
  await page.getByLabel("New project note").fill("Compare delayed tests before generalizing across subjects.");
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Save project note" }).click();
  await expect(page.getByText("Compare delayed tests before generalizing across subjects.")).toBeVisible();
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Download & index PDF" }).click();

  await expect(page.getByText("1 ready for deep analysis")).toBeVisible();
  await page.getByLabel("Research question").fill("What evidence supports retrieval practice?");
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Save question" }).click();
  await expect(page.getByText("What evidence supports retrieval practice?")).toBeVisible();
  await page.getByRole("button", { name: "Analyze indexed papers" }).click();
  await expect(page.getByRole("heading", { name: "Evidence from indexed papers" })).toBeVisible();
  await expect(page.getByText("Students who practised retrieval retained more material on the delayed assessment.")).toBeVisible();

  await page.getByLabel("Draft claims").fill("Retrieval practice improves delayed retention.\nThe effect is identical in every subject.");
  await page.getByRole("button", { name: "Validate against project evidence" }).click();
  await expect(page.getByText("supported", { exact: true })).toBeVisible();
  await expect(page.getByText("unsupported", { exact: true })).toBeVisible();
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Generate bibliography" }).click();
  await expect(page.getByText("@article{researcher2025retrieval", { exact: false })).toBeVisible();

  await page.getByRole("button", { name: "Supervise project" }).click();
  await expect(page.getByRole("heading", { name: "What to read and do next" })).toBeVisible();
  await expect(page.getByText("Compare the indexed trial with a second open-access study before drawing a broad conclusion.")).toBeVisible();
});

test("inspects persisted evidence and confirms cancellation recovery", async ({ page }) => {
  await openMockWorkspace(page);
  await page.getByRole("button", { name: "Research", exact: true }).click();
  await page.getByLabel("Premium run ID").fill(RUN_ID);
  await page.getByRole("button", { name: "Inspect run timeline" }).click();

  await expect(page.getByRole("heading", { name: "Recoverable run timeline" })).toBeVisible();
  await expect(page.getByText("1 / 2")).toBeVisible();
  await page.getByRole("button", { name: "Cancel run" }).click();
  await page.getByRole("button", { name: "Confirm cancellation" }).click();

  await expect(page.getByText("Cancellation requested by the user.")).toBeVisible();
  await expect(page.getByText("Cancellation is persisted and prevents later stages from starting.")).toBeVisible();
});
