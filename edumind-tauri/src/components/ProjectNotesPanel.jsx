import { useState } from "react";

import {
  PROJECT_NOTE_STATUSES,
  createProjectNoteKey,
  createProjectNoteValue,
  emptyProjectNoteDraft,
  projectNoteStats,
  projectNotesFromRecords,
  safeHttpUrl,
  validateProjectNoteDraft
} from "../services/project-notes";
import "./ProjectNotesPanel.css";

export function ProjectNotesPanel({ records, onPersist, saving }) {
  const [draft, setDraft] = useState(() => emptyProjectNoteDraft());
  const [editingKey, setEditingKey] = useState(null);
  const [status, setStatus] = useState("Project notes are stored with your local Student OS records.");
  const projectNotes = projectNotesFromRecords(records);
  const stats = projectNoteStats(projectNotes);

  function updateDraft(field, value) {
    setDraft((current) => ({ ...current, [field]: value }));
  }

  async function saveProjectNote(event) {
    event.preventDefault();
    const validation = validateProjectNoteDraft(draft);
    if (!validation.valid) {
      setStatus(validation.error);
      return;
    }

    const updatedAt = new Date().toISOString();
    const key = editingKey ?? createProjectNoteKey();
    const value = createProjectNoteValue(validation.value, draft.created_at || updatedAt);
    const next = upsertRecord(records, key, value, updatedAt);
    const successMessage = editingKey
      ? "Updated the project note in your canonical Student OS page."
      : "Saved the project note in your canonical Student OS page.";
    const saved = await onPersist(next, successMessage);
    if (saved) {
      resetEditor();
      setStatus(successMessage);
    } else {
      setStatus(editingKey
        ? "Could not update the project note. Your previous Student OS data was restored."
        : "Could not save the project note. Your previous Student OS data was restored.");
    }
  }

  function editProjectNote(projectNote) {
    setEditingKey(projectNote.key);
    setDraft({ ...projectNote.value });
    setStatus(`Editing ${projectNote.value.title}.`);
  }

  async function removeProjectNote(projectNote) {
    const confirmed = globalThis.confirm?.(`Remove the project note “${projectNote.value.title}”?`) ?? true;
    if (!confirmed) {
      return;
    }
    const next = markDeleted(records, projectNote.key);
    const saved = await onPersist(next, "Removed the project note from your canonical Student OS page.");
    if (saved) {
      if (editingKey === projectNote.key) {
        resetEditor();
      }
      setStatus("Removed the project note from your canonical Student OS page.");
    } else {
      setStatus("Could not remove the project note. Your previous Student OS data was restored.");
    }
  }

  function resetEditor() {
    setDraft(emptyProjectNoteDraft());
    setEditingKey(null);
  }

  return (
    <section className="project-notes-workspace" aria-labelledby="project-notes-title">
      <header className="project-notes-header">
        <div>
          <p className="eyebrow">Build log</p>
          <h2 id="project-notes-title">Projects &amp; Notes</h2>
          <p>Keep each project’s purpose, working notes, resources, and learning evidence together.</p>
        </div>
        <div className="project-notes-local-badge">
          <i className="fa-solid fa-hard-drive" aria-hidden="true" />
          <span>Stored locally</span>
        </div>
      </header>

      <div className="project-note-summary" aria-label="Project note tracker summary">
        <SummaryCard label="Projects" value={stats.total} detail="Saved project notes" />
        <SummaryCard label="Active" value={stats.active} detail="Projects in progress" tone="accent" />
        <SummaryCard label="Complete" value={stats.complete} detail="Finished build logs" />
        <SummaryCard label="Resources" value={stats.resources} detail="Safe links attached" tone="accent" />
      </div>

      <div className="project-notes-grid">
        <div className="project-notes-main">
          <form className="project-note-editor" onSubmit={saveProjectNote}>
            <div className="project-note-editor-heading">
              <div>
                <p className="eyebrow">{editingKey ? "Edit project note" : "Add project note"}</p>
                <h3>{editingKey ? "Refine your build log" : "Capture your work while it is fresh"}</h3>
              </div>
              {editingKey ? <span className="project-note-editing">Editing</span> : null}
            </div>
            <div className="project-note-form-grid">
              <label className="project-note-wide">
                <span>Project name</span>
                <input value={draft.title} onChange={(event) => updateDraft("title", event.target.value)} placeholder="e.g. Physics lab report" disabled={saving} />
              </label>
              <label>
                <span>Project status</span>
                <select value={draft.status} onChange={(event) => updateDraft("status", event.target.value)} disabled={saving}>
                  {PROJECT_NOTE_STATUSES.map((projectStatus) => <option key={projectStatus.id} value={projectStatus.id}>{projectStatus.label}</option>)}
                </select>
              </label>
              <label>
                <span>Technology or course</span>
                <input value={draft.technology} onChange={(event) => updateDraft("technology", event.target.value)} placeholder="Python, Calculus, Biology…" disabled={saving} />
              </label>
              <label className="project-note-wide">
                <span>Goal or idea</span>
                <textarea value={draft.summary} onChange={(event) => updateDraft("summary", event.target.value)} placeholder="What are you building, investigating, or trying to learn?" disabled={saving} />
              </label>
              <label className="project-note-wide">
                <span>Project notes</span>
                <textarea className="project-note-body-input" value={draft.notes} onChange={(event) => updateDraft("notes", event.target.value)} placeholder="Capture decisions, evidence, questions, mistakes, and next steps." disabled={saving} />
              </label>
              <label>
                <span>What I learnt</span>
                <textarea value={draft.learnings} onChange={(event) => updateDraft("learnings", event.target.value)} placeholder="Key concept, skill, or takeaway" disabled={saving} />
              </label>
              <label>
                <span>Resource link</span>
                <input value={draft.resource_url} onChange={(event) => updateDraft("resource_url", event.target.value)} placeholder="https://…" inputMode="url" disabled={saving} />
              </label>
            </div>
            <div className="project-note-editor-actions">
              <p aria-live="polite">{status}</p>
              <div>
                {editingKey ? <button type="button" className="text-button" onClick={resetEditor} disabled={saving}>Cancel edit</button> : null}
                <button type="submit" disabled={saving}>{editingKey ? "Update project note" : "Save project note"}</button>
              </div>
            </div>
          </form>

          <div className="project-note-list" aria-live="polite">
            {projectNotes.length ? projectNotes.map((projectNote) => (
              <ProjectNoteCard key={projectNote.key} projectNote={projectNote} saving={saving} onEdit={editProjectNote} onRemove={removeProjectNote} />
            )) : <EmptyProjectNotes />}
          </div>
        </div>

        <aside className="project-notes-aside">
          <section className="project-note-side-card">
            <p className="eyebrow">How it works</p>
            <h3>One card per project</h3>
            <p>Use the note area as a living build log. Add a source or course context when it helps you return to the work later.</p>
          </section>
          <section className="project-note-side-card project-note-side-card-accent">
            <p className="eyebrow">Student-owned</p>
            <h3>Local by default</h3>
            <p>Notes are saved in your canonical Student OS data on this device. They are not shared or submitted from this page.</p>
          </section>
        </aside>
      </div>
    </section>
  );
}

