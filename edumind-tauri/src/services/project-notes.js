export const PROJECT_NOTE_RECORD_PREFIX = "project-note.";

export const PROJECT_NOTE_STATUSES = Object.freeze([
  { id: "idea", label: "Idea" },
  { id: "active", label: "Active" },
  { id: "complete", label: "Complete" }
]);

const PROJECT_NOTE_LIMITS = Object.freeze({
  title: 120,
  summary: 600,
  notes: 12000,
  technology: 500,
  learnings: 2500,
  resourceUrl: 1000
});

const PROJECT_NOTE_STATUS_IDS = new Set(PROJECT_NOTE_STATUSES.map((status) => status.id));

export function isProjectNoteRecordKey(key) {
  return cleanText(key, 200).startsWith(PROJECT_NOTE_RECORD_PREFIX);
}

export function createProjectNoteKey() {
  const identifier = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return `${PROJECT_NOTE_RECORD_PREFIX}${identifier}`;
}

export function emptyProjectNoteDraft() {
  return {
    title: "",
    summary: "",
    notes: "",
    technology: "",
    learnings: "",
    resource_url: "",
    status: "active",
    created_at: ""
  };
}

export function validateProjectNoteDraft(draft) {
  const normalized = normalizeProjectNote(draft);
  const fields = [
    ["Project name", draft?.title, PROJECT_NOTE_LIMITS.title],
    ["Goal or idea", draft?.summary, PROJECT_NOTE_LIMITS.summary],
    ["Project notes", draft?.notes, PROJECT_NOTE_LIMITS.notes],
    ["Technology or course", draft?.technology, PROJECT_NOTE_LIMITS.technology],
    ["What I learnt", draft?.learnings, PROJECT_NOTE_LIMITS.learnings],
    ["Resource link", draft?.resource_url, PROJECT_NOTE_LIMITS.resourceUrl]
  ];

  for (const [label, value, limit] of fields) {
    if (String(value ?? "").trim().length > limit) {
      return { valid: false, error: `${label} must be ${limit.toLocaleString()} characters or fewer.` };
    }
  }

  if (!normalized.title) {
    return { valid: false, error: "Add a project name before saving." };
  }

  if (!normalized.notes) {
    return { valid: false, error: "Add a project note before saving." };
  }

  if (normalized.resource_url && !safeHttpUrl(normalized.resource_url)) {
    return { valid: false, error: "Resource links must use http:// or https://." };
  }

  return { valid: true, value: normalized };
}

export function createProjectNoteValue(draft, createdAt = new Date().toISOString()) {
  const normalized = normalizeProjectNote(draft);
  return {
    version: 1,
    kind: "project-note",
    ...normalized,
    created_at: cleanText(draft?.created_at, 80) || createdAt
  };
}

export function projectNotesFromRecords(records) {
  return (Array.isArray(records) ? records : [])
    .filter((record) => !record?.deleted && isProjectNoteRecordKey(record?.key))
    .map((record) => ({
      key: record.key,
      updated_at: cleanText(record.updated_at, 80),
      value: createProjectNoteValue(record.value, cleanText(record.updated_at, 80) || new Date(0).toISOString())
    }))
    .sort((first, second) => dateValue(second.updated_at) - dateValue(first.updated_at));
}

export function projectNoteStats(projectNotes) {
  const notes = Array.isArray(projectNotes) ? projectNotes : [];
  return {
    total: notes.length,
    active: notes.filter((note) => note.value.status === "active").length,
    complete: notes.filter((note) => note.value.status === "complete").length,
    resources: notes.filter((note) => safeHttpUrl(note.value.resource_url)).length
  };
}

export function safeHttpUrl(value) {
  const candidate = cleanText(value, PROJECT_NOTE_LIMITS.resourceUrl);
  if (!candidate) {
    return "";
  }
  try {
    const parsed = new URL(candidate);
    return parsed.protocol === "https:" || parsed.protocol === "http:" ? parsed.toString() : "";
  } catch {
    return "";
  }
}

function normalizeProjectNote(draft) {
  const requestedStatus = cleanText(draft?.status, 32).toLowerCase();
  return {
    title: cleanText(draft?.title, PROJECT_NOTE_LIMITS.title),
    summary: cleanText(draft?.summary, PROJECT_NOTE_LIMITS.summary),
    notes: cleanText(draft?.notes, PROJECT_NOTE_LIMITS.notes),
    technology: cleanText(draft?.technology, PROJECT_NOTE_LIMITS.technology),
    learnings: cleanText(draft?.learnings, PROJECT_NOTE_LIMITS.learnings),
    resource_url: cleanText(draft?.resource_url, PROJECT_NOTE_LIMITS.resourceUrl),
    status: PROJECT_NOTE_STATUS_IDS.has(requestedStatus) ? requestedStatus : "active"
  };
}

function cleanText(value, limit) {
  return String(value ?? "").trim().slice(0, limit);
}

function dateValue(value) {
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? 0 : parsed;
}
