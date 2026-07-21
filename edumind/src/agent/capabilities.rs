use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

use chrono::{DateTime, Utc};
use edumind_core::PipelineRunId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::infra::{EduMindError, Result};

/// A signed, in-memory least-privilege grant for one agent run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilityGrant {
    pub id: Uuid,
    pub run_id: PipelineRunId,
    pub agent_id: String,
    pub module_id: String,
    #[serde(default)]
    pub allowed_tools: BTreeSet<String>,
    #[serde(default)]
    pub allowed_write_roots: Vec<PathBuf>,
    #[serde(default)]
    pub allow_mutation: bool,
    pub expires_at: DateTime<Utc>,
    pub signature: String,
}

/// Input used to mint a capability grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityGrantRequest {
    pub run_id: PipelineRunId,
    pub agent_id: String,
    pub module_id: String,
    pub allowed_tools: BTreeSet<String>,
    pub allowed_write_roots: Vec<PathBuf>,
    pub allow_mutation: bool,
    pub expires_at: DateTime<Utc>,
}

/// A proposed tool invocation checked against a capability grant.
#[derive(Clone, Debug)]
pub struct CapabilityCheck<'a> {
    pub run_id: PipelineRunId,
    pub agent_id: &'a str,
    pub module_id: &'a str,
    pub tool_name: &'a str,
    pub write_path: Option<&'a Path>,
    pub mutating: bool,
}

/// Per-process signer and validator for capability grants.
#[derive(Clone, Debug)]
pub struct CapabilityAuthority {
    secret: [u8; 32],
}

impl Default for CapabilityAuthority {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilityAuthority {
    /// Creates a fresh signer whose grants become invalid when the process restarts.
    #[must_use]
    pub fn new() -> Self {
        let mut hasher = Sha256::new();
        hasher.update(Uuid::new_v4().as_bytes());
        hasher.update(Uuid::new_v4().as_bytes());
        let digest = hasher.finalize();
        let mut secret = [0_u8; 32];
        secret.copy_from_slice(&digest);
        Self { secret }
    }

    /// Mints a signed grant after validating its least-privilege fields.
    pub fn issue(&self, request: CapabilityGrantRequest) -> Result<CapabilityGrant> {
        validate_grant_request(&request)?;
        let mut grant = CapabilityGrant {
            id: Uuid::new_v4(),
            run_id: request.run_id,
            agent_id: request.agent_id,
            module_id: request.module_id,
            allowed_tools: request.allowed_tools,
            allowed_write_roots: request.allowed_write_roots,
            allow_mutation: request.allow_mutation,
            expires_at: request.expires_at,
            signature: String::new(),
        };
        grant.signature = self.sign(&grant)?;
        Ok(grant)
    }

    /// Validates an optional grant and denies every request without one.
    pub fn authorize(
        &self,
        grant: Option<&CapabilityGrant>,
        check: &CapabilityCheck<'_>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let grant = grant.ok_or_else(|| {
            EduMindError::Security("capability grant is required for this run action".to_owned())
        })?;
        self.verify_signature(grant)?;
        if grant.expires_at <= now {
            return Err(EduMindError::Security(
                "capability grant has expired".to_owned(),
            ));
        }
        if grant.run_id != check.run_id
            || grant.agent_id != check.agent_id
            || grant.module_id != check.module_id
        {
            return Err(EduMindError::Security(
                "capability grant does not match the run, agent, or module".to_owned(),
            ));
        }
        if !grant.allowed_tools.contains(check.tool_name) {
            return Err(EduMindError::Security(format!(
                "tool '{}' is not granted for this run",
                check.tool_name
            )));
        }
        if check.mutating && !grant.allow_mutation {
            return Err(EduMindError::Security(
                "capability grant does not permit mutation".to_owned(),
            ));
        }
        if let Some(path) = check.write_path
            && !grant
                .allowed_write_roots
                .iter()
                .any(|root| path_is_within(root, path))
        {
            return Err(EduMindError::Security(
                "capability grant does not permit the requested write path".to_owned(),
            ));
        }
        Ok(())
    }

    fn verify_signature(&self, grant: &CapabilityGrant) -> Result<()> {
        let expected = self.sign(grant)?;
        if expected
            .as_bytes()
            .ct_eq(grant.signature.as_bytes())
            .unwrap_u8()
            != 1
        {
            return Err(EduMindError::Security(
                "capability grant signature is invalid".to_owned(),
            ));
        }
        Ok(())
    }

    fn sign(&self, grant: &CapabilityGrant) -> Result<String> {
        let payload = CapabilityGrantPayload::from(grant);
        let payload = serde_json::to_vec(&payload)?;
        let mut hasher = Sha256::new();
        hasher.update(self.secret);
        hasher.update(payload);
        Ok(hex_encode(&hasher.finalize()))
    }
}