function SummaryCard({ label, value, detail, tone = "" }) {
  return (
    <article className={`project-note-summary-card ${tone}`.trim()}>
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{detail}</small>
    </article>
  );
}

function ProjectNoteCard({ projectNote, saving, onEdit, onRemove }) {
  const { value } = projectNote;
  const resourceUrl = safeHttpUrl(value.resource_url);
  return (
    <article className="project-note-card">
      <header>
        <div>
          <span className={`project-note-status ${value.status}`}>{statusLabel(value.status)}</span>
          <h3>{value.title}</h3>
          {value.summary ? <p>{value.summary}</p> : null}
        </div>
        <time dateTime={projectNote.updated_at}>Updated {formatUpdatedAt(projectNote.updated_at)}</time>
      </header>
      <div className="project-note-content">
        <section>
          <span>Notes</span>
          <p className="project-note-copy">{value.notes}</p>
        </section>
        {value.technology ? <section><span>Technology or course</span><p>{value.technology}</p></section> : null}
        {value.learnings ? <section><span>What I learnt</span><p className="project-note-copy">{value.learnings}</p></section> : null}
        {resourceUrl ? <section><span>Resource</span><a href={resourceUrl} target="_blank" rel="noreferrer">Open attached resource</a></section> : null}
      </div>
      <footer>
        <button type="button" className="text-button" onClick={() => onEdit(projectNote)} disabled={saving}>Edit {value.title}</button>
        <button type="button" className="text-button project-note-remove" onClick={() => void onRemove(projectNote)} disabled={saving}>Remove {value.title}</button>
      </footer>
    </article>
  );
}

function EmptyProjectNotes() {
  return (
    <section className="project-note-empty">
      <i className="fa-regular fa-note-sticky" aria-hidden="true" />
      <h3>Your first project note starts here.</h3>
      <p>Add a project name and a short working note to create a durable study build log.</p>
    </section>
  );
}

function upsertRecord(records, key, value, updatedAt) {
  const next = Array.isArray(records) ? [...records] : [];
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
  return (Array.isArray(records) ? records : []).map((record) => (
    record.key === key ? { ...record, deleted: true, updated_at: updatedAt } : record
  ));
}

function statusLabel(status) {
  return PROJECT_NOTE_STATUSES.find((item) => item.id === status)?.label ?? "Active";
}

function formatUpdatedAt(value) {
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return "recently";
  }
  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", year: "numeric" }).format(parsed);
}
