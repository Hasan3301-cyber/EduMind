import { useEffect, useState } from "react";

import { AgentManagementPanel } from "./AgentManagementPanel";
import {
  clearLlmProviderApiKey,
  getLlmProviderSettings,
  saveLlmProviderSettings,
  testLlmProviderConnection
} from "../tauri-bridge";

const DEFAULT_LLM_SETTINGS = {
  baseUrl: "https://api.openai.com/v1",
  model: "gpt-4o-mini",
  apiKey: ""
};

export function AdminPanel({ client, endpoint }) {
  const [runtime, setRuntime] = useState(null);
  const [error, setError] = useState(null);
  const [agentRuntime, setAgentRuntime] = useState(null);
  const [agentError, setAgentError] = useState(null);
  const [llmSettings, setLlmSettings] = useState(DEFAULT_LLM_SETTINGS);
  const [llmStatus, setLlmStatus] = useState(null);
  const [llmError, setLlmError] = useState(null);
  const [llmMessage, setLlmMessage] = useState(null);
  const [llmCheck, setLlmCheck] = useState(null);
  const [isSavingLlm, setIsSavingLlm] = useState(false);
  const [isTestingLlm, setIsTestingLlm] = useState(false);
  const desktopMode = Boolean(endpoint?.embedded);
  const masterAgent = agentRuntime?.agents?.find((agent) => agent.id === agentRuntime.defaultAgent) ?? null;

  useEffect(() => {
    let active = true;
    if (!client) {
      return undefined;
    }
    client
      .runtimeStatus()
      .then((status) => active && setRuntime(status))
      .catch((reason) => active && setError(reason.message));
    client
      .agentStatus()
      .then((status) => active && setAgentRuntime(status))
      .catch((reason) => active && setAgentError(reason.message));
    return () => {
      active = false;
    };
  }, [client]);

  useEffect(() => {
    let active = true;
    if (!desktopMode) {
      setLlmStatus(null);
      return undefined;
    }
    getLlmProviderSettings()
      .then((settings) => {
        if (!active || !settings) {
          return;
        }
        setLlmStatus(settings);
        setLlmSettings((current) => ({
          ...current,
          baseUrl: settings.baseUrl,
          model: settings.model,
          apiKey: ""
        }));
      })
      .catch((reason) => active && setLlmError(reason.message));
    return () => {
      active = false;
    };
  }, [desktopMode]);

  async function saveLlmSettings(event) {
    event.preventDefault();
    setIsSavingLlm(true);
    setLlmError(null);
    setLlmMessage(null);
    setLlmCheck(null);
    try {
      const nextStatus = await saveLlmProviderSettings(llmSettings);
      setLlmStatus(nextStatus);
      setLlmSettings((current) => ({ ...current, apiKey: "" }));
      setLlmMessage("Provider saved locally. The API key is stored in your operating system keychain.");
    } catch (reason) {
      setLlmError(reason.message);
    } finally {
      setIsSavingLlm(false);
    }
  }

  async function clearLlmKey() {
    if (!window.confirm("Remove the stored LLM API key from this device?")) {
      return;
    }
    setIsSavingLlm(true);
    setLlmError(null);
    setLlmMessage(null);
    setLlmCheck(null);
    try {
      const nextStatus = await clearLlmProviderApiKey();
      setLlmStatus(nextStatus);
      setLlmMessage("The stored API key was removed from this device.");
    } catch (reason) {
      setLlmError(reason.message);
    } finally {
      setIsSavingLlm(false);
    }
  }

  async function testLlmConnection() {
    setIsTestingLlm(true);
    setLlmError(null);
    setLlmMessage(null);
    setLlmCheck(null);
    try {
      setLlmCheck(await testLlmProviderConnection());
    } catch (reason) {
      setLlmError(reason.message);
    } finally {
      setIsTestingLlm(false);
    }
  }

  return (
    <section className="admin-panel">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Administration</p>
          <h1>Local runtime health and optional enhancements.</h1>
        </div>
      </div>
      <div className="admin-grid">
        <article>
          <h2>Embedded gateway</h2>
          <p>{endpoint?.embedded ? "Running in-process with a per-launch token." : "Browser preview is disconnected."}</p>
        </article>
        <article>
          <h2>Runtime tools</h2>
          {runtime ? (
            <ul className="status-list">
              <li>LaTeX: {runtime.latex_enabled ? "enabled" : "optional"}</li>
              <li>Document converter: {runtime.document_converter_enabled ? "enabled" : "optional"}</li>
              <li>Slide converter: {runtime.slide_converter_enabled ? "enabled" : "optional"}</li>
              <li>NotebookLM: {runtime.notebooklm_enabled ? "enabled" : "optional"}</li>
            </ul>
          ) : (
            <p>{error ?? "Checking local runtime…"}</p>
          )}
        </article>
        <article className="agent-runtime-card">
          <div className="agent-runtime-heading">
            <div>
              <p className="eyebrow">Agent runtime</p>
              <h2>Master-agent guardrails</h2>
            </div>
            <i className="fa-solid fa-shield-halved" aria-hidden="true" />
          </div>
          {agentRuntime ? (
            <>
              <ul className="status-list">
                <li>Default agent: {agentRuntime.defaultAgent}</li>
                <li>Default model: {agentRuntime.defaultModel}</li>
                <li>Tool profile: {agentRuntime.toolProfile}</li>
                <li>Read-only tools available: {masterAgent?.allowedTools?.length ?? 0}</li>
              </ul>
              {masterAgent?.allowedTools?.length > 0 && (
                <p className="agent-tool-list">Allowed context: {masterAgent.allowedTools.join(", ")}.</p>
              )}
            </>
          ) : (
            <p>{agentError ?? "Checking the local master agent…"}</p>
          )}
        </article>
        <AgentManagementPanel
          desktopMode={desktopMode}
          onChanged={async () => {
            if (!client) {
              return;
            }
            try {
              setAgentRuntime(await client.agentStatus());
              setAgentError(null);
            } catch (reason) {
              setAgentError(reason.message);
            }
          }}
        />
        <article className="llm-provider-card">
          <div className="llm-provider-heading">
            <div>
              <p className="eyebrow">Private LLM setup</p>
              <h2>Study assistant provider</h2>
            </div>
            <i className="fa-solid fa-wand-magic-sparkles" aria-hidden="true" />
          </div>
          <p>Use OpenAI, OpenRouter, LM Studio, Ollama, or another OpenAI-compatible endpoint. EduMind stores only the base URL and model locally; your API key stays in the operating system keychain.</p>
          {desktopMode ? (
            <form className="llm-provider-form" onSubmit={saveLlmSettings}>
              <label>
                <span>Base URL</span>
                <input
                  aria-label="LLM base URL"
                  value={llmSettings.baseUrl}
                  onChange={(event) => setLlmSettings((current) => ({ ...current, baseUrl: event.target.value }))}
                  placeholder="https://api.openai.com/v1"
                  required
                />
              </label>
              <label>
                <span>Model</span>
                <input
                  aria-label="LLM model"
                  value={llmSettings.model}
                  onChange={(event) => setLlmSettings((current) => ({ ...current, model: event.target.value }))}
                  placeholder="gpt-4o-mini"
                  required
                />
              </label>
              <label className="llm-api-key-field">
                <span>API key</span>
                <input
                  aria-label="LLM API key"
                  type="password"
                  value={llmSettings.apiKey}
                  onChange={(event) => setLlmSettings((current) => ({ ...current, apiKey: event.target.value }))}
                  placeholder={llmStatus?.apiKeyConfigured ? "Stored securely — leave blank to keep it" : "Optional for local providers"}
                  autoComplete="off"
                />
              </label>
              <div className="llm-provider-actions">
                <button type="submit" disabled={isSavingLlm || isTestingLlm}>{isSavingLlm ? "Saving…" : "Save securely"}</button>
                <button type="button" className="secondary-button" disabled={isSavingLlm || isTestingLlm} onClick={testLlmConnection}>{isTestingLlm ? "Testing…" : "Test connection"}</button>
                {llmStatus?.apiKeyConfigured && (
                  <button type="button" className="secondary-button" disabled={isSavingLlm || isTestingLlm} onClick={clearLlmKey}>Remove stored key</button>
                )}
              </div>
              <p className="llm-provider-test-note">Test connection sends an empty diagnostic request. It never includes study content or displays your key.</p>
            </form>
          ) : (
            <p className="llm-provider-unavailable">Launch the Tauri desktop app to configure an API key securely. Browser preview never stores provider keys.</p>
          )}
          {llmStatus && (
            <p className="llm-provider-status">
              {llmStatus.apiKeyConfigured ? "API key stored securely" : "No API key stored"}
              {llmStatus.configured ? " · active for in-app chat" : " · save a provider to activate chat"}
            </p>
          )}
          {llmMessage && <p className="llm-provider-message">{llmMessage}</p>}
          {llmCheck && <p className={`llm-provider-check llm-provider-check-${llmCheck.status}`} role="status">{llmCheck.message}</p>}
          {llmError && <p className="error-message" role="alert">{llmError}</p>}
        </article>
      </div>
    </section>
  );
}
