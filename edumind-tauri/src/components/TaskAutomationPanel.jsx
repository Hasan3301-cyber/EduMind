import { useEffect, useMemo, useState } from "react";

import {
  AUTOMATION_ACTIONS,
  AUTOMATION_DEFINITIONS_RECORD_KEY,
  AUTOMATION_EXECUTIONS_RECORD_KEY,
  AUTOMATION_TEMPLATES,
  buildAutomationDefinition,
  buildAutomationQueue,
  createAutomationDraft,
  createAutomationExecution,
  emptyAutomationDefinitions,
  emptyAutomationExecutions,
  executionSummary,
  formatAutomationSchedule,
  getAutomationAction,
  localAutomationDate,
  normalizeAutomationDefinitions,
  normalizeAutomationExecutions,
  updateAutomationExecution,
  upsertAutomationExecution
} from "../services/task-automation";

const WEEKDAYS = [
  { id: "monday", label: "Mon" },
  { id: "tuesday", label: "Tue" },
  { id: "wednesday", label: "Wed" },
  { id: "thursday", label: "Thu" },
  { id: "friday", label: "Fri" },
  { id: "saturday", label: "Sat" },
  { id: "sunday", label: "Sun" }
];

function defaultDraft(date = localAutomationDate()) {
  return createAutomationDraft({
    title: "",
    description: "",
    action_id: "refresh-study-recommendations",
    cadence: "daily",
    reminder_time: "",
    start_date: date
  }, date);
}

