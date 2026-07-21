const DAYS = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const DAY_ALIASES = new Map([
  ["mon", "Monday"],
  ["monday", "Monday"],
  ["tue", "Tuesday"],
  ["tues", "Tuesday"],
  ["tuesday", "Tuesday"],
  ["wed", "Wednesday"],
  ["wednesday", "Wednesday"],
  ["thu", "Thursday"],
  ["thur", "Thursday"],
  ["thurs", "Thursday"],
  ["thursday", "Thursday"],
  ["fri", "Friday"],
  ["friday", "Friday"],
  ["sat", "Saturday"],
  ["saturday", "Saturday"],
  ["sun", "Sunday"],
  ["sunday", "Sunday"]
]);
const ALLOWED_KINDS = new Set(["study", "review", "assignment", "admin", "wellbeing", "recovery"]);
const MAX_BLOCKS = 10;
const MAX_IMAGE_OBSERVATIONS = 20;
const MAX_MODEL_TEXT = 32000;

export const ROUTINE_DAYS = DAYS;

export function parseRoutineProposal(modelText) {
  const parsed = extractJsonObject(modelText);
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { proposal: null, issues: ["The AI response did not contain a routine proposal in the expected JSON format."] };
  }

  const rawBlocks = Array.isArray(parsed.blocks)
    ? parsed.blocks
    : Array.isArray(parsed.proposed_blocks)
      ? parsed.proposed_blocks
      : [];
  const rawImageObservations = Array.isArray(parsed.image_observations)
    ? parsed.image_observations
    : Array.isArray(parsed.timetable_observations)
      ? parsed.timetable_observations
      : [];
  const issues = [];
  const seen = new Set();
  const blocks = [];
  const imageObservationSeen = new Set();
  const imageObservations = [];

  for (const [index, rawBlock] of rawBlocks.slice(0, MAX_BLOCKS * 4).entries()) {
    const block = normalizeBlock(rawBlock, index);
    if (!block) {
      issues.push(`Ignored an invalid proposed block at position ${index + 1}.`);
      continue;
    }
    const fingerprint = [block.day, block.start, block.end, block.title.toLowerCase()].join("|");
    if (seen.has(fingerprint)) {
      issues.push(`Ignored a duplicate block: ${block.title}.`);
      continue;
    }
    seen.add(fingerprint);
    blocks.push(block);
    if (blocks.length === MAX_BLOCKS) {
      break;
    }
  }
  if (rawBlocks.length > MAX_BLOCKS * 4) {
    issues.push("Ignored extra proposed blocks beyond the safe review limit.");
  }

  for (const [index, rawObservation] of rawImageObservations.slice(0, MAX_IMAGE_OBSERVATIONS * 4).entries()) {
    const observation = normalizeImageObservation(rawObservation, index);
    if (!observation) {
      issues.push(`Ignored an invalid timetable observation at position ${index + 1}.`);
      continue;
    }
    const fingerprint = [observation.day, observation.start, observation.end, observation.title.toLowerCase()].join("|");
    if (imageObservationSeen.has(fingerprint)) {
      issues.push(`Ignored a duplicate timetable observation: ${observation.title}.`);
      continue;
    }
    imageObservationSeen.add(fingerprint);
    imageObservations.push(observation);
    if (imageObservations.length === MAX_IMAGE_OBSERVATIONS) {
      break;
    }
  }
  if (rawImageObservations.length > MAX_IMAGE_OBSERVATIONS * 4) {
    issues.push("Ignored extra timetable observations beyond the safe review limit.");
  }

  if (!blocks.length && !imageObservations.length) {
    return {
      proposal: null,
      issues: issues.length ? issues : ["The AI did not return any valid routine blocks or timetable observations."]
    };
  }

  blocks.sort((left, right) => {
    const dayOrder = DAYS.indexOf(left.day) - DAYS.indexOf(right.day);
    return dayOrder || left.start.localeCompare(right.start) || left.end.localeCompare(right.end);
  });
  imageObservations.sort((left, right) => {
    const dayOrder = DAYS.indexOf(left.day) - DAYS.indexOf(right.day);
    return dayOrder || left.start.localeCompare(right.start) || left.end.localeCompare(right.end);
  });

  return {
    proposal: {
      title: cleanText(parsed.title, 96) || "AI routine proposal",
      summary: cleanText(parsed.summary, 420),
      assumptions: cleanTextList(parsed.assumptions, 5, 180),
      tradeoffs: cleanTextList(parsed.tradeoffs, 5, 180),
      blocks,
      imageObservations
    },
    issues
  };
}

