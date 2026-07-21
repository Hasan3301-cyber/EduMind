mod agent_management;
mod llm_settings;
mod meetmind_sync;

use std::{fs, path::PathBuf, sync::Mutex};

use edumind::{
    config::{EduMindConfig, types::AuthMode},
    gateway::{AppState, bind_listener, serve_with_shutdown},
};
use serde::Serialize;
use tauri::{Manager, State};
use tokio::sync::oneshot;
use uuid::Uuid;

#[cfg(debug_assertions)]
const EMBEDDED_GATEWAY_ORIGINS: [&str; 3] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "http://127.0.0.1:1420",
];

#[cfg(not(debug_assertions))]
const EMBEDDED_GATEWAY_ORIGINS: [&str; 2] = ["tauri://localhost", "http://tauri.localhost"];

fn embedded_gateway_origins() -> Vec<String> {
    EMBEDDED_GATEWAY_ORIGINS
        .iter()
        .map(|origin| (*origin).to_owned())
        .collect()
}

/// Connection details delivered only to the embedded frontend for this application launch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayEndpoint {
    pub base_url: String,
    pub token: String,
}

/// Owns the embedded loopback gateway and its one-shot graceful shutdown signal.
pub struct EmbeddedGateway {
    endpoint: GatewayEndpoint,
    state: AppState,
    data_dir: PathBuf,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
}

impl EmbeddedGateway {
    async fn start<R: tauri::Runtime>(app: &tauri::App<R>) -> Result<Self, String> {
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("could not resolve application data directory: {error}"))?;
        Self::start_in_data_dir(data_dir).await
    }

    async fn start_in_data_dir(data_dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&data_dir)
            .map_err(|error| format!("could not create application data directory: {error}"))?;

        let token = Uuid::new_v4().to_string();
        let mut config = EduMindConfig::default();
        config.meta.data_dir = data_dir.clone();
        config.memory.db_path = data_dir.join("memory.db");
        config.security.action_password_hash_path = data_dir.join("action-password.argon2");
        config.security.allowed_tool_write_roots =
            vec![data_dir.join("OUTPUT"), data_dir.join("scratch")];
        config.security.allowed_origins = embedded_gateway_origins();
        config.gateway.bind_address = "127.0.0.1".to_owned();
        config.gateway.port = 0;
        config.gateway.auth.mode = AuthMode::Token;
        config.gateway.auth.token = Some(token.clone());
        let agent_profile = agent_management::load_profile(&data_dir)?;
        agent_management::apply_profile(&mut config, &data_dir, &agent_profile)?;
        if let Some(profile) = llm_settings::load_profile(&data_dir)? {
            let api_key = llm_settings::load_api_key()?;
            llm_settings::apply_profile(&mut config, &profile, api_key);
        }
        config
            .validate()
            .map_err(|error| format!("embedded gateway configuration is invalid: {error}"))?;

        let listener = bind_listener(&config.gateway)
            .await
            .map_err(|error| format!("could not bind embedded gateway: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("could not read embedded gateway address: {error}"))?;
        let state = AppState::new(config)
            .map_err(|error| format!("could not initialize embedded gateway: {error}"))?;
        let (shutdown, shutdown_signal) = oneshot::channel();
        let server_state = state.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = serve_with_shutdown(listener, server_state, async move {
                let _ = shutdown_signal.await;
            })
            .await
            {
                eprintln!("EduMind embedded gateway stopped unexpectedly: {error}");
            }
        });
        Ok(Self {
            endpoint: GatewayEndpoint {
                base_url: format!("http://{address}"),
                token,
            },
            state,
            data_dir,
            shutdown: Mutex::new(Some(shutdown)),
        })
    }

    fn endpoint(&self) -> GatewayEndpoint {
        self.endpoint.clone()
    }

    fn llm_provider_status(&self) -> Result<llm_settings::LlmProviderStatus, String> {
        llm_settings::status(&self.data_dir)
    }

    fn save_llm_provider(
        &self,
        input: llm_settings::LlmProviderInput,
    ) -> Result<llm_settings::LlmProviderStatus, String> {
        let (profile, new_api_key) = llm_settings::normalize_input(input)?;
        let active_api_key = match new_api_key.as_deref() {
            Some(api_key) => Some(api_key.to_owned()),
            None => llm_settings::load_api_key()?,
        };
        let mut config = self
            .state
            .config_snapshot()
            .map_err(|error| format!("could not update LLM settings: {error}"))?;
        llm_settings::apply_profile(&mut config, &profile, active_api_key);
        config
            .validate()
            .map_err(|error| format!("LLM settings are invalid: {error}"))?;

        if let Some(api_key) = new_api_key.as_deref() {
            llm_settings::store_api_key(api_key)?;
        }
        llm_settings::save_profile(&self.data_dir, &profile)?;
        self.state
            .replace_config(config)
            .map_err(|error| format!("could not activate LLM settings: {error}"))?;
        self.llm_provider_status()
    }

    fn clear_llm_api_key(&self) -> Result<llm_settings::LlmProviderStatus, String> {
        llm_settings::clear_api_key()?;
        if let Some(profile) = llm_settings::load_profile(&self.data_dir)? {
            let mut config = self
                .state
                .config_snapshot()
                .map_err(|error| format!("could not update LLM settings: {error}"))?;
            llm_settings::apply_profile(&mut config, &profile, None);
            self.state
                .replace_config(config)
                .map_err(|error| format!("could not activate LLM settings: {error}"))?;
        }
        self.llm_provider_status()
    }

    fn meetmind_sync_status(&self) -> Result<meetmind_sync::MeetMindSyncStatus, String> {
        meetmind_sync::status(&self.data_dir)
    }

    fn save_meetmind_sync(
        &self,
        input: meetmind_sync::MeetMindSyncInput,
    ) -> Result<meetmind_sync::MeetMindSyncStatus, String> {
        let (profile, new_api_key) = meetmind_sync::normalize_input(input)?;
        if let Some(api_key) = new_api_key.as_deref() {
            meetmind_sync::store_api_key(api_key)?;
        }
        meetmind_sync::save_profile(&self.data_dir, &profile)?;
        self.meetmind_sync_status()
    }

    fn clear_meetmind_api_key(&self) -> Result<meetmind_sync::MeetMindSyncStatus, String> {
        meetmind_sync::clear_api_key()?;
        self.meetmind_sync_status()
    }

    fn agent_management_status(&self) -> Result<agent_management::AgentManagementStatus, String> {
        agent_management::status(&self.data_dir)
    }

    fn save_agent_management(
        &self,
        input: agent_management::AgentManagementInput,
    ) -> Result<agent_management::AgentManagementStatus, String> {
        let profile = agent_management::normalize_input(input)?;
        let mut config = self
            .state
            .config_snapshot()
            .map_err(|error| format!("could not update agent management: {error}"))?;
        agent_management::apply_profile(&mut config, &self.data_dir, &profile)?;
        config
            .validate()
            .map_err(|error| format!("agent management settings are invalid: {error}"))?;
        agent_management::ensure_agent_workspaces(&self.data_dir, &profile)?;
        agent_management::save_profile(&self.data_dir, &profile)?;
        self.state
            .replace_config(config)
            .map_err(|error| format!("could not activate agent management: {error}"))?;
        self.agent_management_status()
    }

    fn agent_sandbox_documents(
        &self,
        agent_id: String,
    ) -> Result<agent_management::AgentSandboxDocuments, String> {
        agent_management::load_documents(&self.data_dir, &agent_id)
    }

    fn save_agent_sandbox_documents(
        &self,
        input: agent_management::AgentSandboxDocumentsInput,
    ) -> Result<agent_management::AgentSandboxDocuments, String> {
        agent_management::save_documents(&self.data_dir, input)
    }

    fn stop(&self) {
        let shutdown = self
            .shutdown
            .lock()
            .ok()
            .and_then(|mut signal| signal.take());
        if let Some(shutdown) = shutdown {
            let _ = shutdown.send(());
        }
    }
}

