import { useCallback, useEffect, useMemo, useState } from "react";

import { OnboardingFlow } from "./OnboardingFlow";
import { WeeklyFinalSchedule } from "./WeeklyFinalSchedule";
import { isProjectNoteRecordKey } from "../services/project-notes";
import { isAutomationRecordKey } from "../services/task-automation";
import { isWellnessRecordKey } from "../services/wellness-data";
import { formatScheduleTime } from "../services/weekly-schedule";

const FOCUS_SESSIONS_KEY = "edumind:home-dashboard:focus-sessions:v1";
const TIMER_MODES = {
  focus: { label: "Focus", minutes: 25 },
  short: { label: "Short break", minutes: 5 },
  long: { label: "Long break", minutes: 15 }
};

const MODULES = [
  { id: "class-notes", title: "Class Notes", icon: "fa-note-sticky", detail: "Turn lectures and slides into source-aware notes.", tone: "gold" },
  { id: "exam-practice", title: "Exam Practice", icon: "fa-pen-ruler", detail: "Build transparent practice from approved material.", tone: "coral" },
  { id: "study", title: "Study Review", icon: "fa-brain", detail: "Resolve due cards with focused recall.", tone: "mint" },
  { id: "group-study", title: "Group Study", icon: "fa-people-group", detail: "Continue focused rooms with classmates and an AI facilitator.", tone: "lilac" },
  { id: "planner", title: "Planner", icon: "fa-calendar-days", detail: "Keep classes and focus blocks canonical.", tone: "sage" },
  { id: "routine", title: "Routine Coach", icon: "fa-clock", detail: "Draft and manage AI routine blocks after review.", tone: "sky" },
  { id: "research", title: "Research", icon: "fa-flask", detail: "Build a grounded paper trail.", tone: "lilac" },
  { id: "student-os", title: "Student OS", icon: "fa-graduation-cap", detail: "Keep personal goals and operating cards in one place.", tone: "mint" },
  { id: "wellness", title: "Wellness", icon: "fa-heart-pulse", detail: "Track student-owned workouts, meals, and daily energy.", tone: "coral" },
  { id: "automation", title: "Task Automation", icon: "fa-list-check", detail: "Review confirmation-gated study workflows in one queue.", tone: "gold" },
  { id: "memory", title: "Memory Graph", icon: "fa-diagram-project", detail: "Explore source-linked concepts.", tone: "sky" }
];

const EMPTY_DASHBOARD = Object.freeze({
  insights: null,
  planner: null,
  projects: [],
  studentOs: null,
  agentStatus: null,
  runtimeStatus: null
});

