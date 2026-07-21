export const AUTOMATION_RECORD_PREFIX = "automation.";
export const AUTOMATION_DEFINITIONS_RECORD_KEY = "automation.definitions.v1";
export const AUTOMATION_EXECUTIONS_RECORD_KEY = "automation.executions.v1";

const MAX_DEFINITIONS = 60;
const MAX_EXECUTIONS = 240;
const MAX_LOG_ENTRIES = 12;
const CADENCES = ["once", "daily", "weekly", "interval"];
const EXECUTION_STATUSES = ["ready", "running", "completed", "needs-review", "blocked", "skipped", "cancelled"];
const WEEKDAYS = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];

export const AUTOMATION_ACTIONS = Object.freeze([
  {
    id: "refresh-study-recommendations",
    label: "Refresh study recommendations",
    description: "Recompute local study priorities without adding planner blocks.",
    category: "Study",
    icon: "fa-wand-magic-sparkles",
    execution: "refresh-study-recommendations",
    route: "study",
    requiresGateway: true,
    requiresProject: false
  },
  {
    id: "review-study-insights",
    label: "Review study signals",
    description: "Read the current study insights and keep a short local audit summary.",
    category: "Study",
    icon: "fa-brain",
    execution: "review-study-insights",
    route: "study",
    requiresGateway: true,
    requiresProject: false
  },
  {
    id: "review-planner",
    label: "Review planner",
    description: "Read the canonical schedule only; no blocks are changed.",
    category: "Planning",
    icon: "fa-calendar-check",
    execution: "review-planner",
    route: "planner",
    requiresGateway: true,
    requiresProject: false
  },
  {
    id: "research-supervision",
    label: "Run research supervision",
    description: "Inspect a chosen research project for reading priorities and open questions.",
    category: "Research",
    icon: "fa-user-graduate",
    execution: "research-supervision",
    route: "research",
    requiresGateway: true,
    requiresProject: true
  },
  {
    id: "research-gaps",
    label: "Check research gaps",
    description: "Inspect evidence gaps for a chosen research project without changing its sources.",
    category: "Research",
    icon: "fa-magnifying-glass-chart",
    execution: "research-gaps",
    route: "research",
    requiresGateway: true,
    requiresProject: true
  },
  workspaceAction("open-class-notes", "Open Class Notes", "Prepare the evidence-led notes workspace; course material remains your explicit choice.", "Class Notes", "fa-note-sticky", "class-notes"),
  workspaceAction("open-exam-practice", "Open Exam Practice", "Prepare the approved-material practice workspace; no questions are generated automatically.", "Exam Practice", "fa-pen-ruler", "exam-practice"),
  workspaceAction("open-research", "Open Research", "Open the paper workspace; downloads and indexing still require a direct confirmation.", "Research", "fa-flask", "research"),
  workspaceAction("open-study-review", "Open Study Review", "Open the recall workspace for a focused, student-led review.", "Study", "fa-book-open-reader", "study"),
  workspaceAction("open-student-os", "Open Student OS", "Open your canonical personal operating cards.", "Student OS", "fa-graduation-cap", "student-os"),
  workspaceAction("open-planner", "Open Planner", "Open the canonical schedule; calendar edits remain review and confirmation gated.", "Planning", "fa-calendar-days", "planner"),
  workspaceAction("open-routine-coach", "Open Routine Coach", "Open routine drafting; AI proposals never apply schedule changes on their own.", "Planning", "fa-clock", "routine"),
  workspaceAction("open-wellness", "Open Wellness", "Open the private wellness check-in; it does not alter academic plans.", "Wellness", "fa-heart-pulse", "wellness"),
  workspaceAction("open-memory-graph", "Open Memory Graph", "Open source-linked memory exploration.", "Study", "fa-diagram-project", "memory"),
  workspaceAction("open-chat", "Open Chat", "Open the study assistant with your chosen question.", "Assistant", "fa-message", "chat"),
  workspaceAction("open-agent-admin", "Open Agent Admin", "Open desktop-only agent management; configuration changes stay direct and confirmed.", "Assistant", "fa-sliders", "admin")
]);