export function TaskAutomationPanel({ client, connectionState, onNavigate }) {
  const [definitionState, setDefinitionState] = useState(() => emptyAutomationDefinitions());
  const [executionState, setExecutionState] = useState(() => emptyAutomationExecutions());
  const [projects, setProjects] = useState([]);
  const [draft, setDraft] = useState(() => defaultDraft());
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [runningOccurrenceId, setRunningOccurrenceId] = useState(null);
  const [status, setStatus] = useState("Loading your student-owned automation queue…");
  const [error, setError] = useState(null);

  const today = useMemo(() => localAutomationDate(), []);
  const selectedAction = getAutomationAction(draft.action_id) ?? AUTOMATION_ACTIONS[0];
  const queue = useMemo(
    () => buildAutomationQueue(definitionState, executionState, today),
    [definitionState, executionState, today]
  );
  const readyCount = queue.filter((item) => ["ready", "blocked", "needs-review"].includes(item.status)).length;
  const completedCount = queue.filter((item) => item.status === "completed").length;
  const enabledCount = definitionState.definitions.filter((definition) => definition.enabled).length;
  const recentExecutions = executionState.executions.slice(0, 8);
  const connected = Boolean(client && connectionState === "connected");
  const busy = isLoading || isSaving || Boolean(runningOccurrenceId);

  useEffect(() => {
    let active = true;

    async function load() {
      if (!client) {
        if (active) {
          setIsLoading(false);
          setStatus("Offline preview is ready. Connect the desktop gateway to save automation definitions and audit records locally.");
        }
        return;
      }

      const [snapshotResult, projectResult] = await Promise.allSettled([
        client.studentPage("student-os"),
        client.listProjects()
      ]);
      if (!active) {
        return;
      }

      if (snapshotResult.status === "fulfilled") {
        const records = Array.isArray(snapshotResult.value?.records) ? snapshotResult.value.records : [];
        setDefinitionState(normalizeAutomationDefinitions(activeRecordValue(records, AUTOMATION_DEFINITIONS_RECORD_KEY), today));
        setExecutionState(normalizeAutomationExecutions(activeRecordValue(records, AUTOMATION_EXECUTIONS_RECORD_KEY), today));
        setStatus("Your automation definitions are local, visible, and ready for review.");
      } else {
        setError(safeMessage(snapshotResult.reason, "EduMind could not load the task automation records."));
        setStatus("A temporary local automation queue is available for this session.");
      }

      if (projectResult.status === "fulfilled") {
        setProjects(projectList(projectResult.value));
      }
      setIsLoading(false);
    }

    void load();
    return () => {
      active = false;
    };
  }, [client, today]);

  async function persistDefinitions(nextState, successMessage) {
    if (isSaving) {
      return false;
    }
    const previous = definitionState;
    const normalized = normalizeAutomationDefinitions(nextState, today);
    setDefinitionState(normalized);
    setIsSaving(true);
    setError(null);
    try {
      if (!client) {
        setStatus("Updated in this preview session. Connect the desktop gateway to save this automation locally.");
        return true;
      }
      await client.upsertStudentPageRecord("student-os", AUTOMATION_DEFINITIONS_RECORD_KEY, normalized, { source: "task-automation" });
      setStatus(successMessage);
      return true;
    } catch (reason) {
      setDefinitionState(previous);
      setError(safeMessage(reason, "EduMind could not save the automation definition."));
      return false;
    } finally {
      setIsSaving(false);
    }
  }

  async function persistExecutions(nextState, successMessage) {
    if (isSaving) {
      return false;
    }
    const previous = executionState;
    const normalized = normalizeAutomationExecutions(nextState, today);
    setExecutionState(normalized);
    setIsSaving(true);
    setError(null);
    try {
      if (!client) {
        setStatus("Updated in this preview session. Connect the desktop gateway to save the audit trail locally.");
        return true;
      }
      await client.upsertStudentPageRecord("student-os", AUTOMATION_EXECUTIONS_RECORD_KEY, normalized, { source: "task-automation" });
      setStatus(successMessage);
      return true;
    } catch (reason) {
      setExecutionState(previous);
      setError(safeMessage(reason, "EduMind could not save the automation audit trail."));
      return false;
    } finally {
      setIsSaving(false);
    }
  }

  async function saveAutomation(event) {
    event.preventDefault();
    if (isSaving) {
      return;
    }
    if (!draft.title.trim()) {
      setStatus("Give this automation a clear name before saving it.");
      return;
    }
    if (selectedAction.requiresProject && !draft.research_project_id) {
      setStatus("Choose the research project this automation may inspect.");
      return;
    }
    const definition = buildAutomationDefinition({
      ...draft,
      title: draft.title.trim(),
      description: draft.description.trim(),
      start_date: draft.start_date || today
    });
    if (!definition) {
      setStatus("Choose one of the protected automation actions before saving.");
      return;
    }
    const saved = await persistDefinitions({
      ...definitionState,
      definitions: [...definitionState.definitions, definition]
    }, `Saved “${definition.title}”. Its schedule now appears as a review queue, not a background runner.`);
    if (saved) {
      setDraft(defaultDraft(today));
    }
  }

  function useTemplate(template) {
    const projectId = template.action_id.startsWith("research-")
      ? projects[0]?.id ?? ""
      : "";
    setDraft(createAutomationDraft({ ...template, research_project_id: projectId, start_date: today }, today));
    setStatus(`Template loaded: ${template.title}. Review it before creating the automation.`);
  }

  async function toggleDefinition(definition) {
    const nextEnabled = !definition.enabled;
    await persistDefinitions({
      ...definitionState,
      definitions: definitionState.definitions.map((current) => current.id === definition.id
        ? { ...current, enabled: nextEnabled, updated_at: new Date().toISOString() }
        : current)
    }, `${nextEnabled ? "Resumed" : "Paused"} “${definition.title}”.`);
  }

  async function removeDefinition(definition) {
    if (!window.confirm(`Remove “${definition.title}”? Its saved audit history will remain available.`)) {
      return;
    }
    await persistDefinitions({
      ...definitionState,
      definitions: definitionState.definitions.filter((current) => current.id !== definition.id)
    }, `Removed “${definition.title}”.`);
  }

  async function skipOccurrence(occurrence) {
    if (!window.confirm(`Skip “${occurrence.definition.title}” for today? This records the choice but does not change any study, planner, or research data.`)) {
      return;
    }
    const current = occurrence.execution ?? createAutomationExecution(occurrence, {
      status: "ready",
      message: "Queued for explicit review."
    });
    const skipped = updateAutomationExecution(current, {
      status: "skipped",
      message: "Skipped by the student. No workspace data changed."
    });
    if (!skipped) {
      return;
    }
    await persistExecutions(upsertAutomationExecution(executionState, skipped, today), `Skipped “${occurrence.definition.title}” for today.`);
  }

  async function runOccurrence(occurrence) {
    const action = occurrence.action;
    if (!action) {
      setStatus("This automation action is no longer on EduMind’s protected allow-list.");
      return;
    }
    if (action.requiresGateway && !connected) {
      setStatus("Connect the desktop gateway before running this local read-only check.");
      return;
    }
    if (action.requiresProject && !occurrence.definition.research_project_id) {
      setStatus("Choose a research project in the automation definition before running this check.");
      return;
    }
    const confirmation = `Run “${occurrence.definition.title}”?\n\nThis will: ${action.description}\n\nIt will not write planner blocks, download or index papers, create course material, change agent settings, or contact an external service.`;
    if (!window.confirm(confirmation)) {
      return;
    }

    setRunningOccurrenceId(occurrence.occurrence_id);
    const current = occurrence.execution ?? createAutomationExecution(occurrence, {
      status: "ready",
      message: "Queued for explicit review."
    });
    const running = updateAutomationExecution(current, {
      status: "running",
      message: "Explicitly confirmed by the student."
    });
    if (!running) {
      setRunningOccurrenceId(null);
      return;
    }
    const started = await persistExecutions(upsertAutomationExecution(executionState, running, today), `Running “${occurrence.definition.title}”…`);
    if (!started) {
      setRunningOccurrenceId(null);
      return;
    }

    try {
      if (action.execution === "workspace") {
        const completed = updateAutomationExecution(running, {
          status: "completed",
          message: "Opened the relevant workspace after explicit confirmation.",
          summary: executionSummary(action.id)
        });
        if (completed) {
          await persistExecutions(upsertAutomationExecution(executionState, completed, today), `${action.label} is ready.`);
        }
        onNavigate(action.route);
        return;
      }

      const result = await executeGatewayAction(action, client, occurrence.definition.research_project_id);
      const completed = updateAutomationExecution(running, {
        status: "completed",
        message: "Read-only automation step completed. No protected workspace data changed.",
        summary: executionSummary(action.id, result)
      });
      if (completed) {
        const saved = await persistExecutions(upsertAutomationExecution(executionState, completed, today), `Completed “${occurrence.definition.title}”.`);
        if (saved) {
          setStatus(`Completed “${occurrence.definition.title}”.`);
        }
      }
    } catch (reason) {
      const blocked = updateAutomationExecution(running, {
        status: "blocked",
        message: "The gateway check did not complete. No protected workspace data changed.",
        summary: "Review the local gateway connection or selected research project, then retry."
      });
      if (blocked) {
        const saved = await persistExecutions(upsertAutomationExecution(executionState, blocked, today), `“${occurrence.definition.title}” needs review before it can run again.`);
        if (saved) {
          setStatus(`“${occurrence.definition.title}” needs review before it can run again.`);
        }
      }
      setError(safeMessage(reason, "EduMind could not complete this automation step."));
    } finally {
      setRunningOccurrenceId(null);
    }
  }

  function changeDraft(field, value) {
    setDraft((current) => ({ ...current, [field]: value }));
  }

  function changeAction(actionId) {
    const action = getAutomationAction(actionId);
    setDraft((current) => ({
      ...current,
      action_id: actionId,
      research_project_id: action?.requiresProject ? current.research_project_id : ""
    }));
  }

  function toggleWeekday(weekday) {
    setDraft((current) => {
      const weekdays = current.weekdays.includes(weekday)
        ? current.weekdays.filter((currentDay) => currentDay !== weekday)
        : [...current.weekdays, weekday];
      return { ...current, weekdays };
    });
  }

  return (
    <section className="task-automation-panel premium-automation-panel">
      <header className="automation-hero">
        <div>
          <p className="eyebrow">Task automation</p>
          <h1>Keep every study workspace moving, while you stay in control.</h1>
          <p>Create recurring task definitions for EduMind’s study, planning, research, wellness, and assistant workspaces. A schedule creates a visible review queue only—nothing runs in the background or makes protected changes by itself.</p>
        </div>
        <aside>
          <i className="fa-solid fa-shield-halved" aria-hidden="true" />
          <strong>Explicit-run guardrails</strong>
          <span>Every task run is confirmation-gated. Planner changes, paper downloads, content generation, agent configuration, and external actions remain direct student decisions.</span>
        </aside>
      </header>

      <p className="automation-status" role="status" aria-live="polite">{isLoading ? "Loading local automation records…" : status}</p>
      {error ? <p className="error-message" role="alert">{error}</p> : null}

      <section className="automation-metric-grid" aria-label="Task automation summary">
        <Metric icon="fa-list-check" label="Ready today" value={readyCount} detail={readyCount ? "Awaiting your explicit run or skip." : "Nothing is waiting for review."} tone="mint" />
        <Metric icon="fa-toggle-on" label="Enabled automations" value={enabledCount} detail="Schedules appear only while you open EduMind." tone="sky" />
        <Metric icon="fa-circle-check" label="Completed today" value={completedCount} detail="Short audit records stay in Student OS state." tone="gold" />
        <Metric icon="fa-lock" label="Protected actions" value="Manual" detail="Writes, downloads, and submissions are never unattended." tone="coral" />
      </section>

      <section className="automation-top-grid">
        <article className="automation-card automation-queue-card" aria-labelledby="automation-queue-heading">
          <div className="automation-card-heading">
            <div>
              <p className="eyebrow">Today</p>
              <h2 id="automation-queue-heading">Review queue</h2>
            </div>
            <span>{queue.length} due</span>
          </div>
          <p className="automation-card-intro">Tasks only appear here when their recurrence matches your local desktop date. Opening the app does not execute them.</p>
          {queue.length ? (
            <div className="automation-queue-list">
              {queue.map((occurrence) => <QueueItem key={occurrence.occurrence_id} occurrence={occurrence} busy={busy} running={runningOccurrenceId === occurrence.occurrence_id} onRun={runOccurrence} onSkip={skipOccurrence} onNavigate={onNavigate} />)}
            </div>
          ) : <EmptyState icon="fa-mug-hot" text="Your queue is clear for today. Create an automation or return on its scheduled day." />}
        </article>

        <aside className="automation-safety-card">
          <p className="eyebrow">What automation means here</p>
          <h2>Prepare, inspect, hand off.</h2>
          <ol>
            <li><span>1</span><strong>Schedule a small, allow-listed step.</strong></li>
            <li><span>2</span><strong>Review it in this queue when you open EduMind.</strong></li>
            <li><span>3</span><strong>Confirm each run and review its compact audit result.</strong></li>
          </ol>
          <p><i className="fa-solid fa-user-shield" aria-hidden="true" /> The task page never bypasses the Routine Manager’s conflict check, Student OS ownership, research source selection, or agent tool allow-lists.</p>
        </aside>
      </section>

      <section className="automation-builder-grid">
        <article className="automation-card automation-template-card">
          <div className="automation-card-heading">
            <div>
              <p className="eyebrow">Start faster</p>
              <h2>Safe templates</h2>
            </div>
          </div>
          <div className="automation-template-list">
            {AUTOMATION_TEMPLATES.map((template) => (
              <button key={template.id} type="button" className="automation-template-button" onClick={() => useTemplate(template)} disabled={busy}>
                <span><i className={`fa-solid ${getAutomationAction(template.action_id)?.icon ?? "fa-bolt"}`} aria-hidden="true" /></span>
                <span><strong>{template.title}</strong><small>{template.description}</small></span>
                <i className="fa-solid fa-arrow-right" aria-hidden="true" />
              </button>
            ))}
          </div>
          <p className="automation-template-note">Templates only prefill the form. You review the action, recurrence, and project before saving anything.</p>
        </article>

        <form className="automation-card automation-builder-form" aria-label="Create task automation" onSubmit={(event) => void saveAutomation(event)}>
          <div className="automation-card-heading">
            <div>
              <p className="eyebrow">Your automation</p>
              <h2>Create a reviewable task</h2>
            </div>
          </div>
          <div className="automation-form-grid">
            <label className="automation-form-wide">
              <span>Automation name</span>
              <input value={draft.title} onChange={(event) => changeDraft("title", event.target.value)} maxLength="120" placeholder="Daily study reset" disabled={busy} required />
            </label>
            <label className="automation-form-wide">
              <span>Why this helps (optional)</span>
              <textarea value={draft.description} onChange={(event) => changeDraft("description", event.target.value)} maxLength="480" rows="2" placeholder="Protect time to see the next study priority." disabled={busy} />
            </label>
            <label className="automation-form-wide">
              <span>Automation action</span>
              <select value={draft.action_id} onChange={(event) => changeAction(event.target.value)} disabled={busy}>
                {actionGroups(AUTOMATION_ACTIONS).map(([category, actions]) => (
                  <optgroup key={category} label={category}>
                    {actions.map((action) => <option key={action.id} value={action.id}>{action.label}</option>)}
                  </optgroup>
                ))}
              </select>
              <small className="automation-action-description"><i className={`fa-solid ${selectedAction.icon}`} aria-hidden="true" /> {selectedAction.description}</small>
            </label>
            {selectedAction.requiresProject ? (
              <label className="automation-form-wide">
                <span>Research project</span>
                <select value={draft.research_project_id} onChange={(event) => changeDraft("research_project_id", event.target.value)} disabled={busy} required>
                  <option value="">Choose a project to inspect</option>
                  {projects.map((project) => <option key={project.id} value={project.id}>{project.topic ?? project.title ?? project.id}</option>)}
                </select>
                <small>Only read-only supervision and gap checks are available here. Source ingestion and downloads remain in Research.</small>
              </label>
            ) : null}
            <label>
              <span>Repeat</span>
              <select value={draft.cadence} onChange={(event) => changeDraft("cadence", event.target.value)} disabled={busy}>
                <option value="once">One time</option>
                <option value="daily">Every day</option>
                <option value="weekly">Weekly on selected days</option>
                <option value="interval">Every N days</option>
              </select>
            </label>
            <label>
              <span>Starts on</span>
              <input type="date" value={draft.start_date} onChange={(event) => changeDraft("start_date", event.target.value)} disabled={busy} />
            </label>
            <label>
              <span>Reminder time (optional)</span>
              <input type="time" value={draft.reminder_time} onChange={(event) => changeDraft("reminder_time", event.target.value)} disabled={busy} />
            </label>
            {draft.cadence === "interval" ? (
              <label>
                <span>Every how many days</span>
                <input type="number" min="2" max="31" value={draft.interval_days} onChange={(event) => changeDraft("interval_days", event.target.value)} disabled={busy} />
              </label>
            ) : null}
            {draft.cadence === "weekly" ? (
              <fieldset className="automation-weekdays automation-form-wide">
                <legend>Repeat on</legend>
                <div>
                  {WEEKDAYS.map((weekday) => (
                    <label key={weekday.id}>
                      <input type="checkbox" checked={draft.weekdays.includes(weekday.id)} onChange={() => toggleWeekday(weekday.id)} disabled={busy} />
                      <span>{weekday.label}</span>
                    </label>
                  ))}
                </div>
              </fieldset>
            ) : null}
          </div>
          <div className="automation-builder-actions">
            <button type="submit" disabled={busy}>Create automation</button>
            <button type="button" className="secondary-button" onClick={() => setDraft(defaultDraft(today))} disabled={busy}>Clear form</button>
            <small><i className="fa-solid fa-clock" aria-hidden="true" /> Reminder times label the queue only; EduMind does not run tasks when closed.</small>
          </div>
        </form>
      </section>

      <section className="automation-bottom-grid">
        <article className="automation-card automation-definitions-card">
          <div className="automation-card-heading">
            <div>
              <p className="eyebrow">Manage</p>
              <h2>Saved automations</h2>
            </div>
            <span>{definitionState.definitions.length}</span>
          </div>
          {definitionState.definitions.length ? (
            <div className="automation-definition-list">
              {definitionState.definitions.map((definition) => {
                const action = getAutomationAction(definition.action_id);
                return (
                  <article key={definition.id} className={!definition.enabled ? "is-paused" : ""}>
                    <span className="automation-definition-icon"><i className={`fa-solid ${action?.icon ?? "fa-bolt"}`} aria-hidden="true" /></span>
                    <div>
                      <h3>{definition.title}</h3>
                      <p>{action?.label ?? "Protected action"} · {formatAutomationSchedule(definition)}</p>
                      {definition.description ? <small>{definition.description}</small> : null}
                    </div>
                    <div className="automation-definition-actions">
                      <button type="button" className="text-button" onClick={() => void toggleDefinition(definition)} disabled={busy}>{definition.enabled ? "Pause" : "Resume"}</button>
                      <button type="button" className="text-button danger-text" onClick={() => void removeDefinition(definition)} disabled={busy}>Remove</button>
                    </div>
                  </article>
                );
              })}
            </div>
          ) : <EmptyState icon="fa-seedling" text="No automation definitions yet. Use a safe template or make a task that fits your own study rhythm." />}
        </article>

        <article className="automation-card automation-audit-card">
          <div className="automation-card-heading">
            <div>
              <p className="eyebrow">Audit</p>
              <h2>Recent runs</h2>
            </div>
            <span>{recentExecutions.length}</span>
          </div>
          {recentExecutions.length ? (
            <ol className="automation-audit-list">
              {recentExecutions.map((execution) => (
                <li key={execution.occurrence_id}>
                  <span className={`automation-status-dot status-${execution.status}`} aria-hidden="true" />
                  <div>
                    <strong>{execution.title}</strong>
                    <small>{statusLabel(execution.status)} · {formatAuditTime(execution.updated_at)}</small>
                    <p>{execution.summary || execution.logs.at(-1)?.message || "Awaiting a compact audit summary."}</p>
                  </div>
                </li>
              ))}
            </ol>
          ) : <EmptyState icon="fa-clipboard-list" text="When you run or skip a task, EduMind saves a compact local audit entry here—never a raw transcript." />}
        </article>
      </section>
    </section>
  );
}