export function ModulePanel({ onNavigate, client, connectionState }) {
  const [dashboard, setDashboard] = useState(EMPTY_DASHBOARD);
  const [status, setStatus] = useState("Loading your local study signals…");
  const [isLoading, setIsLoading] = useState(false);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [timerMode, setTimerMode] = useState("focus");
  const [timerSeconds, setTimerSeconds] = useState(TIMER_MODES.focus.minutes * 60);
  const [timerRunning, setTimerRunning] = useState(false);
  const [focusSessions, setFocusSessions] = useState(loadFocusSessions);

  const loadDashboard = useCallback(async (refreshRecommendations = false) => {
    if (!client) {
      setDashboard(EMPTY_DASHBOARD);
      setStatus("Offline preview is ready. Connect the desktop gateway to load your saved study signals.");
      return;
    }

    setIsLoading(true);
    const [
      insightsResult,
      plannerResult,
      projectsResult,
      studentOsResult,
      agentStatusResult,
      runtimeStatusResult
    ] = await Promise.allSettled([
      refreshRecommendations ? client.refreshStudyRecommendations() : client.studyInsights(),
      client.plannerSchedule(),
      client.listProjects(),
      client.studentPage("student-os"),
      client.agentStatus(),
      client.runtimeStatus()
    ]);
    const insights = insightsResult.status === "fulfilled" ? insightsResult.value : null;
    const planner = plannerResult.status === "fulfilled" ? plannerResult.value : null;
    const projects = projectsResult.status === "fulfilled" ? researchProjects(projectsResult.value) : [];
    const studentOs = studentOsResult.status === "fulfilled" ? studentOsResult.value : null;
    const agentStatus = agentStatusResult.status === "fulfilled" ? agentStatusResult.value : null;
    const runtimeStatus = runtimeStatusResult.status === "fulfilled" ? runtimeStatusResult.value : null;
    setDashboard({ insights, planner, projects, studentOs, agentStatus, runtimeStatus });
    if (insights) {
      setStatus(insights.recommendations?.length
        ? "Your local study priorities are ready."
        : "Add notes, reviews, or planner blocks to generate your next study priority.");
    } else if (planner) {
      setStatus("Your local planner is ready. Study recommendations will appear after the next refresh.");
    } else {
      setStatus("Your local study signals could not be loaded right now. Try refreshing from Home.");
    }
    setIsLoading(false);
  }, [client]);

  useEffect(() => {
    void loadDashboard();
  }, [loadDashboard]);

  useEffect(() => {
    if (!timerRunning || timerSeconds <= 0) {
      return undefined;
    }
    const timer = window.setInterval(() => {
      setTimerSeconds((current) => Math.max(0, current - 1));
    }, 1000);
    return () => window.clearInterval(timer);
  }, [timerRunning, timerSeconds]);

  useEffect(() => {
    if (!timerRunning || timerSeconds !== 0) {
      return;
    }
    setTimerRunning(false);
    if (timerMode === "focus") {
      setFocusSessions((current) => [...current, { completedAt: new Date().toISOString(), minutes: TIMER_MODES.focus.minutes }].slice(-120));
      setTimerMode("short");
      setTimerSeconds(TIMER_MODES.short.minutes * 60);
    }
  }, [timerMode, timerRunning, timerSeconds]);

  useEffect(() => {
    try {
      window.localStorage.setItem(FOCUS_SESSIONS_KEY, JSON.stringify(focusSessions));
    } catch {
      return;
    }
  }, [focusSessions]);

  const today = localDateKey();
  const dayName = new Intl.DateTimeFormat("en-US", { weekday: "long" }).format(new Date());
  const recommendations = dashboard.insights?.recommendations ?? [];
  const mastery = dashboard.insights?.mastery ?? [];
  const topRecommendation = recommendations[0] ?? null;
  const projects = dashboard.projects ?? [];
  const todayAgenda = useMemo(
    () => agendaForDay(dashboard.planner, dayName),
    [dashboard.planner, dayName]
  );
  const averageMastery = useMemo(() => averageMasteryPercent(mastery), [mastery]);
  const researchPaperCount = useMemo(() => countResearchPapers(projects), [projects]);
  const studentOsCardCount = useMemo(() => activeRecordCount(dashboard.studentOs), [dashboard.studentOs]);
  const enabledAgentCount = useMemo(() => countEnabledAgents(dashboard.agentStatus), [dashboard.agentStatus]);
  const localRuntimeReady = isLocalRuntimeReady(dashboard.runtimeStatus);
  const focusCount = focusSessions.filter((session) => localDateKey(new Date(session.completedAt)) === today).length;
  const totalTimerSeconds = TIMER_MODES[timerMode].minutes * 60;
  const timerProgress = Math.round((timerSeconds / totalTimerSeconds) * 100);
  const timerDisplay = formatTimer(timerSeconds);
  const connectionLabel = connectionState === "connected" ? "Local gateway connected" : "Offline-friendly preview";

  function selectTimerMode(nextMode) {
    setTimerRunning(false);
    setTimerMode(nextMode);
    setTimerSeconds(TIMER_MODES[nextMode].minutes * 60);
  }

  function resetTimer() {
    setTimerRunning(false);
    setTimerSeconds(TIMER_MODES[timerMode].minutes * 60);
  }

  async function refreshDashboard() {
    if (!client || isRefreshing) {
      return;
    }
    setIsRefreshing(true);
    try {
      await loadDashboard(true);
    } finally {
      setIsRefreshing(false);
    }
  }

  return (
    <section className="module-panel home-dashboard">
      <OnboardingFlow client={client} connectionState={connectionState} onNavigate={onNavigate} />

      <div className="home-dashboard-shell">
        <header className="home-dashboard-hero">
          <div className="home-dashboard-copy">
            <p className="eyebrow">{greeting()} · {dayName}</p>
            <h1>Your study dashboard, built for today.</h1>
            <p className="home-dashboard-subtitle">One calm place for your study system.</p>
            <p className="home-dashboard-description">See what needs attention, protect a focus block, and move into the right workspace without losing your learning context.</p>
            <div className="home-dashboard-actions">
              <button type="button" onClick={() => onNavigate(topRecommendation ? "study" : "class-notes")}>
                <i className={`fa-solid ${topRecommendation ? "fa-play" : "fa-note-sticky"}`} aria-hidden="true" />
                {topRecommendation ? "Start next focus" : "Create study signal"}
              </button>
              <button type="button" className="secondary-button" onClick={() => void refreshDashboard()} disabled={!client || isRefreshing}>
                <i className="fa-solid fa-rotate" aria-hidden="true" />
                {isRefreshing ? "Refreshing…" : "Refresh dashboard"}
              </button>
            </div>
          </div>
          <aside className="home-dashboard-hero-card" aria-label="Study loop status">
            <span className="home-dashboard-orbit" aria-hidden="true"><i className="fa-solid fa-sparkles" /></span>
            <div>
              <span>Learning loop</span>
              <strong>{connectionLabel}</strong>
              <small>{topRecommendation ? `${topRecommendation.recommended_minutes} focused minutes are ready.` : "Your next study signal will appear here."}</small>
            </div>
          </aside>
        </header>

        <p className="home-dashboard-status" aria-live="polite">{isLoading ? "Refreshing local study signals…" : status}</p>

        <section className="dashboard-metric-grid" aria-label="Today at a glance">
          <article className="dashboard-metric-card metric-mint">
            <i className="fa-solid fa-hourglass-half" aria-hidden="true" />
            <span>Suggested focus</span>
            <strong>{dashboard.insights?.available_minutes ?? 0}<small> min</small></strong>
            <p>From your local workload and planner.</p>
          </article>
          <article className="dashboard-metric-card metric-gold">
            <i className="fa-solid fa-bullseye" aria-hidden="true" />
            <span>Priority actions</span>
            <strong>{recommendations.length}</strong>
            <p>{recommendations.length ? "Ranked study actions are ready." : "Refresh after your next study activity."}</p>
          </article>
          <article className="dashboard-metric-card metric-lilac">
            <i className="fa-solid fa-chart-line" aria-hidden="true" />
            <span>Average mastery</span>
            <strong>{averageMastery === null ? "—" : `${averageMastery}%`}</strong>
            <p>{mastery.length ? `${mastery.length} concepts scored locally.` : "No concepts scored yet."}</p>
          </article>
          <article className="dashboard-metric-card metric-sky">
            <i className="fa-solid fa-book-bookmark" aria-hidden="true" />
            <span>Study evidence</span>
            <strong>{dashboard.insights?.module_memory_records ?? 0}</strong>
            <p>Saved records available to your workflows.</p>
          </article>
        </section>

        <section className="dashboard-main-grid">
          <article className="dashboard-card dashboard-next-focus">
            <div className="dashboard-card-heading">
              <div>
                <p className="eyebrow">Next best action</p>
                <h2>Today’s focus</h2>
              </div>
              <span className={`dashboard-risk-pill risk-${topRecommendation?.retention_risk ?? "ready"}`}>{topRecommendation?.retention_risk ?? "ready"}</span>
            </div>
            {topRecommendation ? (
              <>
                <h3>{topRecommendation.concept_id}</h3>
                <p>{topRecommendation.rationale}</p>
                <div className="dashboard-focus-meta">
                  <span><i className="fa-solid fa-clock" aria-hidden="true" /> {topRecommendation.recommended_minutes} minute block</span>
                  <span><i className="fa-solid fa-signal" aria-hidden="true" /> priority {topRecommendation.priority_score}</span>
                </div>
                <button type="button" onClick={() => onNavigate("study")}>Open Study Review <i className="fa-solid fa-arrow-right" aria-hidden="true" /></button>
              </>
            ) : (
              <div className="dashboard-empty-state">
                <span className="dashboard-empty-icon"><i className="fa-solid fa-seedling" aria-hidden="true" /></span>
                <h3>Build your first local study signal.</h3>
                <p>Save class notes or complete a review to unlock evidence-led recommendations.</p>
                <button type="button" onClick={() => onNavigate("class-notes")}>Open Class Notes <i className="fa-solid fa-arrow-right" aria-hidden="true" /></button>
              </div>
            )}
          </article>

          <article className="dashboard-card dashboard-timer-card">
            <div className="dashboard-card-heading">
              <div>
                <p className="eyebrow">Pomodoro</p>
                <h2>Study timer</h2>
              </div>
              <span className="dashboard-session-count">{focusCount} today</span>
            </div>
            <div className="dashboard-timer-modes" role="group" aria-label="Timer mode">
              {Object.entries(TIMER_MODES).map(([modeId, mode]) => (
                <button key={modeId} type="button" className={timerMode === modeId ? "is-active" : ""} aria-pressed={timerMode === modeId} onClick={() => selectTimerMode(modeId)}>{mode.label}</button>
              ))}
            </div>
            <div className="dashboard-timer-body">
              <div className="dashboard-timer-ring" style={{ "--dashboard-timer-progress": `${timerProgress}%` }}>
                <div>
                  <strong>{timerDisplay}</strong>
                  <span>{TIMER_MODES[timerMode].label}</span>
                </div>
              </div>
              <div className="dashboard-timer-actions">
                <button type="button" className="dashboard-timer-primary" aria-label={timerRunning ? "Pause timer" : "Start timer"} onClick={() => setTimerRunning((running) => !running)}>
                  <i className={`fa-solid ${timerRunning ? "fa-pause" : "fa-play"}`} aria-hidden="true" />
                </button>
                <button type="button" aria-label="Reset timer" onClick={resetTimer}><i className="fa-solid fa-rotate-left" aria-hidden="true" /></button>
                <button type="button" aria-label="Skip timer mode" onClick={() => selectTimerMode(timerMode === "focus" ? "short" : "focus")}><i className="fa-solid fa-forward-step" aria-hidden="true" /></button>
              </div>
            </div>
          </article>

          <article className="dashboard-card dashboard-agenda-card">
            <div className="dashboard-card-heading">
              <div>
                <p className="eyebrow">Canonical planner</p>
                <h2>Today’s agenda</h2>
              </div>
              <button type="button" className="text-button" onClick={() => onNavigate("planner")}>Open planner</button>
            </div>
            {todayAgenda.length ? (
              <ul className="dashboard-agenda-list">
                {todayAgenda.slice(0, 4).map((entry) => (
                  <li key={entry.id}>
                    <span className="dashboard-agenda-time">{formatScheduleTime(entry.start)}<small>{entry.end ? `–${formatScheduleTime(entry.end)}` : ""}</small></span>
                    <span><strong>{entry.title}</strong><small>{entry.source || "Planner"}</small></span>
                  </li>
                ))}
              </ul>
            ) : (
              <div className="dashboard-agenda-empty">
                <i className="fa-regular fa-calendar" aria-hidden="true" />
                <p>Your day is open for recovery or focused study.</p>
                <button type="button" className="secondary-button" onClick={() => onNavigate("planner")}>Add a planner block</button>
              </div>
            )}
          </article>
        </section>

        <WeeklyFinalSchedule schedule={dashboard.planner} isLoading={isLoading} onNavigate={onNavigate} />

        <section className="dashboard-operating-grid" aria-label="Student operating map">
          <article className="dashboard-card dashboard-operating-card">
            <div className="dashboard-card-heading">
              <div>
                <p className="eyebrow">Daily operating map</p>
                <h2>What needs your attention</h2>
              </div>
              <span className="dashboard-map-badge">Live local signals</span>
            </div>
            <ul className="dashboard-operating-list">
              <OperatingMapItem
                icon="fa-brain"
                tone="review"
                label="Review"
                summary={recommendations.length ? `${recommendations.length} priority ${recommendations.length === 1 ? "action" : "actions"} ready` : "No review priority yet"}
                detail={topRecommendation ? `${topRecommendation.concept_id} is the strongest next focus.` : "Complete a review or add class evidence to generate one."}
                action="Open Study Review"
                onClick={() => onNavigate("study")}
              />
              <OperatingMapItem
                icon="fa-calendar-day"
                tone="planner"
                label="Schedule"
                summary={todayAgenda.length ? `${todayAgenda.length} ${todayAgenda.length === 1 ? "block" : "blocks"} on today’s agenda` : "Your day is open"}
                detail={todayAgenda.length ? "Your Planner remains the canonical schedule." : "Protect a realistic focus block before the day fills up."}
                action="Open Planner"
                onClick={() => onNavigate("planner")}
              />
              <OperatingMapItem
                icon="fa-flask"
                tone="research"
                label="Research"
                summary={projects.length ? `${projects.length} research ${projects.length === 1 ? "project" : "projects"}` : "No research project yet"}
                detail={researchPaperCount ? `${researchPaperCount} discovered ${researchPaperCount === 1 ? "paper" : "papers"} ready to curate or index.` : "Start a focused question when you need source-grounded evidence."}
                action="Open Research"
                onClick={() => onNavigate("research")}
              />
              <OperatingMapItem
                icon="fa-graduation-cap"
                tone="student-os"
                label="Student OS"
                summary={studentOsCardCount ? `${studentOsCardCount} active personal ${studentOsCardCount === 1 ? "card" : "cards"}` : "Personal goals are ready for you"}
                detail={studentOsCardCount ? "Your personal operating cards stay separate from academic evidence." : "Add a goal or support that future routine drafts should respect."}
                action="Open Student OS"
                onClick={() => onNavigate("student-os")}
              />
            </ul>
          </article>

          <aside className="dashboard-card dashboard-system-card" aria-label="EduMind system readiness">
            <div className="dashboard-card-heading">
              <div>
                <p className="eyebrow">System readiness</p>
                <h2>Your local study team</h2>
              </div>
              <span className={`dashboard-system-state ${connectionState === "connected" ? "is-ready" : "is-preview"}`}>
                <i className={`fa-solid ${connectionState === "connected" ? "fa-circle-check" : "fa-cloud"}`} aria-hidden="true" />
                {connectionState === "connected" ? "Connected" : "Preview"}
              </span>
            </div>
            <dl className="dashboard-system-details">
              <div>
                <dt>Agents</dt>
                <dd>{enabledAgentCount || "—"}</dd>
                <small>{enabledAgentCount ? `${enabledAgentCount} enabled ${enabledAgentCount === 1 ? "agent" : "agents"} under the safe tool profile.` : "Agent status appears when the gateway is available."}</small>
              </div>
              <div>
                <dt>Runtime</dt>
                <dd>{localRuntimeReady ? "Ready" : connectionState === "connected" ? "Check" : "Preview"}</dd>
                <small>{localRuntimeReady ? "A local model runtime is configured." : "Configure an LLM in Admin when you want AI generation."}</small>
              </div>
            </dl>
            <p className="dashboard-system-note"><i className="fa-solid fa-shield-heart" aria-hidden="true" /> Planner edits, paper downloads, and agent actions still need your explicit review.</p>
            <button type="button" className="secondary-button dashboard-system-action" onClick={() => onNavigate("admin")}>Manage agent system <i className="fa-solid fa-arrow-right" aria-hidden="true" /></button>
          </aside>
        </section>

        <section className="dashboard-workspaces" aria-labelledby="dashboard-workspaces-heading">
          <div className="dashboard-section-heading">
            <div>
              <p className="eyebrow">Study spaces</p>
              <h2 id="dashboard-workspaces-heading">Continue where your learning needs you.</h2>
            </div>
            <span>{connectionState === "connected" ? "Local-first and ready" : "Available in offline preview"}</span>
          </div>
          <div className="dashboard-workspace-grid">
            {MODULES.map((module) => (
              <button type="button" className={`dashboard-workspace-card tone-${module.tone}`} key={module.id} onClick={() => onNavigate(module.id)}>
                <span className="dashboard-workspace-icon"><i className={`fa-solid ${module.icon}`} aria-hidden="true" /></span>
                <strong>{module.title}</strong>
                <span>{module.detail}</span>
                <small>Open workspace <i className="fa-solid fa-arrow-right" aria-hidden="true" /></small>
              </button>
            ))}
          </div>
        </section>
      </div>
    </section>
  );
}