export const AUTOMATION_TEMPLATES = Object.freeze([
  {
    id: "daily-study-reset",
    title: "Daily study reset",
    description: "Refresh today’s study priorities before choosing a focus block.",
    action_id: "refresh-study-recommendations",
    cadence: "daily",
    reminder_time: "08:00"
  },
  {
    id: "weekly-planner-review",
    title: "Weekly planner review",
    description: "Read your canonical schedule and then decide what needs attention.",
    action_id: "review-planner",
    cadence: "weekly",
    weekdays: ["sunday"],
    reminder_time: "18:00"
  },
  {
    id: "research-supervisor",
    title: "Research supervisor check-in",
    description: "Review one selected project’s evidence gaps and next reading step.",
    action_id: "research-supervision",
    cadence: "weekly",
    weekdays: ["wednesday"],
    reminder_time: "16:00"
  },
  {
    id: "wellness-check-in",
    title: "Wellness check-in",
    description: "Open the private tracker to choose realistic movement and meal checks.",
    action_id: "open-wellness",
    cadence: "daily",
    reminder_time: "19:00"
  }
]);

const ACTIONS_BY_ID = new Map(AUTOMATION_ACTIONS.map((action) => [action.id, action]));

export function isAutomationRecordKey(key) {
  return cleanText(key, 160).startsWith(AUTOMATION_RECORD_PREFIX);
}

export function getAutomationAction(actionId) {
  return ACTIONS_BY_ID.get(cleanText(actionId, 80)) ?? null;
}

export function localAutomationDate(date = new Date()) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function createAutomationId(prefix = "automation") {
  const safePrefix = cleanText(prefix, 30).replace(/[^a-z0-9-]+/gi, "-") || "automation";
  const randomId = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return `${safePrefix}-${randomId}`;
}

export function emptyAutomationDefinitions() {
  return { version: 1, definitions: [] };
}

export function emptyAutomationExecutions() {
  return { version: 1, executions: [] };
}

export function normalizeAutomationDefinitions(value, fallbackDate = localAutomationDate()) {
  const source = isObject(value) ? value : {};
  const definitions = uniqueById(source.definitions, MAX_DEFINITIONS, (definition) => normalizeDefinition(definition, fallbackDate));
  return { version: 1, definitions };
}

export function normalizeAutomationExecutions(value, fallbackDate = localAutomationDate()) {
  const source = isObject(value) ? value : {};
  const executions = uniqueById(source.executions, MAX_EXECUTIONS, (execution) => normalizeExecution(execution, fallbackDate))
    .sort((left, right) => String(right.updated_at).localeCompare(String(left.updated_at)));
  return { version: 1, executions };
}

export function createAutomationDraft(template, date = localAutomationDate()) {
  const source = isObject(template) ? template : {};
  return {
    title: cleanText(source.title, 120),
    description: cleanText(source.description, 480),
    action_id: getAutomationAction(source.action_id)?.id ?? "refresh-study-recommendations",
    cadence: enumValue(source.cadence, CADENCES, "daily"),
    weekdays: normalizeWeekdays(source.weekdays),
    interval_days: boundedInteger(source.interval_days, 2, 31, 2),
    reminder_time: validTime(source.reminder_time) ? source.reminder_time : "",
    start_date: validDate(source.start_date) ? source.start_date : date,
    research_project_id: cleanText(source.research_project_id, 180)
  };
}

export function buildAutomationDefinition(draft, { id = createAutomationId(), now = new Date() } = {}) {
  const createdAt = now.toISOString();
  const source = isObject(draft) ? draft : {};
  return normalizeDefinition({
    ...source,
    id,
    enabled: source.enabled !== false,
    created_at: source.created_at || createdAt,
    updated_at: createdAt
  }, localAutomationDate(now));
}