/// Returns the authenticated local endpoint selected for the current desktop launch.
#[tauri::command]
fn get_gateway_endpoint(gateway: State<'_, EmbeddedGateway>) -> GatewayEndpoint {
    gateway.endpoint()
}

#[tauri::command]
fn get_llm_provider_settings(
    gateway: State<'_, EmbeddedGateway>,
) -> Result<llm_settings::LlmProviderStatus, String> {
    gateway.llm_provider_status()
}

#[tauri::command]
fn save_llm_provider_settings(
    gateway: State<'_, EmbeddedGateway>,
    input: llm_settings::LlmProviderInput,
) -> Result<llm_settings::LlmProviderStatus, String> {
    gateway.save_llm_provider(input)
}

#[tauri::command]
fn clear_llm_provider_api_key(
    gateway: State<'_, EmbeddedGateway>,
) -> Result<llm_settings::LlmProviderStatus, String> {
    gateway.clear_llm_api_key()
}

#[tauri::command]
async fn test_llm_provider_connection(
    gateway: State<'_, EmbeddedGateway>,
) -> Result<llm_settings::LlmProviderConnectionCheck, String> {
    let data_dir = gateway.data_dir.clone();
    llm_settings::test_connection(&data_dir).await
}

#[tauri::command]
fn get_meetmind_sync_settings(
    gateway: State<'_, EmbeddedGateway>,
) -> Result<meetmind_sync::MeetMindSyncStatus, String> {
    gateway.meetmind_sync_status()
}

#[tauri::command]
fn save_meetmind_sync_settings(
    gateway: State<'_, EmbeddedGateway>,
    input: meetmind_sync::MeetMindSyncInput,
) -> Result<meetmind_sync::MeetMindSyncStatus, String> {
    gateway.save_meetmind_sync(input)
}

#[tauri::command]
fn clear_meetmind_sync_api_key(
    gateway: State<'_, EmbeddedGateway>,
) -> Result<meetmind_sync::MeetMindSyncStatus, String> {
    gateway.clear_meetmind_api_key()
}

