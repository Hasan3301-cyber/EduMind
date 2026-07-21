use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::Utc;
use edumind::{
    gateway::AppState,
    memory::{ModuleMemoryScope, NewModuleMemory},
    security::secrets::KeyringSecretStore,
};
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use serde_json::json;
use url::Url;

const KEYCHAIN_SERVICE: &str = "edumind.desktop";
const KEYCHAIN_SECRET_NAME: &str = "meetmind-supabase-anon-key";
const SETTINGS_FILE_NAME: &str = "meetmind-sync.json";
const IMPORT_LOG_FILE_NAME: &str = "meetmind-imports.json";
const MAX_TRANSCRIPTS: usize = 50;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_TRANSCRIPT_CHARS: usize = 80_000;
const MAX_API_KEY_CHARS: usize = 8_192;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetMindSyncInput {
    pub supabase_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetMindImportInput {
    pub id: String,
    #[serde(default)]
    pub delete_remote: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedMeetMindSync {
    pub supabase_url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetMindSyncStatus {
    pub supabase_url: String,
    pub api_key_configured: bool,
    pub configured: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetMindTranscript {
    pub id: String,
    pub created_at: String,
    pub preview: String,
    pub character_count: usize,
    pub importable: bool,
    pub issue: Option<String>,
    pub already_imported: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetMindImportResult {
    pub id: String,
    pub memory_id: String,
    pub remote_deleted: bool,
    pub remote_delete_failed: bool,
    pub local_index_recorded: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct SupabaseMeeting {
    id: String,
    transcript: String,
    #[serde(default)]
    created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ImportedTranscript {
    remote_id: String,
    local_memory_id: String,
    imported_at: String,
}

pub fn normalize_input(
    input: MeetMindSyncInput,
) -> Result<(PersistedMeetMindSync, Option<String>), String> {
    let profile = PersistedMeetMindSync {
        supabase_url: normalize_supabase_url(&input.supabase_url)?,
    };
    let api_key = input
        .api_key
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty());
    if let Some(api_key) = &api_key
        && !is_supported_api_key(api_key)
    {
        return Err("Enter a Supabase key without control characters.".to_owned());
    }
    Ok((profile, api_key))
}

pub fn load_profile(data_dir: &Path) -> Result<Option<PersistedMeetMindSync>, String> {
    let path = settings_path(data_dir);
    if !path.exists() {
        return Ok(None);
    }
    let bytes =
        fs::read(path).map_err(|error| format!("could not read MeetMind settings: {error}"))?;
    let profile = serde_json::from_slice::<PersistedMeetMindSync>(&bytes)
        .map_err(|error| format!("could not read MeetMind settings: {error}"))?;
    Ok(Some(PersistedMeetMindSync {
        supabase_url: normalize_supabase_url(&profile.supabase_url)?,
    }))
}

pub fn save_profile(data_dir: &Path, profile: &PersistedMeetMindSync) -> Result<(), String> {
    fs::create_dir_all(data_dir)
        .map_err(|error| format!("could not create the settings directory: {error}"))?;
    let bytes = serde_json::to_vec_pretty(profile)
        .map_err(|error| format!("could not save MeetMind settings: {error}"))?;
    fs::write(settings_path(data_dir), bytes)
        .map_err(|error| format!("could not save MeetMind settings: {error}"))
}

pub fn status(data_dir: &Path) -> Result<MeetMindSyncStatus, String> {
    let profile = load_profile(data_dir)?;
    let api_key_configured = load_api_key()?.is_some_and(|key| !key.trim().is_empty());
    Ok(match profile {
        Some(profile) => MeetMindSyncStatus {
            supabase_url: profile.supabase_url,
            api_key_configured,
            configured: api_key_configured,
        },
        None => MeetMindSyncStatus {
            supabase_url: String::new(),
            api_key_configured,
            configured: false,
        },
    })
}

pub fn store_api_key(api_key: &str) -> Result<(), String> {
    secret_store()?
        .set(KEYCHAIN_SECRET_NAME, api_key)
        .map_err(|error| {
            format!("could not store the MeetMind key in the native keychain: {error}")
        })
}

pub fn clear_api_key() -> Result<bool, String> {
    secret_store()?
        .delete(KEYCHAIN_SECRET_NAME)
        .map_err(|error| {
            format!("could not clear the MeetMind key from the native keychain: {error}")
        })
}

pub fn load_api_key() -> Result<Option<String>, String> {
    let secret = secret_store()?
        .get(KEYCHAIN_SECRET_NAME)
        .map_err(|error| format!("could not access the native keychain: {error}"))?;
    Ok(secret.map(|value| value.expose().to_owned()))
}

pub async fn list_transcripts(data_dir: &Path) -> Result<Vec<MeetMindTranscript>, String> {
    let profile = configured_profile(data_dir)?;
    let imported = imported_ids(data_dir)?;
    let meetings = request_meetings(&profile, None).await?;
    Ok(meetings
        .into_iter()
        .map(|meeting| {
            let already_imported = imported.contains(&meeting.id);
            transcript_summary(meeting, already_imported)
        })
        .collect())
}

pub async fn import_transcript(
    data_dir: &Path,
    state: &AppState,
    input: MeetMindImportInput,
) -> Result<MeetMindImportResult, String> {
    let id = normalize_remote_id(&input.id)?;
    let mut imports = load_imports(data_dir)?;
    if imports.iter().any(|record| record.remote_id == id) {
        return Err("This MeetMind transcript is already in local Class Notes memory.".to_owned());
    }

    let profile = configured_profile(data_dir)?;
    let meeting = request_meetings(&profile, Some(&id))
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| {
            "This MeetMind transcript is no longer available in the inbox.".to_owned()
        })?;
    validate_transcript(&meeting.transcript)?;

    let imported_at = Utc::now();
    let result = state
        .memory_intelligence()
        .ingest(
            "class-notes",
            NewModuleMemory {
                content: meeting.transcript.trim().to_owned(),
                content_type: "transcript".to_owned(),
                scope: ModuleMemoryScope::Module,
                metadata: json!({
                    "source": "meetmind",
                    "remote_id": id,
                    "created_at": meeting.created_at,
                    "imported_at": imported_at,
                }),
            },
            imported_at,
        )
        .await
        .map_err(|_| {
            "EduMind could not import this transcript into Class Notes memory.".to_owned()
        })?;

    let local_memory_id = result.record.id.to_string();
    imports.push(ImportedTranscript {
        remote_id: id.clone(),
        local_memory_id: local_memory_id.clone(),
        imported_at: imported_at.to_rfc3339(),
    });
    let local_index_recorded = save_imports(data_dir, &imports).is_ok();

    let remote_deleted = if input.delete_remote {
        delete_meeting(&profile, &id).await.is_ok()
    } else {
        false
    };
    Ok(MeetMindImportResult {
        id,
        memory_id: local_memory_id,
        remote_deleted,
        remote_delete_failed: input.delete_remote && !remote_deleted,
        local_index_recorded,
    })
}

fn configured_profile(data_dir: &Path) -> Result<PersistedMeetMindSync, String> {
    let profile = load_profile(data_dir)?
        .ok_or_else(|| "Save MeetMind Supabase settings before opening the inbox.".to_owned())?;
    let _ = configured_api_key()?;
    Ok(profile)
}

async fn request_meetings(
    profile: &PersistedMeetMindSync,
    id: Option<&str>,
) -> Result<Vec<SupabaseMeeting>, String> {
    let mut url = meetings_url(&profile.supabase_url)?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("select", "id,transcript,created_at");
        if let Some(id) = id {
            query.append_pair("id", &format!("eq.{id}"));
            query.append_pair("limit", "1");
        } else {
            query.append_pair("order", "created_at.asc");
            query.append_pair("limit", &MAX_TRANSCRIPTS.to_string());
        }
    }
    let response = request(client()?.get(url)).await?;
    let bytes = bounded_response(response, "MeetMind inbox").await?;
    serde_json::from_slice::<Vec<SupabaseMeeting>>(&bytes)
        .map_err(|_| "MeetMind inbox returned unreadable transcript data.".to_owned())
}

async fn delete_meeting(profile: &PersistedMeetMindSync, id: &str) -> Result<(), String> {
    let mut url = meetings_url(&profile.supabase_url)?;
    url.query_pairs_mut().append_pair("id", &format!("eq.{id}"));
    let response = request(client()?.delete(url)).await?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err("The remote MeetMind transcript could not be removed.".to_owned())
    }
}

fn client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|_| "EduMind could not prepare the MeetMind inbox request.".to_owned())
}

async fn request(request: reqwest::RequestBuilder) -> Result<reqwest::Response, String> {
    let api_key = configured_api_key()?;
    request
        .header("apikey", &api_key)
        .header(header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| {
            "EduMind could not reach the MeetMind inbox. Check the URL and connection.".to_owned()
        })
        .and_then(|response| {
            if response.status().is_success() {
                Ok(response)
            } else {
                Err(format!(
                    "MeetMind inbox request failed with HTTP {}.",
                    response.status().as_u16()
                ))
            }
        })
}

async fn bounded_response(response: reqwest::Response, source: &str) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(format!("{source} returned too much data."));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| format!("{source} response could not be read."))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(format!("{source} returned too much data."));
    }
    Ok(bytes.to_vec())
}

