import { useEffect, useState } from "react";

import { ProjectNotesPanel } from "./ProjectNotesPanel";
import { isAutomationRecordKey } from "../services/task-automation";
import { isProjectNoteRecordKey, projectNotesFromRecords } from "../services/project-notes";
import { isWellnessRecordKey } from "../services/wellness-data";

export function StudentOSPage({ client }) {
  const [records, setRecords] = useState([]);
  const [title, setTitle] = useState("");
  const [detail, setDetail] = useState("");
  const [editingKey, setEditingKey] = useState(null);
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState("Local cards are ready to edit.");
  const [activeSection, setActiveSection] = useState("overview");
  const projectNoteCount = projectNotesFromRecords(records).length;

  useEffect(() => {
    let active = true;
    if (!client) {
      return undefined;
    }
    client
      .studentPage("student-os")
      .then((snapshot) => active && setRecords(snapshot.records ?? []))
      .catch((error) => active && setStatus(error.message));
    return () => {
      active = false;
    };
  }, [client]);

  async function persist(next, successMessage) {
    const previous = records;
    setRecords(next);
    if (!client) {
      setStatus("Saved in this preview session. Launch the desktop app to sync the canonical page.");
      return true;
    }
    setSaving(true);
    try {
      const response = await client.saveStudentPage("student-os", next);
      setRecords(response.snapshot?.records ?? next);
      setStatus(successMessage);
      return true;
    } catch (error) {
      setRecords(previous);
      setStatus(error.message);
      return false;
    } finally {
      setSaving(false);
    }
  }

  async function saveCard(event) {
    event.preventDefault();
    if (!title.trim()) {
      setStatus("A card title is required.");
      return;
    }
    const key = editingKey ?? `card-${safeId()}`;
    const next = upsertCardRecord(records, key, {
      title: title.trim(),
      detail: detail.trim()
    });
    const saved = await persist(next, editingKey ? "Updated the canonical Student OS card." : "Added a canonical Student OS card.");
    if (saved) {
      resetEditor();
    }
  }

  function editCard(record) {
    setEditingKey(record.key);
    setTitle(String(record.value?.title ?? ""));
    setDetail(String(record.value?.detail ?? ""));
    setStatus(`Editing ${record.value?.title ?? "Student OS card"}.`);
  }

  async function removeCard(record) {
    const saved = await persist(markDeleted(records, record.key), "Removed the canonical Student OS card.");
    if (saved && editingKey === record.key) {
      resetEditor();
    }
  }

  function resetEditor() {
    setEditingKey(null);
    setTitle("");
    setDetail("");
  }

  return (
    <section className="student-page">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Student OS</p>
          <h1>Keep goals, project notes, supports, and routines in one canonical page.</h1>
          <div className="student-os-section-tabs" role="tablist" aria-label="Student OS sections">
            <button type="button" role="tab" aria-selected={activeSection === "overview"} aria-controls="student-os-overview" onClick={() => setActiveSection("overview")}>Overview</button>
            <button type="button" role="tab" aria-selected={activeSection === "projects"} aria-controls="student-os-projects" onClick={() => setActiveSection("projects")}>Projects &amp; notes <small>{projectNoteCount}</small></button>
          </div>
        </div>
      </div>
      {activeSection === "overview" ? (
        <section id="student-os-overview" role="tabpanel">
          <form className="card-editor" onSubmit={saveCard}>
            <label>
              <span>Card title</span>
              <input value={title} onChange={(event) => setTitle(event.target.value)} placeholder="Weekly priority" disabled={saving} />
            </label>
            <label>
              <span>Details</span>
              <input value={detail} onChange={(event) => setDetail(event.target.value)} placeholder="Protect two focused biology blocks" disabled={saving} />
            </label>
            <div className="editor-actions">
              <button type="submit" disabled={saving}>{editingKey ? "Update card" : "Add card"}</button>
              {editingKey ? (
                <button type="button" className="text-button" onClick={resetEditor} disabled={saving}>Cancel edit</button>
              ) : null}
            </div>
          </form>
          <p className="muted" aria-live="polite">{status}</p>
          <div className="student-card-grid">
            {visibleRecords(records).map((record) => (
              <article className="student-card" key={record.key}>
                <h2>{record.value?.title ?? record.key}</h2>
                <p>{record.value?.detail ?? "No details yet."}</p>
                <div className="student-card-actions">
                  <button type="button" className="text-button" onClick={() => editCard(record)} disabled={saving}>
                    Edit {record.value?.title ?? "card"}
                  </button>
                  <button type="button" className="text-button" onClick={() => void removeCard(record)} disabled={saving}>
                    Remove {record.value?.title ?? "card"}
                  </button>
                </div>
              </article>
            ))}
          </div>
        </section>
      ) : (
        <section id="student-os-projects" role="tabpanel">
          <ProjectNotesPanel records={records} onPersist={persist} saving={saving} />
        </section>
      )}
    </section>
  );
}

function visibleRecords(records) {
  return records.filter((record) => !record.deleted && !isWellnessRecordKey(record.key) && !isAutomationRecordKey(record.key) && !isProjectNoteRecordKey(record.key));
}

function upsertCardRecord(records, key, value, updatedAt = new Date().toISOString()) {
  const next = [...records];
  const index = next.findIndex((record) => record.key === key);
  const entry = { key, value, deleted: false, updated_at: updatedAt };
  if (index === -1) {
    next.push(entry);
  } else {
    next[index] = entry;
  }
  return next;
}

function markDeleted(records, key) {
  const updatedAt = new Date().toISOString();
  return records.map((record) =>
    record.key === key ? { ...record, deleted: true, updated_at: updatedAt } : record
  );
}

function safeId() {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`;
}