function QueueItem({ occurrence, busy, running, onRun, onSkip, onNavigate }) {
  const action = occurrence.action;
  const canRun = ["ready", "blocked", "needs-review"].includes(occurrence.status);
  const isComplete = occurrence.status === "completed";
  const isSkipped = ["skipped", "cancelled"].includes(occurrence.status);
  const runLabel = occurrence.status === "blocked" ? "Retry safe step" : "Run safe step";

  return (
    <article className={`automation-queue-item status-${occurrence.status}`}>
      <span className="automation-queue-icon"><i className={`fa-solid ${action?.icon ?? "fa-bolt"}`} aria-hidden="true" /></span>
      <div className="automation-queue-copy">
        <div>
          <span className={`automation-state-pill status-${occurrence.status}`}>{statusLabel(occurrence.status)}</span>
          {occurrence.definition.reminder_time ? <span className="automation-time-pill"><i className="fa-regular fa-clock" aria-hidden="true" /> {occurrence.definition.reminder_time}</span> : null}
        </div>
        <h3>{occurrence.definition.title}</h3>
        <p>{action?.label ?? "Protected action"}</p>
        {occurrence.definition.description ? <small>{occurrence.definition.description}</small> : null}
        {occurrence.execution?.summary ? <p className="automation-execution-summary">{occurrence.execution.summary}</p> : null}
        {occurrence.execution?.logs?.length ? (
          <details className="automation-log-details">
            <summary>Compact audit trail</summary>
            <ul>{occurrence.execution.logs.map((entry, index) => <li key={`${entry.at}-${index}`}><time>{formatAuditTime(entry.at)}</time>{entry.message}</li>)}</ul>
          </details>
        ) : null}
      </div>
      <div className="automation-queue-actions">
        {canRun ? <button type="button" onClick={() => void onRun(occurrence)} disabled={busy}>{running ? "Running…" : runLabel}</button> : null}
        {!isComplete && !isSkipped && !running ? <button type="button" className="secondary-button" onClick={() => void onSkip(occurrence)} disabled={busy}>Skip today</button> : null}
        {isComplete || isSkipped || occurrence.status === "blocked" ? <button type="button" className="text-button" onClick={() => onNavigate(action?.route ?? "home")}>Open workspace</button> : null}
      </div>
    </article>
  );
}

