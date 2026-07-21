import { useCallback, useEffect, useState } from "react";

export function StudyInsightsPanel({ client }) {
  const [insights, setInsights] = useState(null);
  const [status, setStatus] = useState("Loading local study insights…");
  const [busy, setBusy] = useState(false);
  const [acceptingId, setAcceptingId] = useState(null);
  const [acceptedConcept, setAcceptedConcept] = useState(null);

  const load = useCallback(async () => {
    if (!client) {
      setInsights(null);
      setStatus("Launch the desktop app to load local study insights.");
      return null;
    }
    try {
      const next = await client.studyInsights();
      setInsights(next);
      setStatus(next.recommendations?.length ? "Recommendations are ready." : "Refresh insights after adding study material.");
      return next;
    } catch (error) {
      setStatus(error.message);
      return null;
    }
  }, [client]);

  useEffect(() => {
    void load();
  }, [load]);

  async function refresh() {
    if (!client) {
      return;
    }
    setBusy(true);
    try {
      const next = await client.refreshStudyRecommendations();
      setInsights(next);
      setStatus("Recommendations refreshed from your local SRS, planner, and memory.");
    } catch (error) {
      setStatus(error.message);
    } finally {
      setBusy(false);
    }
  }

  async function acceptRecommendation(recommendation) {
    setAcceptingId(recommendation.concept_id);
    try {
      if (client) {
        await client.upsertStudentPageRecord("student-os", "next-focus-recommendation", {
          concept_id: recommendation.concept_id,
          recommended_minutes: recommendation.recommended_minutes,
          accepted_at: new Date().toISOString()
        });
      }
      setAcceptedConcept(recommendation.concept_id);
      setStatus(client
        ? "Accepted as the next local focus. No planner block was changed."
        : "Accepted for this offline preview session. No planner block was changed.");
    } catch (error) {
      setStatus(`Recommendation could not be accepted: ${error.message}`);
    } finally {
      setAcceptingId(null);
    }
  }

  const recommendations = insights?.recommendations ?? [];
  const mastery = insights?.mastery ?? [];
  const plannerConflicts = insights?.planner_conflicts ?? [];

  return (
    <section className="study-insights-panel" aria-labelledby="study-insights-heading">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Learning intelligence</p>
          <h2 id="study-insights-heading">Next best study actions</h2>
          <p className="muted">Offline and deterministic: no model call is needed.</p>
        </div>
        <button type="button" className="secondary-button" onClick={() => void refresh()} disabled={busy || !client}>
          {busy ? "Refreshing…" : "Refresh insights"}
        </button>
      </div>
      <p className="muted" aria-live="polite">{status}</p>
      {insights && (
        <>
          <div className="insight-metrics">
            <span>{insights.available_minutes ?? 0} suggested minutes</span>
            <span>{insights.module_memory_records ?? 0} module memory records considered</span>
            <span>{mastery.length} concepts scored</span>
            <span>{plannerConflicts.length} planner conflicts</span>
          </div>
          <section className="insight-text-alternative" aria-labelledby="insight-summary-heading">
            <h3 id="insight-summary-heading">Accessible study summary</h3>
            <p>
              {recommendations.length
                ? `${recommendations.length} ranked actions are available. ${recommendations[0].concept_id} is the highest-priority focus.`
                : "No ranked study actions are available yet."}
            </p>
          </section>
          <div className="recommendation-grid">
            {recommendations.slice(0, 6).map((recommendation) => (
              <article key={recommendation.concept_id} className={`recommendation-card risk-${recommendation.retention_risk}`}>
                <p className="eyebrow">{recommendation.retention_risk} risk · priority {recommendation.priority_score}</p>
                <h3>{recommendation.concept_id}</h3>
                <p>{recommendation.rationale}</p>
                <strong>{recommendation.recommended_minutes} minute focus block</strong>
                <button
                  type="button"
                  className="secondary-button"
                  onClick={() => void acceptRecommendation(recommendation)}
                  disabled={acceptingId === recommendation.concept_id}
                >
                  {acceptingId === recommendation.concept_id
                    ? "Accepting…"
                    : acceptedConcept === recommendation.concept_id
                      ? "Accepted as next focus"
                      : "Accept focus block"}
                </button>
              </article>
            ))}
          </div>
          <section className="mastery-summary" aria-labelledby="mastery-summary-heading">
            <h3 id="mastery-summary-heading">Mastery and retention risk</h3>
            {mastery.length ? (
              <ul>
                {mastery.slice(0, 6).map((snapshot) => (
                  <li key={snapshot.concept_id}>
                    <strong>{snapshot.concept_id}</strong>
                    <span>{snapshot.mastery_percent}% mastery · {snapshot.retention_risk} risk · {snapshot.days_since_review} days since review</span>
                  </li>
                ))}
              </ul>
            ) : <p className="muted">Refresh after reviewing cards to calculate mastery.</p>}
          </section>
          <section className="planner-conflicts" aria-labelledby="planner-conflicts-heading">
            <h3 id="planner-conflicts-heading">Planner conflicts</h3>
            {plannerConflicts.length ? (
              <ul>
                {plannerConflicts.map((conflict) => (
                  <li key={`${conflict.day}-${conflict.first_entry_id}-${conflict.second_entry_id}`}>
                    <strong>{conflict.day}</strong>
                    <span>{conflict.first_title} overlaps {conflict.second_title}</span>
                  </li>
                ))}
              </ul>
            ) : <p className="muted">No overlapping planner blocks were detected.</p>}
          </section>
        </>
      )}
    </section>
  );
}
