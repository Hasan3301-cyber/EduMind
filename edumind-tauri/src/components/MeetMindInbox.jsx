import { useEffect, useState } from "react";

import {
  clearMeetMindSyncApiKey,
  fetchMeetMindTranscripts,
  getMeetMindSyncSettings,
  importMeetMindTranscript,
  saveMeetMindSyncSettings
} from "../tauri-bridge";

const EMPTY_SETTINGS = {
  supabaseUrl: "",
  apiKey: ""
};

export function MeetMindInbox({ client, connectionState, onImported }) {
  const [desktopMode, setDesktopMode] = useState(null);
  const [settings, setSettings] = useState(EMPTY_SETTINGS);
  const [syncStatus, setSyncStatus] = useState(null);
  const [transcripts, setTranscripts] = useState([]);
  const [deleteRemote, setDeleteRemote] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [isFetching, setIsFetching] = useState(false);
  const [importingId, setImportingId] = useState(null);
  const [status, setStatus] = useState(null);
  const [error, setError] = useState(null);

  useEffect(() => {
    let active = true;
    getMeetMindSyncSettings()
      .then((nextStatus) => {
        if (!active) {
          return;
        }
        if (!nextStatus) {
          setDesktopMode(false);
          return;
        }
        setDesktopMode(true);
        setSyncStatus(nextStatus);
        setSettings({
          supabaseUrl: nextStatus.supabaseUrl || "",
          apiKey: ""
        });
      })
      .catch((reason) => {
        if (active) {
          setDesktopMode(true);
          setError(reason.message);
        }
      });
    return () => {
      active = false;
    };
  }, []);

  async function saveSettings(event) {
    event.preventDefault();
    setIsSaving(true);
    setError(null);
    setStatus(null);
    try {
      const nextStatus = await saveMeetMindSyncSettings(settings);
      setSyncStatus(nextStatus);
      setSettings((current) => ({ ...current, apiKey: "" }));
      setStatus("MeetMind inbox settings were saved securely on this device.");
    } catch (reason) {
      setError(reason.message);
    } finally {
      setIsSaving(false);
    }
  }

  async function clearStoredKey() {
    if (!window.confirm("Remove the stored MeetMind Supabase key from this device?")) {
      return;
    }
    setIsSaving(true);
    setError(null);
    setStatus(null);
    try {
      const nextStatus = await clearMeetMindSyncApiKey();
      setSyncStatus(nextStatus);
      setTranscripts([]);
      setStatus("The stored MeetMind Supabase key was removed from this device.");
    } catch (reason) {
      setError(reason.message);
    } finally {
      setIsSaving(false);
    }
  }

  async function refreshInbox() {
    setIsFetching(true);
    setError(null);
    setStatus(null);
    try {
      const nextTranscripts = await fetchMeetMindTranscripts();
      setTranscripts(Array.isArray(nextTranscripts) ? nextTranscripts : []);
      setStatus(
        Array.isArray(nextTranscripts) && nextTranscripts.length
          ? "MeetMind inbox refreshed. Review each transcript before importing it."
          : "No MeetMind transcripts are waiting in this inbox."
      );
    } catch (reason) {
      setError(reason.message);
    } finally {
      setIsFetching(false);
    }
  }

  async function importTranscript(transcript) {
    if (!client || connectionState !== "connected") {
      setError("Connect the embedded EduMind gateway before importing into Class Notes memory.");
      return;
    }
    if (transcript.alreadyImported || !transcript.importable || importingId) {
      return;
    }
    const confirmation = deleteRemote
      ? "Import this MeetMind transcript into local Class Notes memory and permanently remove its remote inbox copy?"
      : "Import this MeetMind transcript into local Class Notes memory? The remote inbox copy will remain.";
    if (!window.confirm(confirmation)) {
      return;
    }

    setImportingId(transcript.id);
    setError(null);
    setStatus(null);
    try {
      const result = await importMeetMindTranscript({
        id: transcript.id,
        deleteRemote
      });
      setTranscripts((current) => current.map((item) => (
        item.id === transcript.id
          ? { ...item, alreadyImported: true }
          : item
      )));
      await onImported?.();
      if (result.remoteDeleted) {
        setStatus("Transcript imported into Class Notes memory and removed from the remote inbox.");
      } else if (result.remoteDeleteFailed) {
        setStatus("Transcript imported into Class Notes memory, but its remote copy could not be removed.");
      } else if (!result.localIndexRecorded) {
        setStatus("Transcript imported into Class Notes memory. Refresh before importing it again because the local inbox index could not be updated.");
      } else {
        setStatus("Transcript imported into local Class Notes memory. The remote copy remains by your choice.");
      }
    } catch (reason) {
      setError(reason.message);
    } finally {
      setImportingId(null);
    }
  }

  return (
    <section className="meetmind-inbox">
      <div className="meetmind-inbox-heading">
        <div>
          <p className="eyebrow">MeetMind companion</p>
          <h2>Bring your lecture recordings into Class Notes.</h2>
        </div>
        <i className="fa-solid fa-microphone-lines" aria-hidden="true" />
      </div>
      <p className="meetmind-inbox-description">MeetMind records on your phone, transcribes securely, and delivers the text here. Imported transcripts become local Class Notes evidence for the master agent.</p>

      {desktopMode === false ? (
        <p className="meetmind-inbox-unavailable">The Supabase inbox opens in the installed EduMind desktop app. A MeetMind direct HTTPS bridge already stores transcripts in Class Notes memory automatically.</p>
      ) : desktopMode === null ? (
        <p className="muted">Checking the secure MeetMind inbox…</p>
      ) : (
        <>
          <form className="meetmind-settings-form" onSubmit={saveSettings}>
            <label>
              <span>Supabase URL</span>
              <input
                aria-label="MeetMind Supabase URL"
                value={settings.supabaseUrl}
                onChange={(event) => setSettings((current) => ({ ...current, supabaseUrl: event.target.value }))}
                placeholder="https://your-project.supabase.co"
                required
              />
            </label>
            <label>
              <span>Supabase anonymous key</span>
              <input
                aria-label="MeetMind Supabase key"
                type="password"
                value={settings.apiKey}
                onChange={(event) => setSettings((current) => ({ ...current, apiKey: event.target.value }))}
                placeholder={syncStatus?.apiKeyConfigured ? "Stored securely — leave blank to keep it" : "Paste the Supabase anonymous key"}
                autoComplete="off"
              />
            </label>
            <div className="meetmind-settings-actions">
              <button type="submit" disabled={isSaving || isFetching || Boolean(importingId)}>{isSaving ? "Saving…" : "Save inbox securely"}</button>
              {syncStatus?.apiKeyConfigured && (
                <button type="button" className="secondary-button" disabled={isSaving || isFetching || Boolean(importingId)} onClick={clearStoredKey}>Remove stored key</button>
              )}
            </div>
            <p>Only the URL is stored in app settings. The Supabase key stays in your operating system keychain.</p>
          </form>

          <div className="meetmind-inbox-actions">
            <button type="button" className="secondary-button" disabled={!syncStatus?.configured || isFetching || isSaving || Boolean(importingId)} onClick={refreshInbox}>
              {isFetching ? "Refreshing inbox…" : "Refresh MeetMind inbox"}
            </button>
            <label className="meetmind-delete-option">
              <input
                type="checkbox"
                checked={deleteRemote}
                onChange={(event) => setDeleteRemote(event.target.checked)}
                disabled={isFetching || isSaving || Boolean(importingId)}
              />
              <span>Remove cloud copy after a successful local import</span>
            </label>
          </div>

          {transcripts.length > 0 && (
            <div className="meetmind-transcript-list">
              {transcripts.map((transcript) => (
                <article className="meetmind-transcript-card" key={transcript.id}>
                  <div>
                    <strong>Lecture transcript</strong>
                    <span>{formatDate(transcript.createdAt)} · {Number(transcript.characterCount || 0).toLocaleString()} characters</span>
                  </div>
                  <p>{transcript.preview}</p>
                  {transcript.issue && <small className="error-message">{transcript.issue}</small>}
                  {transcript.alreadyImported ? (
                    <small className="meetmind-imported-note">Already in local Class Notes memory.</small>
                  ) : (
                    <button
                      type="button"
                      className="secondary-button"
                      disabled={!transcript.importable || isFetching || isSaving || Boolean(importingId)}
                      onClick={() => importTranscript(transcript)}
                    >
                      {importingId === transcript.id ? "Importing…" : "Import into Class Notes"}
                    </button>
                  )}
                </article>
              ))}
            </div>
          )}
        </>
      )}

      {status && <p className="meetmind-inbox-status" role="status">{status}</p>}
      {error && <p className="error-message" role="alert">{error}</p>}
    </section>
  );
}

function formatDate(value) {
  if (!value) {
    return "Unknown date";
  }
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "Unknown date" : date.toLocaleString();
}