function Metric({ icon, label, value, detail, tone }) {
  return (
    <article className={`automation-metric-card tone-${tone}`}>
      <i className={`fa-solid ${icon}`} aria-hidden="true" />
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{detail}</small>
    </article>
  );
}

function EmptyState({ icon, text }) {
  return <div className="automation-empty"><i className={`fa-solid ${icon}`} aria-hidden="true" /><p>{text}</p></div>;
}

function actionGroups(actions) {
  const groups = new Map();
  for (const action of actions) {
    const existing = groups.get(action.category) ?? [];
    existing.push(action);
    groups.set(action.category, existing);
  }
  return [...groups.entries()];
}

async function executeGatewayAction(action, client, projectId) {
  if (action.execution === "refresh-study-recommendations") {
    return client.refreshStudyRecommendations();
  }
  if (action.execution === "review-study-insights") {
    return client.studyInsights();
  }
  if (action.execution === "review-planner") {
    return client.plannerSchedule();
  }
  if (action.execution === "research-supervision") {
    return client.researchSupervision(projectId);
  }
  if (action.execution === "research-gaps") {
    return client.researchGaps(projectId);
  }
  throw new Error("This action is not available for task automation.");
}

function activeRecordValue(records, key) {
  return records.find((record) => record?.key === key && !record?.deleted)?.value;
}

function projectList(response) {
  const projects = Array.isArray(response) ? response : response?.projects;
  return Array.isArray(projects)
    ? projects
      .filter((project) => project && typeof project === "object" && typeof project.id === "string")
      .slice(0, 50)
    : [];
}

function safeMessage(reason, fallback) {
  const message = typeof reason?.message === "string" ? reason.message.trim() : "";
  return message ? message.slice(0, 300) : fallback;
}

function statusLabel(status) {
  const labels = {
    ready: "Ready",
    running: "Running",
    completed: "Completed",
    "needs-review": "Needs review",
    blocked: "Blocked",
    skipped: "Skipped",
    cancelled: "Cancelled"
  };
  return labels[status] ?? "Needs review";
}

function formatAuditTime(value) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return "Just now";
  }
  return new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit"
  }).format(date);
}
