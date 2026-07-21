import { afterEach, describe, expect, it, vi } from "vitest";

import { GatewayClient } from "./gateway";

describe("GatewayClient.saveStudentPage", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("omits tombstones so the canonical save endpoint records deletions", async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ snapshot: { records: [] } }), {
      status: 200,
      headers: { "content-type": "application/json" }
    }));
    vi.stubGlobal("fetch", fetch);
    const client = new GatewayClient({ baseUrl: "http://127.0.0.1:18789", token: "test-token" });

    await client.saveStudentPage("student-planner", [
      { key: "active", value: { title: "Keep" }, updated_at: "2026-07-19T10:00:00Z", deleted: false },
      { key: "removed", value: { title: "Remove" }, updated_at: "2026-07-19T10:00:00Z", deleted: true }
    ]);

    const request = fetch.mock.calls[0][1];
    const body = JSON.parse(request.body);
    expect(body.records).toEqual([
      { key: "active", value: { title: "Keep" }, updated_at: "2026-07-19T10:00:00Z" }
    ]);
  });
});

describe("GatewayClient.agentRun", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("forwards a transient timetable image only with the agent request", async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ content: "{}" }), {
      status: 200,
      headers: { "content-type": "application/json" }
    }));
    vi.stubGlobal("fetch", fetch);
    const client = new GatewayClient({ baseUrl: "http://127.0.0.1:18789", token: "test-token" });

    await client.agentRun({
      message: "Analyze the selected timetable image.",
      sessionKey: "desktop:routine:test",
      moduleId: "routine",
      untrustedContent: true,
      image: { mimeType: "image/png", dataBase64: "iVBORw0KGgo=" }
    });

    const body = JSON.parse(fetch.mock.calls[0][1].body);
    expect(body.image).toEqual({ mimeType: "image/png", dataBase64: "iVBORw0KGgo=" });
    expect(body.moduleId).toBe("routine");
  });
});

describe("GatewayClient.health", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("rejects a frontend page that is mistakenly configured as a gateway", async () => {
    const fetch = vi.fn().mockResolvedValue(new Response("<!doctype html><title>EduMind</title>", {
      status: 200,
      headers: { "content-type": "text/html" }
    }));
    vi.stubGlobal("fetch", fetch);
    const client = new GatewayClient({ baseUrl: "http://127.0.0.1:1420", token: "test-token" });

    await expect(client.health()).rejects.toThrow("returned a web page instead of an EduMind gateway");
  });
});

describe("GatewayClient research helpers", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("uses the bounded full-text ingest endpoint with a selected paper ID", async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ paper_id: "paper-1" }), {
      status: 200,
      headers: { "content-type": "application/json" }
    }));
    vi.stubGlobal("fetch", fetch);
    const client = new GatewayClient({ baseUrl: "http://127.0.0.1:18789", token: "test-token" });

    await client.ingestResearchPdf("project/one", {
      paperId: "paper-1",
      source: "https://example.test/open-paper.pdf",
      ocr: "auto"
    });

    const [url, request] = fetch.mock.calls[0];
    expect(url).toContain("/research/projects/project%2Fone/ingest");
    expect(JSON.parse(request.body)).toEqual({
      paper_id: "paper-1",
      source: "https://example.test/open-paper.pdf",
      ocr: "auto"
    });
  });
});

describe("GatewayClient Class Notes exports", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("sends the selected output folder and an explicit export format", async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ artifact_path: "OUTPUT/ClassNotes/Semester 1/Calculus/notes.html" }), {
      status: 200,
      headers: { "content-type": "application/json" }
    }));
    vi.stubGlobal("fetch", fetch);
    const client = new GatewayClient({ baseUrl: "http://127.0.0.1:18789", token: "test-token" });

    await client.exportClassNotes({
      title: "Limits review",
      content: "A limit describes a value a function approaches.",
      destination: "Semester 1/Calculus",
      format: "html"
    });

    const [url, request] = fetch.mock.calls[0];
    expect(url).toContain("/class-notes/export");
    expect(JSON.parse(request.body)).toEqual({
      title: "Limits review",
      content: "A limit describes a value a function approaches.",
      destination: "Semester 1/Calculus",
      format: "html"
    });
  });
});

describe("GatewayClient Group Study helpers", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("creates a room, shares a student message, and asks AI from stored room context", async () => {
    const fetch = vi.fn().mockImplementation(() => Promise.resolve(new Response(JSON.stringify({ group: { id: "group-1" } }), {
      status: 200,
      headers: { "content-type": "application/json" }
    })));
    vi.stubGlobal("fetch", fetch);
    const client = new GatewayClient({ baseUrl: "http://127.0.0.1:18789", token: "test-token" });

    await client.createGroupStudy({
      title: "Calculus review crew",
      topic: "Limits before Thursday's quiz",
      memberName: "Amina"
    });
    await client.sendGroupStudyMessage("group/1", {
      memberName: "Amina",
      content: "Compare the squeeze theorem examples."
    });
    await client.askGroupStudyAi("group/1", {
      memberName: "Amina",
      question: "What should we test first?"
    });

    const [createUrl, createRequest] = fetch.mock.calls[0];
    const [messageUrl, messageRequest] = fetch.mock.calls[1];
    const [aiUrl, aiRequest] = fetch.mock.calls[2];
    expect(createUrl).toContain("/group-study/groups");
    expect(JSON.parse(createRequest.body)).toEqual({
      title: "Calculus review crew",
      topic: "Limits before Thursday's quiz",
      member_name: "Amina"
    });
    expect(messageUrl).toContain("/group-study/groups/group%2F1/messages");
    expect(JSON.parse(messageRequest.body)).toEqual({
      member_name: "Amina",
      content: "Compare the squeeze theorem examples."
    });
    expect(aiUrl).toContain("/group-study/groups/group%2F1/ai");
    expect(JSON.parse(aiRequest.body)).toEqual({
      member_name: "Amina",
      question: "What should we test first?"
    });
  });
});