export function formatAutomationSchedule(definition) {
  const normalized = normalizeDefinition(definition, localAutomationDate());
  if (!normalized) {
    return "Schedule unavailable";
  }

  const reminder = normalized.reminder_time ? ` · ${normalized.reminder_time} reminder` : "";
  if (normalized.cadence === "once") {
    return `Once · ${formatDate(normalized.start_date)}${reminder}`;
  }
  if (normalized.cadence === "weekly") {
    const labels = normalized.weekdays.map((weekday) => weekday.slice(0, 3).replace(/^./, (letter) => letter.toUpperCase()));
    return `Weekly · ${labels.join(", ")}${reminder}`;
  }
  if (normalized.cadence === "interval") {
    return `Every ${normalized.interval_days} days · starts ${formatDate(normalized.start_date)}${reminder}`;
  }
  return `Every day${reminder}`;
}

export function isAutomationDueOnDate(definition, date = localAutomationDate()) {
  const normalized = normalizeDefinition(definition, date);
  const targetDate = validDate(date) ? date : localAutomationDate();
  if (!normalized || targetDate < normalized.start_date) {
    return false;
  }
  if (normalized.cadence === "once") {
    return targetDate === normalized.start_date;
  }
  if (normalized.cadence === "daily") {
    return true;
  }
  if (normalized.cadence === "weekly") {
    return normalized.weekdays.includes(weekdayForDate(targetDate));
  }
  const difference = epochDay(targetDate) - epochDay(normalized.start_date);
  return difference >= 0 && difference % normalized.interval_days === 0;
}

export function buildAutomationQueue(definitionsState, executionsState, date = localAutomationDate()) {
  const definitions = normalizeAutomationDefinitions(definitionsState, date).definitions;
  const executions = normalizeAutomationExecutions(executionsState, date).executions;
  const executionsByOccurrence = new Map(executions.map((execution) => [execution.occurrence_id, execution]));

  return definitions
    .filter((definition) => definition.enabled && isAutomationDueOnDate(definition, date))
    .map((definition) => {
      const occurrenceId = automationOccurrenceId(definition.id, date);
      const execution = executionsByOccurrence.get(occurrenceId) ?? null;
      return {
        occurrence_id: occurrenceId,
        definition,
        date,
        status: execution?.status ?? "ready",
        execution,
        action: getAutomationAction(definition.action_id)
      };
    })
    .sort((left, right) => {
      const timeOrder = String(left.definition.reminder_time || "99:99").localeCompare(String(right.definition.reminder_time || "99:99"));
      return timeOrder || left.definition.title.localeCompare(right.definition.title);
    });
}

export function automationOccurrenceId(definitionId, date = localAutomationDate()) {
  return `${cleanText(definitionId, 120)}:${validDate(date) ? date : localAutomationDate()}`;
}

export function createAutomationExecution(occurrence, { status = "ready", message = "Queued for explicit review.", now = new Date() } = {}) {
  const definition = occurrence?.definition;
  const action = occurrence?.action ?? getAutomationAction(definition?.action_id);
  const date = validDate(occurrence?.date) ? occurrence.date : localAutomationDate(now);
  const timestamp = now.toISOString();
  return normalizeExecution({
    id: createAutomationId("run"),
    occurrence_id: occurrence?.occurrence_id ?? automationOccurrenceId(definition?.id, date),
    definition_id: definition?.id,
    title: definition?.title ?? action?.label,
    action_id: action?.id,
    date,
    status,
    started_at: status === "running" ? timestamp : "",
    completed_at: terminalStatus(status) ? timestamp : "",
    updated_at: timestamp,
    logs: [{ at: timestamp, message }]
  }, date);
}

