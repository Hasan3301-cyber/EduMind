import { useCallback, useEffect, useMemo, useRef, useState } from "react";

const EMPTY_GROUP_DRAFT = Object.freeze({ title: "", topic: "" });
const EMPTY_RESOURCE_DRAFT = Object.freeze({ title: "", url: "", description: "" });

export function GroupStudyPanel({ client, connectionState, onNavigate }) {
  const [groups, setGroups] = useState([]);
  const [activeDetail, setActiveDetail] = useState(null);
  const [profileName, setProfileName] = useState("");
  const [groupDraft, setGroupDraft] = useState(EMPTY_GROUP_DRAFT);
  const [joinCode, setJoinCode] = useState("");
  const [messageDraft, setMessageDraft] = useState("");
  const [resourceDraft, setResourceDraft] = useState(EMPTY_RESOURCE_DRAFT);
  const [aiPrompt, setAiPrompt] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isLoadingRoom, setIsLoadingRoom] = useState(false);
  const [isCreating, setIsCreating] = useState(false);
  const [isJoining, setIsJoining] = useState(false);
  const [isSending, setIsSending] = useState(false);
  const [isSharing, setIsSharing] = useState(false);
  const [isAskingAi, setIsAskingAi] = useState(false);
  const [status, setStatus] = useState("Loading your group study rooms…");
  const [error, setError] = useState(null);
  const activeGroupIdRef = useRef(null);

  const connected = Boolean(client && connectionState === "connected");
  const activeGroup = activeDetail?.group ?? null;
  const memberName = normalizeName(profileName);
  const messages = useMemo(
    () => Array.isArray(activeDetail?.messages) ? activeDetail.messages : [],
    [activeDetail]
  );
  const resources = useMemo(
    () => Array.isArray(activeDetail?.resources) ? activeDetail.resources : [],
    [activeDetail]
  );
  const isMember = Boolean(
    activeGroup && memberName && activeGroup.members?.some((member) => member.name === memberName)
  );

  useEffect(() => {
    activeGroupIdRef.current = activeGroup?.id ?? null;
  }, [activeGroup?.id]);

  const loadRoom = useCallback(async (groupId, { quiet = false } = {}) => {
    if (!client || connectionState !== "connected" || !groupId) {
      return;
    }
    if (!quiet) {
      setIsLoadingRoom(true);
    }
    try {
      const detail = await client.groupStudyGroup(groupId);
      if (!detail?.group?.id) {
        throw new Error("EduMind returned an incomplete Group Study room.");
      }
      activeGroupIdRef.current = detail.group.id;
      setActiveDetail(detail);
      setError(null);
    } catch (reason) {
      setError(safeMessage(reason, "EduMind could not load this Group Study room."));
    } finally {
      if (!quiet) {
        setIsLoadingRoom(false);
      }
    }
  }, [client, connectionState]);

  const loadGroups = useCallback(async ({ preferredId, quiet = false } = {}) => {
    if (!client || connectionState !== "connected") {
      setGroups([]);
      setActiveDetail(null);
      setIsLoading(false);
      if (!quiet) {
        setStatus("Connect the embedded gateway to create local Group Study rooms and continue a shared session.");
      }
      return;
    }
    if (!quiet) {
      setIsLoading(true);
    }
    try {
      const response = await client.groupStudyGroups();
      const nextGroups = Array.isArray(response) ? response : [];
      setGroups(nextGroups);
      const selectedId = preferredId ?? activeGroupIdRef.current;
      const selectedGroup = nextGroups.find((group) => group.id === selectedId) ?? nextGroups[0] ?? null;
      if (selectedGroup) {
        await loadRoom(selectedGroup.id, { quiet: true });
      } else {
        activeGroupIdRef.current = null;
        setActiveDetail(null);
      }
      setError(null);
      if (!quiet) {
        setStatus(nextGroups.length
          ? "Your saved study rooms are ready. Choose one to continue the conversation."
          : "Create a focused room or join with an invite code to begin studying together.");
      }
    } catch (reason) {
      setError(safeMessage(reason, "EduMind could not load Group Study rooms."));
      if (!quiet) {
        setStatus("Group Study is waiting for a healthy local gateway connection.");
      }
    } finally {
      if (!quiet) {
        setIsLoading(false);
      }
    }
  }, [client, connectionState, loadRoom]);

  useEffect(() => {
    void loadGroups();
  }, [loadGroups]);

  useEffect(() => {
    if (!client || connectionState !== "connected") {
      return undefined;
    }
    return client.subscribeEvents({
      onEvent(event) {
        const name = String(event?.event ?? "");
        const groupId = event?.payload?.session_id;
        if (!name.startsWith("collaboration.") || !groupId) {
          return;
        }
        if (groupId === activeGroupIdRef.current || name === "collaboration.session_created") {
          void loadGroups({ preferredId: activeGroupIdRef.current, quiet: true });
        }
      }
    });
  }, [client, connectionState, loadGroups]);

  async function createGroup(event) {
    event.preventDefault();
    const title = groupDraft.title.trim();
    const topic = groupDraft.topic.trim();
    if (!connected) {
      setStatus("Connect the embedded gateway before creating a Group Study room.");
      return;
    }
    if (!memberName) {
      setStatus("Add your name first so your study group knows who started the room.");
      return;
    }
    if (!title || !topic) {
      setStatus("Give the room a clear name and a focused study topic.");
      return;
    }
    if (!window.confirm(`Create “${title}” for ${topic}?\n\nEduMind will save this room on the connected gateway and create an invite code you can share with classmates.`)) {
      return;
    }
    setIsCreating(true);
    setError(null);
    try {
      const created = await client.createGroupStudy({ title, topic, memberName });
      activeGroupIdRef.current = created.group.id;
      setActiveDetail(created);
      setGroups((current) => [created.group, ...current.filter((group) => group.id !== created.group.id)]);
      setGroupDraft(EMPTY_GROUP_DRAFT);
      setStatus(`Created “${created.group.title}”. Share its invite code when your group is ready.`);
    } catch (reason) {
      setError(safeMessage(reason, "EduMind could not create this Group Study room."));
    } finally {
      setIsCreating(false);
    }
  }

  async function joinGroup(event) {
    event?.preventDefault();
    const code = joinCode.trim();
    if (!connected) {
      setStatus("Connect the embedded gateway before joining a Group Study room.");
      return;
    }
    if (!memberName) {
      setStatus("Add your name first so the group can identify you.");
      return;
    }
    if (!code) {
      setStatus("Paste the Group Study invite code first.");
      return;
    }
    if (!window.confirm(`Join the room for invite code ${code}?\n\nYour name will be added to this room's member list.`)) {
      return;
    }
    setIsJoining(true);
    setError(null);
    try {
      const joined = await client.joinGroupStudy({ inviteCode: code, memberName });
      activeGroupIdRef.current = joined.group.id;
      setActiveDetail(joined);
      setGroups((current) => [joined.group, ...current.filter((group) => group.id !== joined.group.id)]);
      setJoinCode("");
      setStatus(`You joined “${joined.group.title}”. Introduce yourself when you are ready.`);
    } catch (reason) {
      setError(safeMessage(reason, "EduMind could not join that Group Study room."));
    } finally {
      setIsJoining(false);
    }
  }

  async function joinActiveGroup() {
    if (!activeGroup) {
      return;
    }
    setJoinCode(activeGroup.invite_code);
    await joinWithCode(activeGroup.invite_code);
  }

  async function joinWithCode(code) {
    if (!connected) {
      setStatus("Connect the embedded gateway before joining a Group Study room.");
      return;
    }
    if (!memberName) {
      setStatus("Add your name first so the group can identify you.");
      return;
    }
    if (!window.confirm(`Join “${activeGroup?.title ?? "this study room"}”?\n\nYour name will be added to this room's member list.`)) {
      return;
    }
    setIsJoining(true);
    setError(null);
    try {
      const joined = await client.joinGroupStudy({ inviteCode: code, memberName });
      activeGroupIdRef.current = joined.group.id;
      setActiveDetail(joined);
      setGroups((current) => [joined.group, ...current.filter((group) => group.id !== joined.group.id)]);
      setStatus(`You joined “${joined.group.title}”. You can now chat and share resources.`);
    } catch (reason) {
      setError(safeMessage(reason, "EduMind could not join that Group Study room."));
    } finally {
      setIsJoining(false);
    }
  }

  async function selectGroup(groupId) {
    activeGroupIdRef.current = groupId;
    setError(null);
    await loadRoom(groupId);
  }

  async function sendMessage(event) {
    event.preventDefault();
    const content = messageDraft.trim();
    if (!activeGroup || !content || isSending) {
      return;
    }
    if (!isMember) {
      setStatus("Join this room with your name before posting a message.");
      return;
    }
    setIsSending(true);
    setError(null);
    try {
      const saved = await client.sendGroupStudyMessage(activeGroup.id, {
        memberName,
        content
      });
      setMessageDraft("");
      appendMessages([saved]);
      setStatus("Message shared with the study room.");
    } catch (reason) {
      setError(safeMessage(reason, "EduMind could not share that message."));
    } finally {
      setIsSending(false);
    }
  }

  async function shareResource(event) {
    event.preventDefault();
    if (!activeGroup || isSharing) {
      return;
    }
    if (!isMember) {
      setStatus("Join this room with your name before sharing a resource.");
      return;
    }
    const title = resourceDraft.title.trim();
    const url = resourceDraft.url.trim();
    if (!title || !url) {
      setStatus("Add both a clear resource title and its HTTPS link.");
      return;
    }
    if (!window.confirm(`Share “${title}” with “${activeGroup.title}”?\n\nThe title, HTTPS link, and optional note will be visible to room members.`)) {
      return;
    }
    setIsSharing(true);
    setError(null);
    try {
      const saved = await client.shareGroupStudyResource(activeGroup.id, {
        memberName,
        title,
        url,
        description: resourceDraft.description.trim() || undefined
      });
      setResourceDraft(EMPTY_RESOURCE_DRAFT);
      setActiveDetail((current) => current?.group?.id === activeGroup.id
        ? { ...current, resources: appendUnique(current.resources, saved) }
        : current);
      setStatus("Resource shared with the room.");
    } catch (reason) {
      setError(safeMessage(reason, "EduMind could not share that resource."));
    } finally {
      setIsSharing(false);
    }
  }

  async function askAiFacilitator(event) {
    event.preventDefault();
    const didAsk = await askAiInSharedChat(aiPrompt);
    if (didAsk) {
      setAiPrompt("");
    }
  }

  async function askAiFromConversation() {
    const didAsk = await askAiInSharedChat(messageDraft);
    if (didAsk) {
      setMessageDraft("");
    }
  }

  async function askAiInSharedChat(value) {
    const question = value.trim();
    if (!activeGroup || !question || isAskingAi || isSending) {
      return false;
    }
    if (!isMember) {
      setStatus("Join this room with your name before asking its AI facilitator.");
      return false;
    }
    if (!window.confirm("Ask EduMind AI in the shared chat?\n\nYour question and its response will be saved for the study room. EduMind will send this room's saved discussion, shared HTTPS resource metadata, and your question to the configured provider. The AI cannot invite people, change a plan, or take actions for the group.")) {
      return false;
    }
    setIsAskingAi(true);
    setError(null);
    try {
      const response = await client.askGroupStudyAi(activeGroup.id, {
        memberName,
        question
      });
      const savedMessages = [response?.question, response?.response].filter(Boolean);
      if (savedMessages.length !== 2) {
        throw new Error("EduMind returned an incomplete shared AI response.");
      }
      appendMessages(savedMessages);
      setStatus("EduMind AI used the shared discussion and added a response to the room.");
      return true;
    } catch (reason) {
      setError(safeMessage(reason, "EduMind could not reach the configured LLM provider."));
      return false;
    } finally {
      setIsAskingAi(false);
    }
  }

  function appendMessages(savedMessages) {
    setActiveDetail((current) => current?.group?.id === activeGroup?.id
      ? {
          ...current,
          messages: savedMessages.reduce((items, saved) => appendUnique(items, saved), current.messages)
        }
      : current);
  }

  async function copyInviteCode() {
    if (!activeGroup?.invite_code) {
      return;
    }
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error("Clipboard access is unavailable.");
      }
      await navigator.clipboard.writeText(activeGroup.invite_code);
      setStatus("Invite code copied. Share it only with classmates you want in this study room.");
    } catch {
      setStatus(`Invite code ready to share: ${activeGroup.invite_code}`);
    }
  }

  return (
    <section className="group-study-panel">
      <header className="group-study-hero">
        <div>
          <p className="eyebrow">Group study</p>
          <h1>Keep your study crew focused, connected, and evidence-aware.</h1>
          <p>Create a room for one topic, invite classmates with a code, discuss ideas, share trustworthy links, and bring EduMind AI directly into the shared conversation when the group needs help.</p>
        </div>
        <aside>
          <i className="fa-solid fa-people-group" aria-hidden="true" />
          <strong>Student-led collaboration</strong>
          <span>Messages and links stay on the connected EduMind gateway. When a member explicitly asks, the AI receives the room's saved discussion context.</span>
        </aside>
      </header>

      <p className="group-study-status" role="status" aria-live="polite">{isLoading ? "Loading Group Study…" : status}</p>
      {error && (
        <div className="group-study-error" role="alert">
          <span>{error}</span>
          <button type="button" className="secondary-button" onClick={() => onNavigate("admin")}>
            <i className="fa-solid fa-sliders" aria-hidden="true" /> Open settings
          </button>
        </div>
      )}

      <div className="group-study-layout">
        <aside className="group-study-sidebar">
          <section className="group-study-card group-study-identity">
            <div className="group-study-card-heading">
              <div>
                <p className="eyebrow">Your study identity</p>
                <h2>Who is in the room?</h2>
              </div>
              <i className="fa-solid fa-id-badge" aria-hidden="true" />
            </div>
            <label htmlFor="group-study-member-name">
              <span>Your name</span>
              <input
                id="group-study-member-name"
                value={profileName}
                onChange={(event) => setProfileName(event.target.value)}
                placeholder="For example: Amina Rahman"
                maxLength="80"
                disabled={!connected}
              />
            </label>
            <small>Use the same name when you return to continue a group room.</small>
          </section>

          <section className="group-study-card group-study-room-list-card">
            <div className="group-study-card-heading">
              <div>
                <p className="eyebrow">Continue studying</p>
                <h2>Your rooms</h2>
              </div>
              <span>{groups.length}</span>
            </div>
            {groups.length ? (
              <div className="group-study-room-list">
                {groups.map((group) => (
                  <button
                    type="button"
                    key={group.id}
                    className={group.id === activeGroup?.id ? "group-study-room-button is-active" : "group-study-room-button"}
                    onClick={() => void selectGroup(group.id)}
                    disabled={isLoadingRoom}
                  >
                    <span className="group-study-room-icon"><i className="fa-solid fa-book-open" aria-hidden="true" /></span>
                    <span>
                      <strong>{group.title}</strong>
                      <small>{group.topic}</small>
                    </span>
                    <em>{group.members?.length ?? 0} member{group.members?.length === 1 ? "" : "s"}</em>
                  </button>
                ))}
              </div>
            ) : (
              <div className="group-study-empty-list">
                <i className="fa-solid fa-seedling" aria-hidden="true" />
                <p>No study rooms yet. Create one for your next shared session.</p>
              </div>
            )}
          </section>

          <form className="group-study-card group-study-create-form" onSubmit={createGroup}>
            <div className="group-study-card-heading">
              <div>
                <p className="eyebrow">Start together</p>
                <h2>Create a room</h2>
              </div>
              <i className="fa-solid fa-circle-plus" aria-hidden="true" />
            </div>
            <label htmlFor="group-study-title">
              <span>Room name</span>
              <input
                id="group-study-title"
                value={groupDraft.title}
                onChange={(event) => setGroupDraft((current) => ({ ...current, title: event.target.value }))}
                placeholder="Calculus review crew"
                maxLength="120"
                disabled={!connected || isCreating}
              />
            </label>
            <label htmlFor="group-study-topic">
              <span>Study focus</span>
              <textarea
                id="group-study-topic"
                value={groupDraft.topic}
                onChange={(event) => setGroupDraft((current) => ({ ...current, topic: event.target.value }))}
                placeholder="Limits before Thursday's quiz"
                rows="3"
                maxLength="240"
                disabled={!connected || isCreating}
              />
            </label>
            <button type="submit" disabled={!connected || isCreating}>
              <i className="fa-solid fa-users" aria-hidden="true" /> {isCreating ? "Creating…" : "Create study room"}
            </button>
          </form>

          <form className="group-study-card group-study-join-form" onSubmit={joinGroup}>
            <div className="group-study-card-heading">
              <div>
                <p className="eyebrow">Classmate invitation</p>
                <h2>Join by code</h2>
              </div>
              <i className="fa-solid fa-right-to-bracket" aria-hidden="true" />
            </div>
            <label htmlFor="group-study-invite-code">
              <span>Invite code</span>
              <input
                id="group-study-invite-code"
                value={joinCode}
                onChange={(event) => setJoinCode(event.target.value)}
                placeholder="STUDY-…"
                maxLength="64"
                disabled={!connected || isJoining}
              />
            </label>
            <button type="submit" className="secondary-button" disabled={!connected || isJoining}>
              <i className="fa-solid fa-door-open" aria-hidden="true" /> {isJoining ? "Joining…" : "Join study room"}
            </button>
          </form>
        </aside>

        <section className="group-study-room" aria-live="polite">
          {!activeGroup ? (
            <div className="group-study-room-empty">
              <span><i className="fa-solid fa-comments" aria-hidden="true" /></span>
              <h2>Your group conversation starts here.</h2>
              <p>Create a room for a topic or join a classmate with an invite code. EduMind keeps the room focused on the learning goal you choose.</p>
            </div>
          ) : (
            <>
              <header className="group-study-room-header">
                <div>
                  <p className="eyebrow">Active study room</p>
                  <h2>{activeGroup.title}</h2>
                  <p>{activeGroup.topic}</p>
                </div>
                <div className="group-study-invite-box">
                  <span>Invite code</span>
                  <code>{activeGroup.invite_code}</code>
                  <button type="button" className="secondary-button" onClick={() => void copyInviteCode()}>
                    <i className="fa-solid fa-copy" aria-hidden="true" /> Copy
                  </button>
                </div>
              </header>

              <div className="group-study-room-meta">
                <div className="group-study-members" aria-label="Group members">
                  {activeGroup.members?.map((member) => (
                    <span key={`${member.name}-${member.joined_at}`} title={`${member.name} joined ${formatDate(member.joined_at)}`}>
                      <b>{initials(member.name)}</b>{member.name}
                    </span>
                  ))}
                </div>
                <p><i className="fa-solid fa-shield-heart" aria-hidden="true" /> Invite codes work for people connected to the same EduMind gateway. The embedded gateway is private to this device unless you intentionally use a shared authenticated gateway.</p>
              </div>

              {!isMember && (
                <div className="group-study-membership-prompt">
                  <div>
                    <strong>Join this room to contribute.</strong>
                    <span>Use your name to chat, share resources, or bring AI into the shared discussion.</span>
                  </div>
                  <button type="button" onClick={() => void joinActiveGroup()} disabled={!connected || isJoining || !memberName}>
                    <i className="fa-solid fa-user-plus" aria-hidden="true" /> {isJoining ? "Joining…" : "Join this room"}
                  </button>
                </div>
              )}

              <div className="group-study-room-grid">
                <section className="group-study-conversation-card">
                  <div className="group-study-section-heading">
                    <div>
                      <p className="eyebrow">Study conversation</p>
                      <h3>Explain, question, connect.</h3>
                    </div>
                    <span>{messages.length} message{messages.length === 1 ? "" : "s"}</span>
                  </div>
                  <div className="group-study-message-list">
                    {messages.length ? messages.map((message) => (
                      <article className={message.role === "ai" ? "group-study-message is-ai" : "group-study-message"} key={message.id}>
                        <div className="group-study-message-meta">
                          <span className="group-study-message-avatar">{message.role === "ai" ? <i className="fa-solid fa-sparkles" aria-hidden="true" /> : initials(message.author)}</span>
                          <strong>{message.author}</strong>
                          <em>{message.role === "ai" ? "AI facilitator" : "Student"}</em>
                          <time dateTime={message.created_at}>{formatTime(message.created_at)}</time>
                        </div>
                        <p>{message.content}</p>
                      </article>
                    )) : (
                      <div className="group-study-message-empty">
                        <i className="fa-solid fa-comment-dots" aria-hidden="true" />
                        <p>Start with a question, a short explanation, or the uncertainty your group wants to resolve.</p>
                      </div>
                    )}
                  </div>
                  <form className="group-study-message-form" onSubmit={sendMessage}>
                    <label htmlFor="group-study-message">
                      <span>Message to {activeGroup.title}</span>
                      <textarea
                        id="group-study-message"
                        value={messageDraft}
                        onChange={(event) => setMessageDraft(event.target.value)}
                        placeholder="Share an explanation, or ask EduMind AI about this shared discussion…"
                        rows="3"
                        maxLength="6000"
                        disabled={!connected || !isMember || isSending}
                      />
                    </label>
                    <div>
                      <small>{isMember ? "Shared messages become AI context only when a member explicitly asks EduMind AI." : "Join the room before posting."}</small>
                      <span className="group-study-message-actions">
                        <button type="button" className="secondary-button" onClick={() => void askAiFromConversation()} disabled={!connected || !isMember || isSending || isAskingAi || !messageDraft.trim()}>
                          <i className="fa-solid fa-sparkles" aria-hidden="true" /> {isAskingAi ? "Thinking…" : "Ask AI in chat"}
                        </button>
                        <button type="submit" disabled={!connected || !isMember || isSending || isAskingAi || !messageDraft.trim()}>
                          <i className="fa-solid fa-paper-plane" aria-hidden="true" /> {isSending ? "Sharing…" : "Share message"}
                        </button>
                      </span>
                    </div>
                  </form>
                </section>

                <aside className="group-study-sidecar">
                  <section className="group-study-side-card group-study-resource-card">
                    <div className="group-study-section-heading">
                      <div>
                        <p className="eyebrow">Shared resources</p>
                        <h3>One useful link at a time.</h3>
                      </div>
                      <span>{resources.length}</span>
                    </div>
                    {resources.length ? (
                      <ul className="group-study-resource-list">
                        {resources.map((resource) => (
                          <li key={resource.id}>
                            <i className="fa-solid fa-link" aria-hidden="true" />
                            <div>
                              <a href={resource.url} target="_blank" rel="noreferrer">{resource.title}</a>
                              {resource.description && <p>{resource.description}</p>}
                              <small>Shared by {resource.author}</small>
                            </div>
                          </li>
                        ))}
                      </ul>
                    ) : <p className="group-study-side-empty">Share a trustworthy HTTPS link your group can open from any device.</p>}
                    <form className="group-study-resource-form" onSubmit={shareResource}>
                      <label htmlFor="group-study-resource-title">
                        <span>Resource title</span>
                        <input
                          id="group-study-resource-title"
                          value={resourceDraft.title}
                          onChange={(event) => setResourceDraft((current) => ({ ...current, title: event.target.value }))}
                          placeholder="Open calculus notes"
                          maxLength="160"
                          disabled={!connected || !isMember || isSharing}
                        />
                      </label>
                      <label htmlFor="group-study-resource-url">
                        <span>HTTPS resource link</span>
                        <input
                          id="group-study-resource-url"
                          type="url"
                          value={resourceDraft.url}
                          onChange={(event) => setResourceDraft((current) => ({ ...current, url: event.target.value }))}
                          placeholder="https://…"
                          maxLength="2048"
                          disabled={!connected || !isMember || isSharing}
                        />
                      </label>
                      <label htmlFor="group-study-resource-description">
                        <span>Why it matters <small>optional</small></span>
                        <textarea
                          id="group-study-resource-description"
                          value={resourceDraft.description}
                          onChange={(event) => setResourceDraft((current) => ({ ...current, description: event.target.value }))}
                          placeholder="Read section 2 before the call."
                          rows="2"
                          maxLength="600"
                          disabled={!connected || !isMember || isSharing}
                        />
                      </label>
                      <button type="submit" className="secondary-button" disabled={!connected || !isMember || isSharing}>
                        <i className="fa-solid fa-share-nodes" aria-hidden="true" /> {isSharing ? "Sharing…" : "Share resource"}
                      </button>
                    </form>
                  </section>

                  <section className="group-study-side-card group-study-ai-card">
                    <div className="group-study-section-heading">
                      <div>
                        <p className="eyebrow">EduMind AI</p>
                        <h3>Bring context into the chat.</h3>
                      </div>
                      <i className="fa-solid fa-sparkles" aria-hidden="true" />
                    </div>
                    <p>Ask the configured LLM to read the current shared discussion, explain a disagreement, turn it into a short plan, or identify what evidence the group still needs.</p>
                    <form onSubmit={askAiFacilitator}>
                      <label htmlFor="group-study-ai-prompt">
                        <span>Ask AI facilitator</span>
                        <textarea
                          id="group-study-ai-prompt"
                          value={aiPrompt}
                          onChange={(event) => setAiPrompt(event.target.value)}
                          placeholder="Help us decide the next 25-minute study step."
                          rows="4"
                          maxLength="3000"
                          disabled={!connected || !isMember || isAskingAi}
                        />
                      </label>
                      <button type="submit" disabled={!connected || !isMember || isAskingAi || !aiPrompt.trim()}>
                        <i className="fa-solid fa-wand-magic-sparkles" aria-hidden="true" /> {isAskingAi ? "Facilitating…" : "Ask EduMind AI"}
                      </button>
                    </form>
                    <small><i className="fa-solid fa-shield-heart" aria-hidden="true" /> Your question and the AI reply appear in the shared chat. The AI uses the room's saved discussion and link metadata within a safe context limit; it cannot invite people or take actions for the group.</small>
                  </section>
                </aside>
              </div>
            </>
          )}
        </section>
      </div>
    </section>
  );
}

function appendUnique(items, value) {
  const current = Array.isArray(items) ? items : [];
  return current.some((item) => item?.id === value?.id) ? current : [...current, value];
}

function normalizeName(value) {
  return String(value ?? "").trim().replace(/\s+/g, " ");
}

function initials(value) {
  const words = normalizeName(value).split(" ").filter(Boolean);
  return words.slice(0, 2).map((word) => word[0]?.toUpperCase() ?? "").join("") || "?";
}

function formatTime(value) {
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) {
    return "just now";
  }
  return date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

function formatDate(value) {
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) {
    return "recently";
  }
  return date.toLocaleDateString([], { month: "short", day: "numeric" });
}

function safeMessage(reason, fallback) {
  return reason instanceof Error && reason.message ? reason.message : fallback;
}
