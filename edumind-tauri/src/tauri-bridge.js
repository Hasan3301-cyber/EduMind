import { invoke } from "@tauri-apps/api/core";

const browserEndpoint = {
  baseUrl: import.meta.env.VITE_GATEWAY_URL?.replace(/\/$/, "") ?? "",
  token: import.meta.env.VITE_GATEWAY_TOKEN ?? "",
  embedded: false
};

export async function getGatewayEndpoint() {
  const testEndpoint = getDevelopmentTestEndpoint();
  if (testEndpoint) {
    return testEndpoint;
  }
  if (!isTauriDesktop()) {
    return browserEndpoint;
  }
  const endpoint = await invoke("get_gateway_endpoint");
  return {
    ...endpoint,
    embedded: true
  };
}

export async function getLlmProviderSettings() {
  if (!isTauriDesktop()) {
    return null;
  }
  return invoke("get_llm_provider_settings");
}

export async function saveLlmProviderSettings(input) {
  if (!isTauriDesktop()) {
    throw new Error("LLM provider settings are available only in the EduMind desktop app.");
  }
  return invoke("save_llm_provider_settings", { input });
}

export async function testLlmProviderConnection() {
  if (!isTauriDesktop()) {
    throw new Error("LLM provider settings are available only in the EduMind desktop app.");
  }
  return invoke("test_llm_provider_connection");
}

export async function clearLlmProviderApiKey() {
  if (!isTauriDesktop()) {
    throw new Error("LLM provider settings are available only in the EduMind desktop app.");
  }
  return invoke("clear_llm_provider_api_key");
}

export async function getMeetMindSyncSettings() {
  if (!isTauriDesktop()) {
    return null;
  }
  return invoke("get_meetmind_sync_settings");
}

export async function saveMeetMindSyncSettings(input) {
  if (!isTauriDesktop()) {
    throw new Error("MeetMind sync settings are available only in the EduMind desktop app.");
  }
  return invoke("save_meetmind_sync_settings", { input });
}

export async function clearMeetMindSyncApiKey() {
  if (!isTauriDesktop()) {
    throw new Error("MeetMind sync settings are available only in the EduMind desktop app.");
  }
  return invoke("clear_meetmind_sync_api_key");
}

export async function fetchMeetMindTranscripts() {
  if (!isTauriDesktop()) {
    throw new Error("MeetMind transcript inbox is available only in the EduMind desktop app.");
  }
  return invoke("fetch_meetmind_transcripts");
}

export async function importMeetMindTranscript(input) {
  if (!isTauriDesktop()) {
    throw new Error("MeetMind transcript inbox is available only in the EduMind desktop app.");
  }
  return invoke("import_meetmind_transcript", { input });
}

export async function getAgentManagementSettings() {
  if (!isTauriDesktop()) {
    return null;
  }
  return invoke("get_agent_management_settings");
}

export async function saveAgentManagementSettings(input) {
  if (!isTauriDesktop()) {
    throw new Error("Agent management is available only in the EduMind desktop app.");
  }
  return invoke("save_agent_management_settings", { input });
}

export async function getAgentSandboxDocuments(agentId) {
  if (!isTauriDesktop()) {
    throw new Error("Sandbox documents are available only in the EduMind desktop app.");
  }
  return invoke("get_agent_sandbox_documents", { agentId });
}

export async function saveAgentSandboxDocuments(input) {
  if (!isTauriDesktop()) {
    throw new Error("Sandbox documents are available only in the EduMind desktop app.");
  }
  return invoke("save_agent_sandbox_documents", { input });
}

function isTauriDesktop() {
  return typeof window !== "undefined" && Boolean(window.__TAURI_INTERNALS__);
}

function getDevelopmentTestEndpoint() {
  if (!import.meta.env.DEV || typeof window === "undefined") {
    return null;
  }
  const endpoint = window.__EDUMIND_TEST_GATEWAY_ENDPOINT__;
  if (!endpoint || typeof endpoint.baseUrl !== "string" || !endpoint.baseUrl.trim()) {
    return null;
  }
  return {
    baseUrl: endpoint.baseUrl.replace(/\/$/, ""),
    token: typeof endpoint.token === "string" ? endpoint.token : "",
    embedded: false
  };
}