function loadFocusSessions() {
  try {
    const stored = window.localStorage.getItem(FOCUS_SESSIONS_KEY);
    const sessions = stored ? JSON.parse(stored) : [];
    return Array.isArray(sessions) ? sessions.filter((session) => session?.completedAt) : [];
  } catch {
    return [];
  }
}

function greeting() {
  const hour = new Date().getHours();
  if (hour < 12) {
    return "Good morning";
  }
  if (hour < 18) {
    return "Good afternoon";
  }
  return "Good evening";
}

function agendaForDay(schedule, dayName) {
  const target = String(dayName).toLowerCase();
  const entries = schedule?.days?.find((day) => String(day?.day).toLowerCase() === target)?.entries ?? [];
  return [...entries].sort((left, right) => String(left.start ?? "").localeCompare(String(right.start ?? "")));
}

function researchProjects(response) {
  if (Array.isArray(response)) {
    return response;
  }
  return Array.isArray(response?.projects) ? response.projects : [];
}

function countResearchPapers(projects) {
  return projects.reduce((total, project) => total + (Array.isArray(project?.papers) ? project.papers.length : 0), 0);
}

function activeRecordCount(snapshot) {
  const records = Array.isArray(snapshot?.records) ? snapshot.records : [];
  return records.filter((record) =>
    !record?.deleted
    && !isWellnessRecordKey(record.key)
    && !isAutomationRecordKey(record.key)
    && !isProjectNoteRecordKey(record.key)
  ).length;
}