#[tauri::command]
async fn fetch_meetmind_transcripts(
    gateway: State<'_, EmbeddedGateway>,
) -> Result<Vec<meetmind_sync::MeetMindTranscript>, String> {
    meetmind_sync::list_transcripts(&gateway.data_dir).await
}

#[tauri::command]
async fn import_meetmind_transcript(
    gateway: State<'_, EmbeddedGateway>,
    input: meetmind_sync::MeetMindImportInput,
) -> Result<meetmind_sync::MeetMindImportResult, String> {
    meetmind_sync::import_transcript(&gateway.data_dir, &gateway.state, input).await
}

#[tauri::command]
fn get_agent_management_settings(
    gateway: State<'_, EmbeddedGateway>,
) -> Result<agent_management::AgentManagementStatus, String> {
    gateway.agent_management_status()
}

#[tauri::command]
fn save_agent_management_settings(
    gateway: State<'_, EmbeddedGateway>,
    input: agent_management::AgentManagementInput,
) -> Result<agent_management::AgentManagementStatus, String> {
    gateway.save_agent_management(input)
}

#[tauri::command]
fn get_agent_sandbox_documents(
    gateway: State<'_, EmbeddedGateway>,
    agent_id: String,
) -> Result<agent_management::AgentSandboxDocuments, String> {
    gateway.agent_sandbox_documents(agent_id)
}

#[tauri::command]
fn save_agent_sandbox_documents(
    gateway: State<'_, EmbeddedGateway>,
    input: agent_management::AgentSandboxDocumentsInput,
) -> Result<agent_management::AgentSandboxDocuments, String> {
    gateway.save_agent_sandbox_documents(input)
}

/// Builds and runs the desktop application with an in-process authenticated gateway.
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let gateway = tauri::async_runtime::block_on(EmbeddedGateway::start(app))
                .map_err(std::io::Error::other)?;
            app.manage(gateway);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_gateway_endpoint,
            get_llm_provider_settings,
            save_llm_provider_settings,
            clear_llm_provider_api_key,
            test_llm_provider_connection,
            get_meetmind_sync_settings,
            save_meetmind_sync_settings,
            clear_meetmind_sync_api_key,
            fetch_meetmind_transcripts,
            import_meetmind_transcript,
            get_agent_management_settings,
            save_agent_management_settings,
            get_agent_sandbox_documents,
            save_agent_sandbox_documents
        ])
        .build(tauri::generate_context!())
        .expect("error while building EduMind desktop application");
    app.run(|app_handle, event| {
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            app_handle.state::<EmbeddedGateway>().stop();
        }
    });
}

#[cfg(test)]
mod tests {
    use std::{env, fs, time::Duration};

    use tokio::time::sleep;
    use uuid::Uuid;

    use super::{EmbeddedGateway, GatewayEndpoint, embedded_gateway_origins};

    #[test]
    fn endpoint_serialization_uses_frontend_field_names() {
        let endpoint = GatewayEndpoint {
            base_url: "http://127.0.0.1:1234".to_owned(),
            token: "token".to_owned(),
        };
        let value = serde_json::to_value(endpoint).unwrap();

        assert_eq!(value["baseUrl"], "http://127.0.0.1:1234");
    }

    #[test]
    fn embedded_gateway_allowlist_covers_desktop_origins() {
        let origins = embedded_gateway_origins();

        assert!(origins.iter().any(|origin| origin == "tauri://localhost"));
        assert!(
            origins
                .iter()
                .any(|origin| origin == "http://tauri.localhost")
        );
        #[cfg(debug_assertions)]
        assert!(
            origins
                .iter()
                .any(|origin| origin == "http://127.0.0.1:1420")
        );
    }

    #[test]
    fn embedded_gateway_starts_serves_health_and_stops() {
        tauri::async_runtime::block_on(async {
            let data_dir =
                env::temp_dir().join(format!("edumind-desktop-smoke-{}", Uuid::new_v4()));
            let gateway = EmbeddedGateway::start_in_data_dir(data_dir.clone())
                .await
                .expect("embedded gateway should start in an isolated data directory");
            let endpoint = gateway.endpoint();
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap();
            let response = client
                .get(format!("{}/health", endpoint.base_url))
                .send()
                .await
                .expect("embedded gateway health endpoint should accept requests");

            assert!(response.status().is_success());
            let payload: serde_json::Value = response.json().await.unwrap();
            assert_eq!(payload["status"], "ok");
            assert!(data_dir.join("memory.db").is_file());

            gateway.stop();
            let mut stopped = false;
            for _ in 0..40 {
                sleep(Duration::from_millis(50)).await;
                let probe = reqwest::Client::builder()
                    .timeout(Duration::from_millis(150))
                    .build()
                    .unwrap()
                    .get(format!("{}/health", endpoint.base_url))
                    .send()
                    .await;
                if probe.is_err() {
                    stopped = true;
                    break;
                }
            }
            assert!(
                stopped,
                "embedded gateway listener remained available after shutdown"
            );

            drop(gateway);
            fs::remove_dir_all(&data_dir)
                .expect("isolated embedded gateway data should be removable after shutdown");
        });
    }
}
