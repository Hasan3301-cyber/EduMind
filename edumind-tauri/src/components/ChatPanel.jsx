import { useEffect, useMemo, useRef, useState } from "react";

export function ChatPanel({ client, connectionState, onNavigate }) {
  const sessionKey = useRef(createChatSessionKey());
  const [draft, setDraft] = useState("");
  const [isSending, setIsSending] = useState(false);
  const [error, setError] = useState(null);
  const [agents, setAgents] = useState([]);
  const [selectedAgentId, setSelectedAgentId] = useState("");
  const [messages, setMessages] = useState([
    {
      id: "welcome",
      role: "assistant",
      content: "I can coordinate your next useful study action. Connect a provider in Administration when you are ready for live answers."
    }
  ]);
  const selectedAgent = useMemo(
    () => agents.find((agent) => agent.id === selectedAgentId) ?? null,
    [agents, selectedAgentId]
  );

  useEffect(() => {
    let isActive = true;
    if (!client || connectionState !== "connected") {
      setAgents([]);
      setSelectedAgentId("");
      return undefined;
    }
    client
      .agentStatus()
      .then((status) => {
        if (!isActive) {
          return;
        }
        const availableAgents = (status?.agents ?? []).filter((agent) => agent.enabled);
        setAgents(availableAgents);
        setSelectedAgentId((current) => availableAgents.some((agent) => agent.id === current) ? current : status?.defaultAgent ?? "");
      })
      .catch(() => isActive && setAgents([]));
    return () => {
      isActive = false;
    };
  }, [client, connectionState]);

  async function send(event) {
    event.preventDefault();
    const content = draft.trim();
    if (!content || isSending) {
      return;
    }
    const userMessage = { id: "user-" + Date.now(), role: "user", content };
    setMessages((current) => [...current, userMessage]);
    setDraft("");
    setError(null);

    if (!client || connectionState !== "connected") {
      setMessages((current) => [
        ...current,
        {
          id: "assistant-" + Date.now(),
          role: "assistant",
          content: "You are in offline preview. Launch the desktop app to connect this chat to its embedded gateway."
        }
      ]);
      return;
    }

    setIsSending(true);
    try {
      const response = await client.agentRun({
        message: content,
        sessionKey: sessionKey.current,
        agentId: selectedAgentId || undefined
      });
      setMessages((current) => [
        ...current,
        {
          id: "assistant-" + Date.now(),
          role: "assistant",
          content: response?.content || "The master agent returned no text.",
          model: response?.model,
          toolsUsed: Array.isArray(response?.toolsUsed) ? response.toolsUsed : []
        }
      ]);
    } catch (reason) {
      setError(reason.message);
    } finally {
      setIsSending(false);
    }
  }

  return (
    <section className="chat-panel">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">{selectedAgent?.name ?? "Master Agent"}</p>
          <h1>Plan the next useful study action.</h1>
        </div>
        <div className="chat-header-actions">
          {agents.length > 1 && (
            <label className="chat-agent-picker">
              <span>Chat agent</span>
              <select
                value={selectedAgentId}
                onChange={(event) => {
                  sessionKey.current = createChatSessionKey();
                  setSelectedAgentId(event.target.value);
                }}
              >
                {agents.map((agent) => <option key={agent.id} value={agent.id}>{agent.name}</option>)}
              </select>
            </label>
          )}
          <button type="button" className="secondary-button" onClick={() => onNavigate("admin")}>
            <i className="fa-solid fa-key" aria-hidden="true" />
            LLM settings
          </button>
        </div>
      </div>
      {selectedAgent && selectedAgent.id !== "master" && <p className="chat-agent-note">Chatting with {selectedAgent.name}. Automatic module workflows remain Master-coordinated.</p>}
      <div className="chat-history" aria-live="polite">
        {messages.map((message) => (
          <article className={"chat-message " + message.role} key={message.id}>
            <strong>{message.role === "assistant" ? "EduMind" : "You"}</strong>
            <p>{message.content}</p>
            {message.model && <small className="chat-model-note">Answered by {message.model}</small>}
            {message.toolsUsed?.length > 0 && <small className="chat-tool-note">Read-only context: {message.toolsUsed.join(", ")}</small>}
          </article>
        ))}
      </div>
      {error && (
        <div className="chat-provider-error" role="alert">
          <p>{error}</p>
          <button type="button" className="secondary-button" onClick={() => onNavigate("admin")}>Open LLM settings</button>
        </div>
      )}
      <form className="chat-compose" onSubmit={send}>
        <label htmlFor="chat-draft">Message</label>
        <textarea
          id="chat-draft"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder="Ask for a focused study plan…"
          rows="3"
        />
        <button type="submit" disabled={isSending}>{isSending ? "Thinking…" : "Send message"}</button>
      </form>
    </section>
  );
}

function createChatSessionKey() {
  const suffix = typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : Date.now().toString(36) + Math.random().toString(36).slice(2);
  return "desktop:chat:" + suffix;
}