fn transcript_summary(meeting: SupabaseMeeting, already_imported: bool) -> MeetMindTranscript {
    let character_count = meeting.transcript.chars().count();
    let issue = if meeting.id.trim().is_empty() {
        Some("This inbox item has no usable identifier.".to_owned())
    } else if meeting.transcript.trim().is_empty() {
        Some("This inbox item has no transcript text.".to_owned())
    } else if character_count > MAX_TRANSCRIPT_CHARS {
        Some("This transcript exceeds the import safety limit.".to_owned())
    } else {
        None
    };
    MeetMindTranscript {
        id: meeting.id,
        created_at: meeting.created_at,
        preview: preview(&meeting.transcript),
        character_count,
        importable: issue.is_none(),
        issue,
        already_imported,
    }
}

fn validate_transcript(content: &str) -> Result<(), String> {
    let characters = content.trim().chars().count();
    if characters == 0 {
        return Err("The MeetMind transcript is empty.".to_owned());
    }
    if characters > MAX_TRANSCRIPT_CHARS {
        return Err("This transcript exceeds EduMind's import safety limit.".to_owned());
    }
    Ok(())
}

fn normalize_supabase_url(value: &str) -> Result<String, String> {
    let mut url =
        Url::parse(value.trim()).map_err(|_| "The Supabase URL is invalid.".to_owned())?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err("MeetMind Supabase must use an HTTPS URL.".to_owned());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("The Supabase URL contains unsupported components.".to_owned());
    }
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&path);
    Ok(url.to_string().trim_end_matches('/').to_owned())
}