export function findRoutineProposalConflicts(blocks, schedule) {
  const scheduleByDay = new Map(
    (schedule?.days ?? []).map((scheduleDay) => [
      normalizeDay(scheduleDay?.day),
      Array.isArray(scheduleDay?.entries) ? scheduleDay.entries : []
    ])
  );
  const conflicts = [];

  for (const block of blocks ?? []) {
    const candidates = scheduleByDay.get(normalizeDay(block?.day)) ?? [];
    for (const existing of candidates) {
      if (!isTimeRange(existing?.start, existing?.end) || !rangesOverlap(block.start, block.end, existing.start, existing.end)) {
        continue;
      }
      const duplicate = block.start === existing.start
        && block.end === existing.end
        && String(block.title).trim().toLowerCase() === String(existing.title ?? "").trim().toLowerCase();
      conflicts.push({
        blockId: block.id,
        blockTitle: block.title,
        day: block.day,
        start: block.start,
        end: block.end,
        existingId: existing.id,
        existingTitle: cleanText(existing.title, 120) || "Existing planner block",
        existingStart: existing.start,
        existingEnd: existing.end,
        type: duplicate ? "duplicate" : "overlap"
      });
    }
  }

  const proposals = Array.isArray(blocks) ? blocks : [];
  for (let index = 0; index < proposals.length; index += 1) {
    const block = proposals[index];
    for (let nextIndex = index + 1; nextIndex < proposals.length; nextIndex += 1) {
      const otherBlock = proposals[nextIndex];
      if (block.day !== otherBlock.day || !rangesOverlap(block.start, block.end, otherBlock.start, otherBlock.end)) {
        continue;
      }
      conflicts.push({
        blockId: block.id,
        blockTitle: block.title,
        day: block.day,
        start: block.start,
        end: block.end,
        existingId: otherBlock.id,
        existingTitle: otherBlock.title,
        existingStart: otherBlock.start,
        existingEnd: otherBlock.end,
        type: "proposal-overlap"
      });
    }
  }

  return conflicts;
}

export function createRoutineRecordValue(block, routinePlanId) {
  return {
    kind: "schedule-block",
    title: block.title,
    day: block.day,
    start: block.start,
    end: block.end,
    source: "routine-ai",
    routine_owner: "ai",
    routine_plan_id: routinePlanId,
    routine_kind: block.kind
  };
}

export function createPlannerImageRecordValue(observation, plannerImportId) {
  return {
    kind: "schedule-block",
    title: observation.title,
    day: observation.day,
    start: observation.start,
    end: observation.end,
    detail: observation.detail,
    source: "planner-image-ai",
    planner_owner: "image-import",
    planner_import_id: plannerImportId,
    planner_confidence: observation.confidence
  };
}

export function mergeRoutineImageObservations(schedule, observations) {
  const entriesByDay = new Map(DAYS.map((day) => [day, []]));
  for (const scheduleDay of schedule?.days ?? []) {
    const day = normalizeDay(scheduleDay?.day);
    if (!day || !entriesByDay.has(day)) {
      continue;
    }
    const entries = Array.isArray(scheduleDay?.entries) ? scheduleDay.entries : [];
    entriesByDay.get(day).push(...entries.map((entry) => ({ ...entry })));
  }
  for (const observation of observations ?? []) {
    const day = normalizeDay(observation?.day);
    if (!day || !entriesByDay.has(day) || !isTimeRange(observation?.start, observation?.end)) {
      continue;
    }
    entriesByDay.get(day).push({
      id: observation.id,
      title: observation.title,
      start: observation.start,
      end: observation.end,
      source: "planner-image-ai"
    });
  }
  return {
    ...(schedule && typeof schedule === "object" ? schedule : {}),
    days: DAYS.map((day) => ({ day, entries: entriesByDay.get(day) ?? [] }))
  };
}

export function routineBlockDuration(block) {
  if (!isTimeRange(block?.start, block?.end)) {
    return 0;
  }
  return timeToMinutes(block.end) - timeToMinutes(block.start);
}

export function formatRoutineKind(kind) {
  return {
    study: "Study",
    review: "Review",
    assignment: "Assignment",
    admin: "Admin",
    wellbeing: "Wellbeing",
    recovery: "Recovery"
  }[kind] ?? "Routine";
}

