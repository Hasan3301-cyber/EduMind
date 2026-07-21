const DAYS = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
const DAY_ALIASES = new Map([
  ["mon", "monday"],
  ["monday", "monday"],
  ["tue", "tuesday"],
  ["tues", "tuesday"],
  ["tuesday", "tuesday"],
  ["wed", "wednesday"],
  ["wednesday", "wednesday"],
  ["thu", "thursday"],
  ["thur", "thursday"],
  ["thurs", "thursday"],
  ["thursday", "thursday"],
  ["fri", "friday"],
  ["friday", "friday"],
  ["sat", "saturday"],
  ["saturday", "saturday"],
  ["sun", "sunday"],
  ["sunday", "sunday"]
]);

export function normalizeWeeklySchedule(records) {
  const byDay = new Map(DAYS.map((day) => [day, []]));
  for (const record of records ?? []) {
    if (record?.deleted || !record?.value || typeof record.value !== "object") {
      continue;
    }
    const day = normalizeDay(record.value.day ?? record.key);
    if (!byDay.has(day)) {
      continue;
    }
    byDay.get(day).push({
      id: record.key,
      title: String(record.value.title ?? "Study block"),
      start: String(record.value.start ?? ""),
      end: String(record.value.end ?? ""),
      source: record.value.source ?? "planner"
    });
  }
  return DAYS.map((day) => ({
    day,
    entries: byDay
      .get(day)
      .sort((left, right) => left.start.localeCompare(right.start) || left.title.localeCompare(right.title))
  }));
}

export function normalizeGatewaySchedule(schedule) {
  const byDay = new Map(
    (schedule?.days ?? []).map((entry) => [normalizeDay(entry?.day), entry?.entries ?? []])
  );
  return DAYS.map((day) => ({
    day,
    entries: [...(byDay.get(day) ?? [])].map((entry) => ({
      id: entry.id,
      title: String(entry.title ?? "Study block"),
      start: String(entry.start ?? ""),
      end: String(entry.end ?? ""),
      source: entry.source ?? "planner"
    }))
  }));
}

export function isValidTimeRange(start, end) {
  const startMinutes = timeToMinutes(start);
  const endMinutes = timeToMinutes(end);
  return startMinutes !== null && endMinutes !== null && startMinutes < endMinutes;
}

export function findScheduleConflicts(records) {
  const conflicts = [];
  for (const day of normalizeWeeklySchedule(records)) {
    const entries = day.entries
      .filter((entry) => isValidTimeRange(entry.start, entry.end))
      .sort((left, right) => left.start.localeCompare(right.start) || left.end.localeCompare(right.end));
    for (let index = 0; index < entries.length; index += 1) {
      const current = entries[index];
      const currentEnd = timeToMinutes(current.end);
      for (let nextIndex = index + 1; nextIndex < entries.length; nextIndex += 1) {
        const next = entries[nextIndex];
        if (timeToMinutes(next.start) >= currentEnd) {
          break;
        }
        conflicts.push({ day: day.day, firstId: current.id, secondId: next.id });
      }
    }
  }
  return conflicts;
}

export function parseRoutineText(text) {
  const blocks = [];
  const unparsedLines = [];
  for (const rawLine of String(text ?? "").split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line) {
      continue;
    }
    const match = line.match(
      /^(?<day>[A-Za-z]+)\s*(?:[|,]|\s)\s*(?<start>\d{1,2}:\d{2})\s*(?:-|–|—|to)\s*(?<end>\d{1,2}:\d{2})\s*(?:[|,]|\s)+(?<title>.+)$/i
    );
    const day = normalizeDay(match?.groups?.day);
    const start = normalizeTime(match?.groups?.start);
    const end = normalizeTime(match?.groups?.end);
    const title = match?.groups?.title?.trim();
    if (!day || !title || !isValidTimeRange(start, end)) {
      unparsedLines.push(rawLine);
      continue;
    }
    blocks.push({
      kind: "schedule-block",
      day: titleCase(day),
      title,
      start,
      end,
      source: "timetable-import"
    });
  }
  return { blocks, unparsedLines };
}

export function upsertPlannerRecord(records, key, value, updatedAt = new Date().toISOString()) {
  const next = [...(records ?? [])];
  const index = next.findIndex((record) => record.key === key);
  const entry = {
    key,
    value,
    deleted: false,
    updated_at: updatedAt
  };
  if (index === -1) {
    next.push(entry);
  } else {
    next[index] = entry;
  }
  return next;
}

export function deletePlannerRecord(records, key, updatedAt = new Date().toISOString()) {
  return (records ?? []).map((record) =>
    record.key === key ? { ...record, deleted: true, updated_at: updatedAt } : record
  );
}

function normalizeDay(value) {
  return DAY_ALIASES.get(String(value ?? "").trim().toLowerCase()) ?? "";
}

function normalizeTime(value) {
  const match = String(value ?? "").trim().match(/^(\d{1,2}):(\d{2})$/);
  if (!match) {
    return "";
  }
  const hours = Number(match[1]);
  const minutes = Number(match[2]);
  if (hours > 23 || minutes > 59) {
    return "";
  }
  return `${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}`;
}

function timeToMinutes(value) {
  const normalized = normalizeTime(value);
  if (!normalized) {
    return null;
  }
  const [hours, minutes] = normalized.split(":").map(Number);
  return (hours * 60) + minutes;
}

function titleCase(value) {
  return `${value[0].toUpperCase()}${value.slice(1)}`;
}
