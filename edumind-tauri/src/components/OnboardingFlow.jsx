import { useEffect, useState } from "react";

const ONBOARDING_RECORD_KEY = "desktop-onboarding";
const WORKSPACES = [
  { id: "study", label: "Study Review", detail: "Start with due cards and deterministic recommendations." },
  { id: "research", label: "Research", detail: "Start a source-grounded research loop." },
  { id: "routine", label: "Planner", detail: "Review schedule conflicts before changing a routine." }
];

export const OFFLINE_DEMO_DATA = Object.freeze([
  { concept: "Calculus limits", risk: "high", action: "Review one 20-minute source-linked card" },
  { concept: "Research methods", risk: "medium", action: "Inspect two cited evidence spans" },
  { concept: "Weekly planning", risk: "low", action: "Resolve one overlapping focus block" }
]);

const DEFAULT_SETUP = Object.freeze({
  version: 1,
  completed: false,
  workspace: "study",
  keep_local_data: true,
  integration_mode: "disabled"
});

export function OnboardingFlow({ client, connectionState, onNavigate }) {
  const [setup, setSetup] = useState(() => ({ ...DEFAULT_SETUP }));
  const [runtime, setRuntime] = useState(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [editing, setEditing] = useState(true);
  const [dismissed, setDismissed] = useState(false);
  const [showDemo, setShowDemo] = useState(false);
  const [status, setStatus] = useState("Checking local setup…");

  useEffect(() => {
    let active = true;

    async function load() {
      if (!client) {
        if (active) {
          setRuntime(null);
          setLoading(false);
          setEditing(true);
          setStatus("Offline preview keeps setup choices in this session only.");
        }
        return;
      }

      try {
        const [nextRuntime, snapshot] = await Promise.all([
          client.runtimeStatus(),
          client.studentPage("student-os")
        ]);
        if (!active) {
          return;
        }
        const record = (snapshot.records ?? []).find((entry) => {
          return entry.key === ONBOARDING_RECORD_KEY && !entry.deleted;
        });
        const nextSetup = normalizeSetup(record?.value);
        setRuntime(nextRuntime);
        setSetup(nextSetup);
        setEditing(!nextSetup.completed);
        setStatus(nextSetup.completed
          ? "Your local setup is saved through the gateway."
          : "Choose the local workspace and privacy defaults that fit this study session.");
      } catch (error) {
        if (active) {
          setRuntime(null);
          setEditing(true);
          setStatus(`Local setup could not be loaded: ${error.message}`);
        }
      } finally {
        if (active) {
          setLoading(false);
        }
      }
    }

    void load();
    return () => {
      active = false;
    };
  }, [client]);

  if (dismissed) {
    return null;
  }

  if (!loading && setup.completed && !editing) {
    return (
      <section className="onboarding-panel onboarding-complete" aria-labelledby="onboarding-heading">
        <div>
          <p className="eyebrow">Getting started</p>
          <h2 id="onboarding-heading">Local setup complete</h2>
          <p className="muted">Your choices stay in canonical Student OS state and can be reviewed before they change.</p>
        </div>
        <button type="button" className="secondary-button" onClick={() => setEditing(true)}>Review setup</button>
      </section>
    );
  }

  const online = connectionState === "connected";
  const localModelLabel = runtime
    ? runtime.local_model_configured
      ? "A local model is configured for optional enabled workflows."
      : "No local model is configured; deterministic study tools remain available."
    : online
      ? "Checking configured local model availability…"
      : "Offline preview uses no model and sends no study data anywhere.";

  async function saveAndContinue() {
    if (!setup.keep_local_data) {
      setStatus("Confirm the local-only privacy preference before saving setup choices.");
      return;
    }
    const nextSetup = {
      ...setup,
      version: 1,
      completed: true,
      completed_at: new Date().toISOString()
    };

    if (!client) {
      setSetup(nextSetup);
      setStatus("Offline demo choices are active for this session and were not persisted.");
      onNavigate(nextSetup.workspace);
      return;
    }

    setSaving(true);
    try {
      await client.upsertStudentPageRecord("student-os", ONBOARDING_RECORD_KEY, nextSetup);
      setSetup(nextSetup);
      setEditing(false);
      setStatus("Local setup saved. No integrations were enabled automatically.");
      onNavigate(nextSetup.workspace);
    } catch (error) {
      setStatus(`Local setup could not be saved: ${error.message}`);
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="onboarding-panel" aria-labelledby="onboarding-heading">
      <div>
        <p className="eyebrow">Getting started</p>
        <h2 id="onboarding-heading">{online ? "Set up your local learning loop." : "Explore safely in offline preview."}</h2>
        <p className="muted">{localModelLabel}</p>
      </div>
      <p className="muted" aria-live="polite">{status}</p>
      <div className="onboarding-grid">
        <article>
          <h3>Choose a first workspace</h3>
          <div className="onboarding-options">
            {WORKSPACES.map((workspace) => (
              <label key={workspace.id}>
                <input
                  type="radio"
                  name="onboarding-workspace"
                  value={workspace.id}
                  checked={setup.workspace === workspace.id}
                  onChange={() => setSetup((current) => ({ ...current, workspace: workspace.id }))}
                />
                <span>
                  <strong>{workspace.label}</strong>
                  <small>{workspace.detail}</small>
                </span>
              </label>
            ))}
          </div>
        </article>
        <article>
          <h3>Privacy and optional integrations</h3>
          <label className="onboarding-toggle">
            <input
              type="checkbox"
              checked={setup.keep_local_data}
              onChange={(event) => setSetup((current) => ({
                ...current,
                keep_local_data: event.target.checked
              }))}
            />
            <span>Keep study preferences local</span>
          </label>
          <label className="onboarding-select">
            <span>Optional integrations</span>
            <select
              value={setup.integration_mode}
              onChange={(event) => setSetup((current) => ({
                ...current,
                integration_mode: event.target.value
              }))}
            >
              <option value="disabled">Keep integrations disabled</option>
              <option value="review">Review integrations in Admin first</option>
            </select>
          </label>
          <p className="muted">Changing this preference never enables an external service by itself.</p>
        </article>
      </div>
      <div className="onboarding-actions">
        <button type="button" className="secondary-button" onClick={() => setShowDemo((current) => !current)}>
          {showDemo ? "Hide offline demo" : "Preview deterministic offline demo"}
        </button>
        <button type="button" onClick={() => void saveAndContinue()} disabled={saving || !setup.keep_local_data}>
          {saving ? "Saving local setup…" : "Save local setup and continue"}
        </button>
        <button type="button" className="text-button" onClick={() => setDismissed(true)}>Hide for this session</button>
      </div>
      {showDemo && (
        <section className="onboarding-demo" aria-labelledby="offline-demo-heading">
          <h3 id="offline-demo-heading">Deterministic offline demo dataset</h3>
          <ul>
            {OFFLINE_DEMO_DATA.map((item) => (
              <li key={item.concept}>
                <strong>{item.concept}</strong>
                <span>{item.risk} risk · {item.action}</span>
              </li>
            ))}
          </ul>
        </section>
      )}
    </section>
  );
}

function normalizeSetup(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return { ...DEFAULT_SETUP };
  }
  const workspace = WORKSPACES.some((item) => item.id === value.workspace)
    ? value.workspace
    : DEFAULT_SETUP.workspace;
  return {
    version: 1,
    completed: value.completed === true,
    workspace,
    keep_local_data: value.keep_local_data !== false,
    integration_mode: value.integration_mode === "review" ? "review" : "disabled"
  };
}