export function updateAutomationExecution(execution, { status, message, summary = "", now = new Date() } = {}) {
  const normalized = normalizeExecution(execution, localAutomationDate(now));
  if (!normalized) {
    return null;
  }
  const nextStatus = enumValue(status, EXECUTION_STATUSES, normalized.status);
  const timestamp = now.toISOString();
  const logs = message
    ? [...normalized.logs, { at: timestamp, message: cleanText(message, 420) }].slice(-MAX_LOG_ENTRIES)
    : normalized.logs;
  return normalizeExecution({
    ...normalized,
    status: nextStatus,
    summary: cleanText(summary, 420) || normalized.summary,
    started_at: normalized.started_at || (nextStatus === "running" ? timestamp : ""),
    completed_at: terminalStatus(nextStatus) ? timestamp : normalized.completed_at,
    updated_at: timestamp,
    logs
  }, localAutomationDate(now));
}

export function upsertAutomationExecution(executionsState, execution, fallbackDate = localAutomationDate()) {
  const normalizedState = normalizeAutomationExecutions(executionsState, fallbackDate);
  const normalizedExecution = normalizeExecution(execution, fallbackDate);
  if (!normalizedExecution) {
    return normalizedState;
  }
  const nextExecutions = [
    normalizedExecution,
    ...normalizedState.executions.filter((current) => current.occurrence_id !== normalizedExecution.occurrence_id)
  ].slice(0, MAX_EXECUTIONS);
  return normalizeAutomationExecutions({ version: 1, executions: nextExecutions }, fallbackDate);
}

export function executionSummary(actionId, result) {
  const action = getAutomationAction(actionId);
  if (!action) {
    return "Completed a confirmation-gated automation step.";
  }
  if (actionId === "refresh-study-recommendations" || actionId === "review-study-insights") {
    const recommendations = Array.isArray(result?.recommendations) ? result.recommendations.length : 0;
    const minutes = boundedInteger(result?.available_minutes, 0, 1440, 0);
    return `${recommendations} study priorities and ${minutes} available minutes reviewed.`;
  }
  if (actionId === "review-planner") {
    const days = Array.isArray(result?.days) ? result.days : [];
    const entries = days.reduce((total, day) => total + (Array.isArray(day?.entries) ? day.entries.length : 0), 0);
    return `${entries} canonical planner entries reviewed across ${days.length} days.`;
  }
  if (actionId === "research-supervision") {
    const papers = boundedInteger(result?.corpus_health?.total_papers, 0, 5000, 0);
    const nextSteps = Array.isArray(result?.next_steps) ? result.next_steps.length : 0;
    return `${papers} research papers and ${nextSteps} next steps reviewed.`;
  }
  if (actionId === "research-gaps") {
    const gaps = Array.isArray(result?.stated_gaps) ? result.stated_gaps.length : 0;
    return `${gaps} evidence gaps reviewed for the selected project.`;
  }
  return `${action.label} was opened after your explicit confirmation.`;
}

function workspaceAction(id, label, description, category, icon, route) {
  return Object.freeze({
    id,
    label,
    description,
    category,
    icon,
    execution: "workspace",
    route,
    requiresGateway: false,
    requiresProject: false
  });
}

function normalizeDefinition(value, fallbackDate) {
  if (!isObject(value)) {
    return null;
  }
  const id = cleanText(value.id, 120);
  const title = cleanText(value.title, 120);
  const action = getAutomationAction(value.action_id);
  if (!id || !title || !action) {
    return null;
  }
  const cadence = enumValue(value.cadence, CADENCES, "daily");
  const startDate = validDate(value.start_date) ? value.start_date : fallbackDate;
  const selectedWeekdays = normalizeWeekdays(value.weekdays);
  return {
    version: 1,
    id,
    title,
    description: cleanText(value.description, 480),
    action_id: action.id,
    cadence,
    weekdays: cadence === "weekly"
      ? (selectedWeekdays.length ? selectedWeekdays : [weekdayForDate(startDate)])
      : [],
    interval_days: cadence === "interval" ? boundedInteger(value.interval_days, 2, 31, 2) : 1,
    reminder_time: validTime(value.reminder_time) ? value.reminder_time : "",
    start_date: startDate,
    research_project_id: cleanText(value.research_project_id, 180),
    enabled: value.enabled !== false,
    created_at: cleanText(value.created_at, 40),
    updated_at: cleanText(value.updated_at, 40)
  };
}

