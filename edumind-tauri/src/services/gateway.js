export class GatewayRequestError extends Error {
  constructor(message, status, payload) {
    super(message);
    this.name = "GatewayRequestError";
    this.status = status;
    this.payload = payload;
  }
}

export class GatewayClient {
  constructor(endpoint) {
    this.baseUrl = endpoint.baseUrl.replace(/\/$/, "");
    this.token = endpoint.token;
  }

  get available() {
    return Boolean(this.baseUrl);
  }

  async request(path, { method = "GET", body } = {}) {
    if (!this.available) {
      throw new GatewayRequestError(
        "The embedded gateway is unavailable in browser preview mode.",
        0,
        null
      );
    }
    const headers = {
      Accept: "application/json"
    };
    if (this.token) {
      headers.Authorization = `Bearer ${this.token}`;
    }
    if (body !== undefined) {
      headers["Content-Type"] = "application/json";
    }
    const response = await fetch(`${this.baseUrl}${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body)
    });
    const text = await response.text();
    const payload = text ? parsePayload(text) : null;
    if (!response.ok) {
      throw new GatewayRequestError(
        errorMessage(payload) ?? `Gateway request failed with status ${response.status}.`,
        response.status,
        payload
      );
    }
    return payload;
  }

  get(path) {
    return this.request(path);
  }

  post(path, body = {}) {
    return this.request(path, { method: "POST", body });
  }

  health() {
    return this.get("/health").then((payload) => {
      if (payload && typeof payload === "object" && !Array.isArray(payload) && payload.status === "ok") {
        return payload;
      }
      throw new GatewayRequestError(
        "The configured endpoint returned a web page instead of an EduMind gateway. Start EduMind with `npm run tauri dev`, or point VITE_GATEWAY_URL at a compatible EduMind gateway rather than the Vite frontend URL.",
        0,
        payload
      );
    });
  }

  chat(messages) {
    return this.post("/api/v1/chat/completions", { messages });
  }

  agentRun({
    message,
    sessionKey,
    agentId,
    requestedModel,
    moduleId,
    untrustedContent = false,
    image
  }) {
    return this.post("/api/v1/agent/run", {
      message,
      sessionKey,
      agentId,
      requestedModel,
      moduleId,
      untrustedContent,
      image
    });
  }

  agentStatus() {
    return this.get("/api/v1/agents/status");
  }

  runtimeStatus() {
    return this.get("/api/v1/runtime/status");
  }

  memoryGraph() {
    return this.get("/api/v1/memory/graph");
  }

  moduleMemorySummary(moduleId) {
    return this.post("/api/v1/modules/" + encodeURIComponent(moduleId) + "/memory/summary");
  }

  moduleMemorySearch(moduleId, query, { limit = 8, scope = "module", contentType } = {}) {
    return this.post("/api/v1/modules/" + encodeURIComponent(moduleId) + "/memory/search", {
      query,
      limit,
      scope,
      content_type: contentType
    });
  }

  moduleMemoryStore(moduleId, {
    content,
    contentType = "note",
    scope = "module",
    metadata = {}
  }) {
    return this.post("/api/v1/modules/" + encodeURIComponent(moduleId) + "/memory/store", {
      content,
      content_type: contentType,
      scope,
      metadata
    });
  }

  listProjects() {
    return this.get("/api/v1/research/projects?limit=50");
  }

  runFocusedResearch({ topic, query = topic, maxResults = 12 }) {
    return this.post("/api/v1/research/focused/run", {
      topic,
      query,
      max_results: maxResults
    });
  }

  researchProject(projectId) {
    return this.get(`/api/v1/research/projects/${encodeURIComponent(projectId)}`);
  }

  researchProjectAnswer(projectId, question, { limit = 5 } = {}) {
    return this.post(`/api/v1/research/projects/${encodeURIComponent(projectId)}/ask`, { question, limit });
  }

  deepResearchAnswer(projectId, question, { limit = 5 } = {}) {
    return this.post(`/api/v1/research/projects/${encodeURIComponent(projectId)}/deep-ask`, { question, limit });
  }

  ingestResearchPdf(projectId, { paperId, source, title, ocr = "auto" } = {}) {
    return this.post(`/api/v1/research/projects/${encodeURIComponent(projectId)}/ingest`, {
      paper_id: paperId,
      source,
      title,
      ocr
    });
  }

  researchDocuments(projectId) {
    return this.get(`/api/v1/research/projects/${encodeURIComponent(projectId)}/documents`);
  }

  researchGaps(projectId) {
    return this.post(`/api/v1/research/projects/${encodeURIComponent(projectId)}/gaps`);
  }

  researchSupervision(projectId) {
    return this.post(`/api/v1/research/projects/${encodeURIComponent(projectId)}/supervise`);
  }

  researchSynthesis(projectId) {
    return this.post(`/api/v1/research/projects/${encodeURIComponent(projectId)}/synthesis`);
  }

  setResearchScope(projectId, scope) {
    return this.post(`/api/v1/research/projects/${encodeURIComponent(projectId)}/scope`, {
      scope: scope?.trim() || null
    });
  }

  addResearchNote(projectId, content) {
    return this.post(`/api/v1/research/projects/${encodeURIComponent(projectId)}/note`, { content });
  }

  addResearchQuestion(projectId, question) {
    return this.post(`/api/v1/research/projects/${encodeURIComponent(projectId)}/question`, { question });
  }

  validateResearchClaims({ claims, evidence, supportThreshold = 0.18 }) {
    return this.post("/api/v1/research/validate-claims", {
      claims,
      evidence,
      support_threshold: supportThreshold
    });
  }

  exportResearchProject(projectId, format) {
    return this.post(`/api/v1/research/projects/${encodeURIComponent(projectId)}/export`, { format });
  }

  researchLiteratureGraph(papers) {
    return this.post("/api/v1/research/literature-graph", {
      papers: Array.isArray(papers) ? papers : [],
      similarity_threshold: 0.18
    });
  }

  studyInsights() {
    return this.get("/api/v1/study/insights");
  }

  refreshStudyRecommendations() {
    return this.post("/api/v1/study/recommendations/refresh");
  }

  generateSrsCards(notes, deck) {
    return this.post("/api/v1/study/srs/generate", { notes, deck });
  }

  createSrsCard({ front, back, deck }) {
    return this.post("/api/v1/study/srs/card", { front, back, deck });
  }

  dueSrsCards({ deck, limit = 20 } = {}) {
    return this.post("/api/v1/study/srs/due", { deck, limit });
  }

  previewSrsReview(cardId, rating) {
    return this.post("/api/v1/study/srs/preview", { card_id: cardId, rating });
  }

  reviewSrsCard(cardId, rating) {
    return this.post("/api/v1/study/srs/review", { card_id: cardId, rating });
  }

  exportClassNotes({ title, content, destination, format = "html" }) {
    return this.post("/api/v1/class-notes/export", {
      title,
      content,
      destination,
      format
    });
  }

  groupStudyGroups() {
    return this.get("/api/v1/group-study/groups");
  }

  createGroupStudy({ title, topic, memberName }) {
    return this.post("/api/v1/group-study/groups", {
      title,
      topic,
      member_name: memberName
    });
  }

  groupStudyGroup(groupId) {
    return this.get(`/api/v1/group-study/groups/${encodeURIComponent(groupId)}`);
  }

  joinGroupStudy({ inviteCode, memberName }) {
    return this.post("/api/v1/group-study/join", {
      invite_code: inviteCode,
      member_name: memberName
    });
  }

  sendGroupStudyMessage(groupId, { memberName, content }) {
    return this.post(`/api/v1/group-study/groups/${encodeURIComponent(groupId)}/messages`, {
      member_name: memberName,
      content
    });
  }

  askGroupStudyAi(groupId, { memberName, question }) {
    return this.post(`/api/v1/group-study/groups/${encodeURIComponent(groupId)}/ai`, {
      member_name: memberName,
      question
    });
  }

  shareGroupStudyResource(groupId, { memberName, title, url, description }) {
    return this.post(`/api/v1/group-study/groups/${encodeURIComponent(groupId)}/resources`, {
      member_name: memberName,
      title,
      url,
      description
    });
  }

  runTimeline(runId) {
    return this.get(`/api/v1/runs/${encodeURIComponent(runId)}/timeline`);
  }

  runEvidence(runId) {
    return this.get(`/api/v1/runs/${encodeURIComponent(runId)}/evidence`);
  }

  cancelRun(runId) {
    return this.post(`/api/v1/runs/${encodeURIComponent(runId)}/cancel`);
  }

  subscribeEvents({ onEvent, onStatus } = {}) {
    if (!this.available || typeof WebSocket === "undefined") {
      onStatus?.("unavailable");
      return () => {};
    }
    const url = new URL(this.baseUrl);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    const path = url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname;
    url.pathname = `${path}/ws`;
    const socket = new WebSocket(url);
    onStatus?.("connecting");
    socket.addEventListener("open", () => {
      socket.send(JSON.stringify({ protocol_version: 1, token: this.token ?? null }));
      onStatus?.("connected");
    });
    socket.addEventListener("message", (message) => {
      const payload = parsePayload(message.data);
      if (payload && typeof payload === "object" && payload.event) {
        onEvent?.(payload);
      }
    });
    socket.addEventListener("error", () => onStatus?.("error"));
    socket.addEventListener("close", () => onStatus?.("closed"));
    return () => {
      if (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING) {
        socket.close();
      }
    };
  }

  studentPage(page) {
    return this.post("/api/v1/student/page-state/get", { page });
  }

  plannerSchedule() {
    return this.get("/api/v1/student/planner/schedule");
  }

  wellnessPlan(profile, { reconcileWithPlanner = true } = {}) {
    return this.post("/api/v1/wellness/plan", {
      profile,
      reconcile_with_planner: reconcileWithPlanner
    });
  }

  saveStudentPage(page, records) {
    const activeRecords = Array.isArray(records)
      ? records
        .filter((record) => !record?.deleted)
        .map((record) => ({
          key: record.key,
          value: record.value,
          updated_at: record.updated_at
        }))
      : [];
    return this.post("/api/v1/student/page-state/save", {
      page,
      records: activeRecords,
      source: "desktop",
      updated_at: new Date().toISOString()
    });
  }

  upsertStudentPageRecord(page, key, value, { source = "desktop" } = {}) {
    return this.post("/api/v1/student/page-state/upsert", {
      page,
      record: {
        key,
        value,
        updated_at: new Date().toISOString()
      },
      source
    });
  }
}

function parsePayload(text) {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

function errorMessage(payload) {
  if (typeof payload === "string") {
    return payload;
  }
  return payload?.error?.message ?? payload?.message ?? null;
}
