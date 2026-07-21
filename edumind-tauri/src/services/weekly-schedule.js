export const WEEKDAY_NAMES = Object.freeze([
  "Monday",
  "Tuesday",
  "Wednesday",
  "Thursday",
  "Friday",
  "Saturday",
  "Sunday"
]);

const MAX_ENTRIES_PER_DAY = 48;
const MAX_ENTRY_TEXT = 160;
const WEEKDAY_BY_PREFIX = new Map(WEEKDAY_NAMES.map((day) => [day.slice(0, 3).toLowerCase(), day]));

export function canonicalWeeklySchedule(schedule, referenceDate = new Date()) {
  const entriesByDay = new Map(WEEKDAY_NAMES.map((day) => [day, []]));
  const sourceDays = Array.isArray(schedule?.days) ? schedule.days : [];

  for (const sourceDay of sourceDays) {
    const day = canonicalDayName(sourceDay?.day);
    if (!day) {
      continue;
    }
    const destination = entriesByDay.get(day);
    const remaining = Math.max(0, MAX_ENTRIES_PER_DAY - destination.length);
    const sourceEntries = Array.isArray(sourceDay?.entries) ? sourceDay.entries.slice(0, remaining) : [];
    for (const sourceEntry of sourceEntries) {
      const entry = normalizePlannerEntry(sourceEntry, day, destination.length);
      if (entry) {
        destination.push(entry);
      }
    }
  }

  const monday = startOfLocalWeek(referenceDate);
  return WEEKDAY_NAMES.map((day, index) => {
    const date = new Date(monday);
    date.setDate(monday.getDate() + index);
    return {
      day,
      date,
      dateKey: localDateKey(date),
      entries: entriesByDay.get(day).sort(comparePlannerEntries)
    };
  });
}

export function hasCanonicalSchedule(schedule) {
  return Array.isArray(schedule?.days);
}

export function scheduleEntryCount(scheduleDays) {
  return (Array.isArray(scheduleDays) ? scheduleDays : []).reduce(
    (total, day) => total + (Array.isArray(day?.entries) ? day.entries.length : 0),
    0
  );
}

export function formatScheduleTime(value) {
  const normalized = normalizeTime(value);
  if (!normalized) {
    return "Time TBD";
  }
  const [hoursText, minutesText] = normalized.split(":");
  const hours = Number(hoursText);
  return `${hours % 12 || 12}:${minutesText} ${hours >= 12 ? "PM" : "AM"}`;
}

export function formatScheduleDay(date) {
  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" }).format(date);
}

export function formatScheduleWeek(scheduleDays) {
  const firstDay = Array.isArray(scheduleDays) ? scheduleDays[0]?.date : null;
  const lastDay = Array.isArray(scheduleDays) ? scheduleDays.at(-1)?.date : null;
  if (!(firstDay instanceof Date) || Number.isNaN(firstDay.getTime()) || !(lastDay instanceof Date) || Number.isNaN(lastDay.getTime())) {
    return "This week";
  }
  const firstMonth = new Intl.DateTimeFormat(undefined, { month: "short" }).format(firstDay);
  const lastMonth = new Intl.DateTimeFormat(undefined, { month: "short" }).format(lastDay);
  const year = new Intl.DateTimeFormat(undefined, { year: "numeric" }).format(lastDay);
  return firstMonth === lastMonth
    ? `${firstMonth} ${firstDay.getDate()}–${lastDay.getDate()}, ${year}`
    : `${firstMonth} ${firstDay.getDate()} – ${lastMonth} ${lastDay.getDate()}, ${year}`;
}

export function isSameLocalDate(firstDate, secondDate) {
  return firstDate instanceof Date && secondDate instanceof Date
    && firstDate.getFullYear() === secondDate.getFullYear()
    && firstDate.getMonth() === secondDate.getMonth()
    && firstDate.getDate() === secondDate.getDate();
}

function canonicalDayName(value) {
  return WEEKDAY_BY_PREFIX.get(cleanText(value, 32).slice(0, 3).toLowerCase()) ?? "";
}

function normalizePlannerEntry(entry, day, index) {
  const title = cleanText(entry?.title, MAX_ENTRY_TEXT);
  if (!title) {
    return null;
  }
  return {
    id: cleanText(entry?.id, MAX_ENTRY_TEXT) || `${day.toLowerCase()}-${index}-${title.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`,
    title,
    start: normalizeTime(entry?.start),
    end: normalizeTime(entry?.end),
    source: cleanText(entry?.source, MAX_ENTRY_TEXT) || "Planner"
  };
}

function normalizeTime(value) {
  const candidate = cleanText(value, 8);
  return /^([01]\d|2[0-3]):[0-5]\d$/.test(candidate) ? candidate : "";
}

function comparePlannerEntries(firstEntry, secondEntry) {
  return `${firstEntry.start || "99:99"}-${firstEntry.title}`.localeCompare(`${secondEntry.start || "99:99"}-${secondEntry.title}`);
}

function startOfLocalWeek(referenceDate) {
  const date = referenceDate instanceof Date && !Number.isNaN(referenceDate.getTime()) ? new Date(referenceDate) : new Date();
  date.setHours(12, 0, 0, 0);
  const mondayOffset = (date.getDay() + 6) % 7;
  date.setDate(date.getDate() - mondayOffset);
  return date;
}

function localDateKey(date) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function cleanText(value, limit) {
  return String(value ?? "").trim().slice(0, limit);
}
