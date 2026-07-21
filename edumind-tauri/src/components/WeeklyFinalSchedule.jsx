import { useMemo } from "react";

import {
  canonicalWeeklySchedule,
  formatScheduleDay,
  formatScheduleTime,
  formatScheduleWeek,
  hasCanonicalSchedule,
  isSameLocalDate,
  scheduleEntryCount
} from "../services/weekly-schedule";
import "./WeeklyFinalSchedule.css";

export function WeeklyFinalSchedule({ schedule, isLoading, onNavigate }) {
  const scheduleDays = useMemo(() => canonicalWeeklySchedule(schedule), [schedule]);
  const scheduleReady = hasCanonicalSchedule(schedule);
  const blockCount = scheduleEntryCount(scheduleDays);
  const today = new Date();

  return (
    <section className="dashboard-card dashboard-final-schedule" aria-labelledby="final-weekly-schedule-title">
      <div className="dashboard-card-heading dashboard-final-schedule-heading">
        <div>
          <p className="eyebrow">Canonical planner</p>
          <h2 id="final-weekly-schedule-title">Final weekly schedule</h2>
          <p>Only confirmed Planner blocks appear here. Routine drafts stay out until you explicitly apply them.</p>
        </div>
        <button type="button" className="text-button" onClick={() => onNavigate("planner")}>Open Planner</button>
      </div>

      {scheduleReady ? (
        <>
          <div className="dashboard-final-schedule-meta" aria-label="Final weekly schedule status">
            <span><i className="fa-solid fa-calendar-check" aria-hidden="true" /> {blockCount} confirmed {blockCount === 1 ? "block" : "blocks"}</span>
            <span>{formatScheduleWeek(scheduleDays)}</span>
          </div>
          <div className="dashboard-final-week-scroll">
            <div className="dashboard-final-week-grid">
              {scheduleDays.map((scheduleDay) => (
                <article className={`dashboard-final-week-day ${isSameLocalDate(scheduleDay.date, today) ? "is-today" : ""}`} key={scheduleDay.day}>
                  <header>
                    <div>
                      <h3>{scheduleDay.day}</h3>
                      <span>{formatScheduleDay(scheduleDay.date)}</span>
                    </div>
                    <strong aria-label={`${scheduleDay.entries.length} scheduled blocks`}>{scheduleDay.entries.length}</strong>
                  </header>
                  <div className="dashboard-final-day-events">
                    {scheduleDay.entries.map((entry) => <FinalScheduleEvent key={`${scheduleDay.day}-${entry.id}`} entry={entry} onNavigate={onNavigate} />)}
                    {!scheduleDay.entries.length ? <div className="dashboard-final-day-empty">Open</div> : null}
                  </div>
                </article>
              ))}
            </div>
          </div>
        </>
      ) : (
        <div className="dashboard-final-schedule-unavailable" aria-live="polite">
          <i className="fa-regular fa-calendar" aria-hidden="true" />
          <div>
            <strong>{isLoading ? "Loading your final weekly schedule…" : "Your final weekly schedule is unavailable."}</strong>
            <p>{isLoading ? "Reading the canonical Planner state." : "Reconnect the local gateway, then refresh Home to load the confirmed Planner blocks."}</p>
          </div>
        </div>
      )}
    </section>
  );
}

function FinalScheduleEvent({ entry, onNavigate }) {
  const time = entry.end
    ? `${formatScheduleTime(entry.start)}–${formatScheduleTime(entry.end)}`
    : formatScheduleTime(entry.start);
  return (
    <button type="button" className="dashboard-final-schedule-event" aria-label={`Open ${entry.title} in Planner`} onClick={() => onNavigate("planner")}>
      <span>{time}</span>
      <strong>{entry.title}</strong>
      <small>{entry.source}</small>
    </button>
  );
}