fn meetings_url(base_url: &str) -> Result<Url, String> {
    let mut url =
        Url::parse(base_url).map_err(|_| "The saved Supabase URL is invalid.".to_owned())?;
    let path = url.path().trim_end_matches('/');
    url.set_path(&format!("{path}/rest/v1/meetings"));
    Ok(url)
}

fn normalize_remote_id(value: &str) -> Result<String, String> {
    let id = value.trim();
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("The selected MeetMind transcript identifier is invalid.".to_owned());
    }
    Ok(id.to_owned())
}

fn is_supported_api_key(value: &str) -> bool {
    value.len() <= MAX_API_KEY_CHARS && value.is_ascii() && !value.chars().any(char::is_control)
}

fn configured_api_key() -> Result<String, String> {
    let api_key = load_api_key()?
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| "Save a MeetMind Supabase key before opening the inbox.".to_owned())?;
    if !is_supported_api_key(&api_key) {
        return Err(
            "The stored MeetMind Supabase key is invalid. Save it again in Class Notes.".to_owned(),
        );
    }
    Ok(api_key)
}

fn preview(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() > 360 {
        normalized.chars().take(357).collect::<String>() + "…"
    } else {
        normalized
    }
}

fn imported_ids(data_dir: &Path) -> Result<HashSet<String>, String> {
    Ok(load_imports(data_dir)?
        .into_iter()
        .map(|record| record.remote_id)
        .collect())
}

fn load_imports(data_dir: &Path) -> Result<Vec<ImportedTranscript>, String> {
    let path = import_log_path(data_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes =
        fs::read(path).map_err(|error| format!("could not read MeetMind import state: {error}"))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("could not read MeetMind import state: {error}"))
}

fn save_imports(data_dir: &Path, imports: &[ImportedTranscript]) -> Result<(), String> {
    fs::create_dir_all(data_dir)
        .map_err(|error| format!("could not create the settings directory: {error}"))?;
    let bytes = serde_json::to_vec_pretty(imports)
        .map_err(|error| format!("could not save MeetMind import state: {error}"))?;
    fs::write(import_log_path(data_dir), bytes)
        .map_err(|error| format!("could not save MeetMind import state: {error}"))
}

fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SETTINGS_FILE_NAME)
}

fn import_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join(IMPORT_LOG_FILE_NAME)
}

fn secret_store() -> Result<KeyringSecretStore, String> {
    KeyringSecretStore::new(KEYCHAIN_SERVICE)
        .map_err(|error| format!("could not initialize native keychain storage: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{MeetMindSyncInput, normalize_input, normalize_remote_id, preview};

    #[test]
    fn accepts_https_supabase_settings_without_exposing_keys() {
        let (profile, key) = normalize_input(MeetMindSyncInput {
            supabase_url: "https://example.supabase.co/".to_owned(),
            api_key: Some("anon-key".to_owned()),
        })
        .unwrap();

        assert_eq!(profile.supabase_url, "https://example.supabase.co");
        assert_eq!(key.as_deref(), Some("anon-key"));
    }

    #[test]
    fn rejects_insecure_urls_and_invalid_remote_ids() {
        assert!(
            normalize_input(MeetMindSyncInput {
                supabase_url: "http://example.supabase.co".to_owned(),
                api_key: None,
            })
            .is_err()
        );
        assert!(normalize_remote_id("record with spaces").is_err());
    }

    #[test]
    fn truncates_inbox_previews_at_character_boundaries() {
        let preview = preview(&"é".repeat(500));

        assert!(preview.ends_with('…'));
        assert!(preview.chars().count() <= 360);
    }
}
