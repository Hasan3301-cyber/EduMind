import { useState } from "react";

import { useRunTimeline } from "../hooks/useRunTimeline";

export function RunTimelinePanel({ client, runId }) {
  const { loading, timeline, evidence, error, connection, refresh } = useRunTimeline(client, runId);
  const [confirmingCancel, setConfirmingCancel] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [cancelError, setCancelError] = useState(null);

  if (!runId) {
    return null;
  }

  const cancelled = timeline.some((event) => event.event_type === "run_cancelled");
  const budget = evidence?.budget;

  async function cancelRun() {
    if (!client) {
      return;
    }
    setCancelling(true);
    setCancelError(null);
    try {
      await client.cancelRun(runId);
      setConfirmingCancel(false);
      await refresh();
    } catch (reason) {
      setCancelError(reason.message);
    } finally {
      setCancelling(false);
    }
  }

  return (
    <section className="run-timeline-panel" aria-labelledby="run-timeline-heading">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Run evidence</p>
          <h2 id="run-timeline-heading">Recoverable run timeline</h2>
          <p className="muted">Run {runId} · event channel {connection}</p>
          {cancelled && <p className="muted">Cancellation is persisted and prevents later stages from starting.</p>}
        </div>
        <div className="timeline-actions">
          <button type="button" className="secondary-button" onClick={() => void refresh()} disabled={loading}>
            Refresh
          </button>
          <button
            type="button"
            className="danger-button"
            onClick={() => setConfirmingCancel(true)}
            disabled={cancelling || cancelled}
          >
            {cancelled ? "Cancellation requested" : "Cancel run"}
          </button>
        </div>
      </div>
      {confirmingCancel && (
        <div className="run-cancel-confirmation" role="alert">
          <p>Cancel this run before its next stage begins?</p>
          <button type="button" className="danger-button" onClick={() => void cancelRun()} disabled={cancelling}>
            {cancelling ? "Cancelling…" : "Confirm cancellation"}
          </button>
          <button type="button" className="secondary-button" onClick={() => setConfirmingCancel(false)} disabled={cancelling}>
            Keep running
          </button>
        </div>
      )}
      {error && <p className="error-message">{error.message}</p>}
      {cancelError && <p className="error-message">{cancelError}</p>}
      <div className="timeline-grid">
        <article>
          <h3>Timeline</h3>
          {timeline.length ? (
            <ol className="timeline-list">
              {timeline.map((event) => (
                <li key={event.id ?? `${event.event_type}-${event.at}`}>
                  <strong>{event.event_type}</strong>
                  <span>{event.message}</span>
                  <small>{formatTime(event.at ?? event.created_at)}</small>
                </li>
              ))}
            </ol>
          ) : (
            <p className="muted">{loading ? "Loading run events…" : "No persisted events yet."}</p>
          )}
        </article>
        <article>
          <h3>Budget use</h3>
          {budget ? (
            <dl className="run-budget">
              <div><dt>Tool calls</dt><dd>{budget.tool_calls_used} / {formatLimit(budget.max_tool_calls)}</dd></div>
              <div><dt>Output</dt><dd>{budget.output_bytes_used} B / {formatLimit(budget.max_output_bytes)} B</dd></div>
              <div><dt>Elapsed</dt><dd>{budget.elapsed_secs_used} s / {formatLimit(budget.max_elapsed_secs)} s</dd></div>
            </dl>
          ) : <p className="muted">No persisted budget has been initialized for this run.</p>}
        </article>
        <article>
          <h3>Verification evidence</h3>
          <p className="muted">{evidence?.checkpoints?.length ?? 0} checkpoints · {evidence?.verifications?.length ?? 0} verifications</p>
          <ul className="verification-list">
            {(evidence?.verifications ?? []).map((verification) => (
              <li key={verification.id}>
                <strong>{verification.passed ? "Passed" : "Failed"} · {verification.stage}</strong>
                <span>{verification.summary}</span>
              </li>
            ))}
          </ul>
        </article>
      </div>
    </section>
  );
}

function formatLimit(value) {
  return value ?? "unbounded";
}

function formatTime(value) {
  if (!value) {
    return "Time unavailable";
  }
  const parsed = new Date(value);
  return Number.isNaN(parsed.valueOf()) ? String(value) : parsed.toLocaleString();
}