function normalizeBlock(rawBlock, index) {
  if (!rawBlock || typeof rawBlock !== "object" || Array.isArray(rawBlock)) {
    return null;
  }
  const day = normalizeDay(rawBlock.day);
  const start = cleanTime(rawBlock.start);
  const end = cleanTime(rawBlock.end);
  const title = cleanText(rawBlock.title, 96);
  const kind = String(rawBlock.kind ?? "").trim().toLowerCase();
  const detail = cleanText(rawBlock.detail ?? rawBlock.details, 240);

  if (!day || !title || !ALLOWED_KINDS.has(kind) || !isTimeRange(start, end)) {
    return null;
  }
  const duration = timeToMinutes(end) - timeToMinutes(start);
  if (duration < 15 || duration > 240) {
    return null;
  }

  return {
    id: `proposal-${index + 1}`,
    day,
    start,
    end,
    title,
    kind,
    detail
  };
}

function normalizeImageObservation(rawObservation, index) {
  if (!rawObservation || typeof rawObservation !== "object" || Array.isArray(rawObservation)) {
    return null;
  }
  const day = normalizeDay(rawObservation.day);
  const start = cleanTime(rawObservation.start);
  const end = cleanTime(rawObservation.end);
  const title = cleanText(rawObservation.title, 96);
  const detail = cleanText(rawObservation.detail ?? rawObservation.details, 180);
  const confidence = typeof rawObservation.confidence === "number" ? rawObservation.confidence : Number.NaN;

  if (!day || !title || !isTimeRange(start, end) || !Number.isFinite(confidence) || confidence < 0 || confidence > 1) {
    return null;
  }
  const duration = timeToMinutes(end) - timeToMinutes(start);
  if (duration < 15 || duration > 480) {
    return null;
  }

  return {
    id: `image-observation-${index + 1}`,
    day,
    start,
    end,
    title,
    detail,
    confidence: Math.round(confidence * 100) / 100
  };
}

function extractJsonObject(value) {
  const source = String(value ?? "").slice(0, MAX_MODEL_TEXT).trim();
  if (!source) {
    return null;
  }
  const candidates = [source];
  for (const match of source.matchAll(/```(?:json)?\s*([\s\S]*?)```/gi)) {
    if (match[1]) {
      candidates.unshift(match[1].trim());
    }
  }
  const start = source.indexOf("{");
  if (start >= 0) {
    const balanced = balancedJsonObject(source, start);
    if (balanced) {
      candidates.push(balanced);
    }
  }
  for (const candidate of candidates) {
    try {
      return JSON.parse(candidate);
    } catch {
      continue;
    }
  }
  return null;
}

function balancedJsonObject(source, start) {
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let index = start; index < source.length; index += 1) {
    const character = source[index];
    if (inString) {
      if (escaped) {
        escaped = false;
      } else if (character === "\\") {
        escaped = true;
      } else if (character === '"') {
        inString = false;
      }
      continue;
    }
    if (character === '"') {
      inString = true;
    } else if (character === "{") {
      depth += 1;
    } else if (character === "}") {
      depth -= 1;
      if (depth === 0) {
        return source.slice(start, index + 1);
      }
    }
  }
  return null;
}

function normalizeDay(value) {
  return DAY_ALIASES.get(String(value ?? "").trim().toLowerCase()) ?? "";
}

function cleanTime(value) {
  const normalized = String(value ?? "").trim();
  return /^([01]\d|2[0-3]):[0-5]\d$/.test(normalized) ? normalized : "";
}

function isTimeRange(start, end) {
  return Boolean(start && end && timeToMinutes(start) < timeToMinutes(end));
}

function rangesOverlap(firstStart, firstEnd, secondStart, secondEnd) {
  return timeToMinutes(firstStart) < timeToMinutes(secondEnd)
    && timeToMinutes(secondStart) < timeToMinutes(firstEnd);
}

function timeToMinutes(value) {
  const [hours, minutes] = String(value).split(":").map(Number);
  return (hours * 60) + minutes;
}

function cleanText(value, limit) {
  if (typeof value !== "string") {
    return "";
  }
  const normalized = stripControlCharacters(value).replace(/\s+/g, " ").trim();
  return normalized.slice(0, limit);
}

function cleanTextList(value, maximumItems, maximumLength) {
  if (!Array.isArray(value)) {
    return [];
  }
  return value
    .map((item) => cleanText(item, maximumLength))
    .filter(Boolean)
    .slice(0, maximumItems);
}

function stripControlCharacters(value) {
  return Array.from(value)
    .filter((character) => {
      const code = character.codePointAt(0);
      return code !== undefined && code >= 32 && code !== 127;
    })
    .join("");
}