function normalizeExecution(value, fallbackDate) {
  if (!isObject(value)) {
    return null;
  }
  const id = cleanText(value.id, 120);
  const definitionId = cleanText(value.definition_id, 120);
  const action = getAutomationAction(value.action_id);
  const date = validDate(value.date) ? value.date : fallbackDate;
  const occurrenceId = cleanText(value.occurrence_id, 180) || automationOccurrenceId(definitionId, date);
  if (!id || !definitionId || !action || !occurrenceId) {
    return null;
  }
  return {
    version: 1,
    id,
    occurrence_id: occurrenceId,
    definition_id: definitionId,
    title: cleanText(value.title, 120) || action.label,
    action_id: action.id,
    date,
    status: enumValue(value.status, EXECUTION_STATUSES, "ready"),
    summary: cleanText(value.summary, 420),
    started_at: cleanText(value.started_at, 40),
    completed_at: cleanText(value.completed_at, 40),
    updated_at: cleanText(value.updated_at, 40),
    logs: normalizeLogs(value.logs)
  };
}

function normalizeLogs(value) {
  const source = Array.isArray(value) ? value : [];
  const entries = [];
  for (const entry of source) {
    if (!isObject(entry)) {
      continue;
    }
    const message = cleanText(entry.message, 420);
    if (!message) {
      continue;
    }
    entries.push({ at: cleanText(entry.at, 40), message });
    if (entries.length >= MAX_LOG_ENTRIES) {
      break;
    }
  }
  return entries;
}

function normalizeWeekdays(value) {
  const source = Array.isArray(value) ? value : [];
  const selected = [];
  for (const weekday of source) {
    const normalized = cleanText(weekday, 16).toLowerCase();
    if (WEEKDAYS.includes(normalized) && !selected.includes(normalized)) {
      selected.push(normalized);
    }
  }
  return selected;
}

function uniqueById(value, limit, mapper) {
  const source = Array.isArray(value) ? value : [];
  const seen = new Set();
  const result = [];
  for (const item of source) {
    const normalized = mapper(item);
    if (!normalized || seen.has(normalized.id)) {
      continue;
    }
    seen.add(normalized.id);
    result.push(normalized);
    if (result.length >= limit) {
      break;
    }
  }
  return result;
}

function terminalStatus(status) {
  return ["completed", "needs-review", "blocked", "skipped", "cancelled"].includes(status);
}

function weekdayForDate(date) {
  const [year, month, day] = date.split("-").map(Number);
  return WEEKDAYS[new Date(year, month - 1, day).getDay()];
}

function epochDay(date) {
  const [year, month, day] = date.split("-").map(Number);
  return Math.floor(Date.UTC(year, month - 1, day) / 86400000);
}

function formatDate(date) {
  const [year, month, day] = date.split("-").map(Number);
  return new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: "numeric" }).format(new Date(year, month - 1, day));
}

function enumValue(value, allowed, fallback) {
  return allowed.includes(value) ? value : fallback;
}

function boundedInteger(value, minimum, maximum, fallback) {
  const number = Number(value);
  if (!Number.isFinite(number)) {
    return fallback;
  }
  return Math.min(maximum, Math.max(minimum, Math.round(number)));
}

function validDate(value) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(String(value ?? ""))) {
    return false;
  }
  const [year, month, day] = String(value).split("-").map(Number);
  const date = new Date(Date.UTC(year, month - 1, day));
  return date.getUTCFullYear() === year && date.getUTCMonth() === month - 1 && date.getUTCDate() === day;
}

function validTime(value) {
  return /^([01]\d|2[0-3]):[0-5]\d$/.test(String(value ?? ""));
}

function cleanText(value, maximum) {
  return typeof value === "string" ? value.trim().slice(0, maximum) : "";
}

function isObject(value) {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}