#[derive(Serialize)]
struct CapabilityGrantPayload<'a> {
    id: Uuid,
    run_id: PipelineRunId,
    agent_id: &'a str,
    module_id: &'a str,
    allowed_tools: &'a BTreeSet<String>,
    allowed_write_roots: &'a [PathBuf],
    allow_mutation: bool,
    expires_at: DateTime<Utc>,
}

impl<'a> From<&'a CapabilityGrant> for CapabilityGrantPayload<'a> {
    fn from(grant: &'a CapabilityGrant) -> Self {
        Self {
            id: grant.id,
            run_id: grant.run_id,
            agent_id: &grant.agent_id,
            module_id: &grant.module_id,
            allowed_tools: &grant.allowed_tools,
            allowed_write_roots: &grant.allowed_write_roots,
            allow_mutation: grant.allow_mutation,
            expires_at: grant.expires_at,
        }
    }
}

fn validate_grant_request(request: &CapabilityGrantRequest) -> Result<()> {
    if request.agent_id.trim().is_empty() || request.module_id.trim().is_empty() {
        return Err(EduMindError::Security(
            "capability grants require non-empty agent and module IDs".to_owned(),
        ));
    }
    if request
        .allowed_tools
        .iter()
        .any(|tool| tool.trim().is_empty())
    {
        return Err(EduMindError::Security(
            "capability grants cannot contain blank tool names".to_owned(),
        ));
    }
    if request
        .allowed_write_roots
        .iter()
        .any(|root| !root.is_absolute())
    {
        return Err(EduMindError::Security(
            "capability write roots must be absolute paths".to_owned(),
        ));
    }
    Ok(())
}

fn path_is_within(root: &Path, candidate: &Path) -> bool {
    if !root.is_absolute() || !candidate.is_absolute() {
        return false;
    }
    normalize_path(candidate).starts_with(normalize_path(root))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = normalized.pop();
            }
            Component::Normal(segment) => normalized.push(segment),
        }
    }
    normalized
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use chrono::{Duration, TimeZone, Utc};
    use edumind_core::PipelineRunId;

    use super::{CapabilityAuthority, CapabilityCheck, CapabilityGrantRequest};

    #[test]
    fn denies_missing_tampered_expired_and_out_of_scope_capabilities() {
        let authority = CapabilityAuthority::new();
        let now = Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0).unwrap();
        let run_id = PipelineRunId::new();
        let root = std::env::temp_dir().join("edumind-capabilities");
        let request = CapabilityGrantRequest {
            run_id,
            agent_id: "researcher".to_owned(),
            module_id: "research".to_owned(),
            allowed_tools: BTreeSet::from(["research_deep_ask".to_owned()]),
            allowed_write_roots: vec![root.clone()],
            allow_mutation: false,
            expires_at: now + Duration::minutes(5),
        };
        let grant = authority.issue(request).unwrap();
        let check = CapabilityCheck {
            run_id,
            agent_id: "researcher",
            module_id: "research",
            tool_name: "research_deep_ask",
            write_path: None,
            mutating: false,
        };

        assert!(authority.authorize(Some(&grant), &check, now).is_ok());
        assert!(authority.authorize(None, &check, now).is_err());
        assert!(
            authority
                .authorize(Some(&grant), &check, now + Duration::minutes(6))
                .is_err()
        );

        let mut tampered = grant.clone();
        tampered.allowed_tools.insert("write_file".to_owned());
        assert!(authority.authorize(Some(&tampered), &check, now).is_err());

        let outside = root.join("..").join("outside.txt");
        let path_check = CapabilityCheck {
            write_path: Some(&outside),
            mutating: true,
            ..check
        };
        assert!(authority.authorize(Some(&grant), &path_check, now).is_err());
    }
}