function countEnabledAgents(status) {
  const agents = Array.isArray(status?.agents) ? status.agents : [];
  return agents.filter((agent) => agent?.enabled !== false).length;
}

function isLocalRuntimeReady(status) {
  return Boolean(status?.local_model_configured ?? status?.localModelConfigured);
}

function OperatingMapItem({ icon, tone, label, summary, detail, action, onClick }) {
  return (
    <li>
      <span className={`dashboard-map-icon tone-${tone}`}><i className={`fa-solid ${icon}`} aria-hidden="true" /></span>
      <div>
        <span>{label}</span>
        <strong>{summary}</strong>
        <small>{detail}</small>
      </div>
      <button type="button" className="text-button" onClick={onClick}>{action}</button>
    </li>
  );
}

function averageMasteryPercent(mastery) {
  const values = mastery
    .map((entry) => Number(entry?.mastery_percent))
    .filter((value) => Number.isFinite(value));
  if (!values.length) {
    return null;
  }
  return Math.round(values.reduce((total, value) => total + value, 0) / values.length);
}

function formatTimer(seconds) {
  const minutes = String(Math.floor(seconds / 60)).padStart(2, "0");
  const remainder = String(seconds % 60).padStart(2, "0");
  return `${minutes}:${remainder}`;
}

function localDateKey(date = new Date()) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}
