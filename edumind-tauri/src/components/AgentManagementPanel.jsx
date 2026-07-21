import { useEffect, useMemo, useState } from "react";

import {
  getAgentManagementSettings,
  getAgentSandboxDocuments,
  saveAgentManagementSettings,
  saveAgentSandboxDocuments
} from "../tauri-bridge";

const DOCUMENT_TABS = [
  { id: "agents", label: "AGENTS.md" },
  { id: "soul", label: "SOUL.md" },
  { id: "identity", label: "IDENTITY.md" },
  { id: "user", label: "USER.md" }
];

export function AgentManagementPanel({ desktopMode, onChanged }) {
  const [settings, setSettings] = useState(null);
  const [persistedAgentIds, setPersistedAgentIds] = useState([]);
  const [selectedAgentId, setSelectedAgentId] = useState("master");
  const [documents, setDocuments] = useState(null);
  const [activeDocument, setActiveDocument] = useState("agents");
  const [isLoading, setIsLoading] = useState(false);
  const [isSavingAgents, setIsSavingAgents] = useState(false);
  const [isSavingDocuments, setIsSavingDocuments] = useState(false);
  const [error, setError] = useState(null);
  const [message, setMessage] = useState(null);

  const selectedAgent = useMemo(
    () => settings?.agents?.find((agent) => agent.id === selectedAgentId) ?? null,
    [selectedAgentId, settings]
  );
  const selectedAgentIsSaved = persistedAgentIds.includes(selectedAgentId);
  const activeDocumentLabel = DOCUMENT_TABS.find((document) => document.id === activeDocument)?.label ?? "AGENTS.md";

  useEffect(() => {
    let isActive = true;
    if (!desktopMode) {
      setSettings(null);
      setPersistedAgentIds([]);
      return undefined;
    }
    setIsLoading(true);
    setError(null);
    getAgentManagementSettings()
      .then((nextSettings) => {
        if (!isActive || !nextSettings) {
          return;
        }
        setSettings(nextSettings);
        setPersistedAgentIds(nextSettings.agents.map((agent) => agent.id));
        setSelectedAgentId((current) => nextSettings.agents.some((agent) => agent.id === current) ? current : nextSettings.defaultAgent);
      })
      .catch((reason) => isActive && setError(reason.message))
      .finally(() => isActive && setIsLoading(false));
    return () => {
      isActive = false;
    };
  }, [desktopMode]);

  useEffect(() => {
    let isActive = true;
    if (!desktopMode || !selectedAgentId || !selectedAgentIsSaved) {
      setDocuments(null);
      return undefined;
    }
    setDocuments(null);
    getAgentSandboxDocuments(selectedAgentId)
      .then((nextDocuments) => isActive && setDocuments(nextDocuments))
      .catch((reason) => isActive && setError(reason.message));
    return () => {
      isActive = false;
    };
  }, [desktopMode, selectedAgentId, selectedAgentIsSaved]);

  function updateSelectedAgent(updates) {
    setSettings((current) => {
      if (!current) {
        return current;
      }
      return {
        ...current,
        agents: current.agents.map((agent) => agent.id === selectedAgentId ? { ...agent, ...updates } : agent)
      };
    });
  }

  function addAgent() {
    if (!settings || settings.agents.length >= 12) {
      setError("Keep up to 12 local desktop agents.");
      return;
    }
    const knownIds = new Set(settings.agents.map((agent) => agent.id));
    let serial = settings.agents.length + 1;
    let id = `study-agent-${serial}`;
    while (knownIds.has(id)) {
      serial += 1;
      id = `study-agent-${serial}`;
    }
    const agent = {
      id,
      name: "New Study Agent",
      enabled: true,
      model: "",
      systemPrompt: "Support one clearly scoped study task at a time. Ground suggestions in available course evidence and state uncertainty.",
      identity: "EduMind study specialist",
      allowedTools: [],
      isMaster: false
    };
    setSettings((current) => ({ ...current, agents: [...current.agents, agent] }));
    setSelectedAgentId(id);
    setDocuments(null);
    setError(null);
    setMessage("Save this profile before editing its sandbox documents.");
  }

  function removeSelectedAgent() {
    if (!settings || !selectedAgent || selectedAgent.isMaster) {
      return;
    }
    if (!window.confirm(`Remove ${selectedAgent.name} from the active desktop registry? Its local sandbox documents will be retained on this device.`)) {
      return;
    }
    setSettings((current) => ({
      ...current,
      agents: current.agents.filter((agent) => agent.id !== selectedAgent.id)
    }));
    setSelectedAgentId("master");
    setDocuments(null);
    setMessage("The agent will be removed after you save the registry. Its sandbox documents stay local.");
  }

  function toggleTool(toolId) {
    if (!selectedAgent) {
      return;
    }
    const tools = new Set(selectedAgent.allowedTools ?? []);
    if (tools.has(toolId)) {
      tools.delete(toolId);
    } else {
      tools.add(toolId);
    }
    updateSelectedAgent({ allowedTools: [...tools].sort() });
  }

  async function saveAgents(submitEvent) {
    submitEvent.preventDefault();
    if (!settings || isSavingAgents) {
      return;
    }
    setIsSavingAgents(true);
    setError(null);
    setMessage(null);
    try {
      const nextSettings = await saveAgentManagementSettings({
        agents: settings.agents.map((agent) => ({
          id: agent.id,
          name: agent.name,
          enabled: Boolean(agent.enabled),
          model: agent.model?.trim() || null,
          systemPrompt: agent.systemPrompt,
          identity: agent.identity,
          allowedTools: agent.allowedTools ?? []
        }))
      });
      setSettings(nextSettings);
      setPersistedAgentIds(nextSettings.agents.map((agent) => agent.id));
      setSelectedAgentId((current) => nextSettings.agents.some((agent) => agent.id === current) ? current : nextSettings.defaultAgent);
      setMessage("Local agent registry saved. The embedded gateway is using the new profiles now.");
      await onChanged?.();
    } catch (reason) {
      setError(reason.message);
    } finally {
      setIsSavingAgents(false);
    }
  }

  async function saveDocuments(submitEvent) {
    submitEvent.preventDefault();
    if (!documents || isSavingDocuments) {
      return;
    }
    setIsSavingDocuments(true);
    setError(null);
    setMessage(null);
    try {
      const savedDocuments = await saveAgentSandboxDocuments({
        agentId: selectedAgentId,
        soul: documents.soul,
        agents: documents.agents,
        user: documents.user,
        identity: documents.identity
      });
      setDocuments(savedDocuments);
      setMessage("Sandbox control files saved locally. They apply on the next agent turn.");
    } catch (reason) {
      setError(reason.message);
    } finally {
      setIsSavingDocuments(false);
    }
  }

  if (!desktopMode) {
    return (
      <article className="agent-management-card agent-management-unavailable">
        <div className="agent-management-heading">
          <div>
            <p className="eyebrow">Agent OS</p>
            <h2>Local Agent Sandbox</h2>
          </div>
          <i className="fa-solid fa-user-shield" aria-hidden="true" />
        </div>
        <p>Agent management and sandbox documents open only in the installed EduMind desktop app. Browser preview never edits local agent policy.</p>
      </article>
    );
  }

  return (
    <article className="agent-management-card">
      <div className="agent-management-heading">
        <div>
          <p className="eyebrow">Agent OS</p>
          <h2>Local Agent Sandbox</h2>
        </div>
        <i className="fa-solid fa-user-shield" aria-hidden="true" />
      </div>
      <p className="agent-management-intro">Create focused local study profiles and edit their sandbox control files. The Master Agent remains the coordinator; every profile stays non-delegating and can use only the read-only tools shown here.</p>
      {isLoading && <p className="agent-management-loading">Loading your local agent registry…</p>}
      {settings && selectedAgent && (
        <>
          <div className="agent-management-layout">
            <aside className="agent-list" aria-label="Desktop agents">
              <div className="agent-list-heading">
                <span>{settings.agents.length} profiles</span>
                <button type="button" className="secondary-button" onClick={addAgent} disabled={isSavingAgents}>Add agent</button>
              </div>
              {settings.agents.map((agent) => (
                <button
                  type="button"
                  className={`agent-list-item${agent.id === selectedAgentId ? " is-selected" : ""}`}
                  key={agent.id}
                  aria-pressed={agent.id === selectedAgentId}
                  onClick={() => {
                    setSelectedAgentId(agent.id);
                    setError(null);
                    setMessage(null);
                  }}
                >
                  <strong>{agent.name}</strong>
                  <span>{agent.isMaster ? "Master coordinator" : agent.enabled ? "Ready for Chat" : "Paused"}</span>
                </button>
              ))}
            </aside>

            <form className="agent-editor" onSubmit={saveAgents}>
              <div className="agent-editor-title">
                <div>
                  <p className="eyebrow">{selectedAgent.isMaster ? "Required profile" : "Study profile"}</p>
                  <h3>{selectedAgent.name}</h3>
                </div>
                {!selectedAgent.isMaster && (
                  <button type="button" className="secondary-button danger-button" onClick={removeSelectedAgent} disabled={isSavingAgents}>Remove</button>
                )}
              </div>
              <div className="agent-editor-fields">
                <label>
                  <span>Agent ID</span>
                  <input
                    value={selectedAgent.id}
                    onChange={(event) => updateSelectedAgent({ id: event.target.value.toLowerCase() })}
                    readOnly={selectedAgentIsSaved || selectedAgent.isMaster}
                    aria-label="Agent ID"
                  />
                </label>
                <label>
                  <span>Name</span>
                  <input value={selectedAgent.name} onChange={(event) => updateSelectedAgent({ name: event.target.value })} aria-label="Agent name" required />
                </label>
                <label className="agent-enabled-field">
                  <span>Availability</span>
                  <span className="agent-switch">
                    <input
                      type="checkbox"
                      checked={Boolean(selectedAgent.enabled)}
                      disabled={selectedAgent.isMaster}
                      onChange={(event) => updateSelectedAgent({ enabled: event.target.checked })}
                    />
                    {selectedAgent.enabled ? "Active" : "Paused"}
                  </span>
                </label>
                <label>
                  <span>Model override</span>
                  <input
                    value={selectedAgent.model ?? ""}
                    onChange={(event) => updateSelectedAgent({ model: event.target.value })}
                    placeholder="Uses the configured provider model"
                    aria-label="Agent model override"
                  />
                </label>
                <label className="agent-full-field">
                  <span>Identity</span>
                  <input value={selectedAgent.identity} onChange={(event) => updateSelectedAgent({ identity: event.target.value })} aria-label="Agent identity" required />
                </label>
                <label className="agent-full-field">
                  <span>System instruction</span>
                  <textarea value={selectedAgent.systemPrompt} onChange={(event) => updateSelectedAgent({ systemPrompt: event.target.value })} rows="5" aria-label="Agent system instruction" required />
                </label>
              </div>
              <div className="agent-tools-section">
                <div>
                  <p className="eyebrow">Read-only access</p>
                  <h4>Choose approved study context</h4>
                </div>
                <div className="agent-tool-grid">
                  {settings.availableTools.map((tool) => (
                    <label className="agent-tool-toggle" key={tool.id}>
                      <input type="checkbox" checked={selectedAgent.allowedTools?.includes(tool.id)} onChange={() => toggleTool(tool.id)} />
                      <span>
                        <strong>{tool.label}</strong>
                        <small>{tool.description}</small>
                      </span>
                    </label>
                  ))}
                </div>
              </div>
              <div className="agent-editor-actions">
                <button type="submit" disabled={isSavingAgents}>{isSavingAgents ? "Saving profiles…" : "Save agent profiles"}</button>
                <span>Only local configuration changes are applied.</span>
              </div>
            </form>
          </div>

          <section className="agent-documents-section" aria-labelledby="agent-documents-title">
            <div>
              <p className="eyebrow">Sandbox control files</p>
              <h3 id="agent-documents-title">{selectedAgent.name} workspace</h3>
              <p>These files supply role and learner preferences for the selected agent. They cannot grant tools, disable confirmations, or access files outside the local sandbox.</p>
            </div>
            {!selectedAgentIsSaved ? (
              <p className="agent-document-notice">Save this new agent profile before creating its sandbox workspace.</p>
            ) : !documents ? (
              <p className="agent-document-notice">Loading local control files…</p>
            ) : (
              <form className="agent-documents-editor" onSubmit={saveDocuments}>
                <div className="agent-document-tabs" role="tablist" aria-label="Sandbox documents">
                  {DOCUMENT_TABS.map((document) => (
                    <button
                      type="button"
                      role="tab"
                      aria-selected={activeDocument === document.id}
                      className={activeDocument === document.id ? "is-active" : ""}
                      key={document.id}
                      onClick={() => setActiveDocument(document.id)}
                    >
                      {document.label}
                    </button>
                  ))}
                </div>
                <label>
                  <span>{activeDocumentLabel}</span>
                  <textarea
                    value={documents[activeDocument] ?? ""}
                    onChange={(event) => setDocuments((current) => ({ ...current, [activeDocument]: event.target.value }))}
                    rows="11"
                    aria-label={`${activeDocumentLabel} content`}
                  />
                </label>
                <div className="agent-document-actions">
                  <button type="submit" disabled={isSavingDocuments}>{isSavingDocuments ? "Saving sandbox…" : "Save sandbox files"}</button>
                  <span>Credentials and private keys are blocked from this local policy store.</span>
                </div>
              </form>
            )}
          </section>
        </>
      )}
      {message && <p className="agent-management-message" role="status">{message}</p>}
      {error && <p className="error-message" role="alert">{error}</p>}
    </article>
  );
}
