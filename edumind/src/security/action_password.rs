use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::{
    config::types::SecurityConfig,
    infra::{EduMindError, Result},
};

const ACTION_GRANT_TTL_MINUTES: i64 = 5;

/// A short-lived proof that a user completed the action-password check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionGrant {
    expires_at: DateTime<Utc>,
}

impl ActionGrant {
    /// Returns true while the grant remains valid for sensitive tool operations.
    #[must_use]
    pub fn is_valid(&self, now: DateTime<Utc>) -> bool {
        now < self.expires_at
    }
}

/// Whether a configured password hash is ready for sensitive actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionPasswordStatus {
    NotConfigured,
    Configured,
}

/// Argon2-backed local action-password verifier with a configurable hash location.
#[derive(Clone, Debug)]
pub struct ActionPasswordService {
    required: bool,
    hash_path: PathBuf,
    min_length: usize,
}

impl ActionPasswordService {
    /// Creates the service from the active security configuration.
    #[must_use]
    pub fn from_config(config: &SecurityConfig) -> Self {
        Self {
            required: config.action_password_required,
            hash_path: config.action_password_hash_path.clone(),
            min_length: config.action_password_min_length,
        }
    }

    /// Returns whether sensitive actions require a completed password check.
    #[must_use]
    pub fn is_required(&self) -> bool {
        self.required
    }

    /// Returns the stored-hash readiness without exposing its contents.
    pub fn status(&self) -> Result<ActionPasswordStatus> {
        match fs::metadata(&self.hash_path) {
            Ok(_) => Ok(ActionPasswordStatus::Configured),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                Ok(ActionPasswordStatus::NotConfigured)
            }
            Err(error) => Err(storage_error(&self.hash_path, error)),
        }
    }

    /// Sets a new Argon2id password hash after enforcing the configured minimum length.
    pub fn set_password(&self, password: &str) -> Result<()> {
        if password.chars().count() < self.min_length {
            return Err(EduMindError::Security(format!(
                "action password must contain at least {} characters",
                self.min_length
            )));
        }
        let salt = SaltString::encode_b64(Uuid::new_v4().as_bytes()).map_err(|error| {
            EduMindError::Security(format!("failed to create action-password salt: {error}"))
        })?;
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|error| {
                EduMindError::Security(format!("failed to hash action password: {error}"))
            })?
            .to_string();
        if let Some(parent) = self
            .hash_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| storage_error(parent, error))?;
        }
        fs::write(&self.hash_path, hash).map_err(|error| storage_error(&self.hash_path, error))?;
        set_owner_only_permissions(&self.hash_path)?;
        Ok(())
    }

    /// Verifies a password against the stored Argon2id hash without returning password data.
    pub fn verify(&self, password: &str) -> Result<bool> {
        let hash = match fs::read_to_string(&self.hash_path) {
            Ok(hash) => hash,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(storage_error(&self.hash_path, error)),
        };
        let parsed_hash = PasswordHash::new(hash.trim()).map_err(|error| {
            EduMindError::Security(format!("stored action-password hash is invalid: {error}"))
        })?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok())
    }

    /// Verifies a supplied password when required and returns a temporary mutation grant.
    pub fn grant(&self, password: Option<&str>, now: DateTime<Utc>) -> Result<ActionGrant> {
        self.authorize(password)?;
        Ok(ActionGrant {
            expires_at: now + Duration::minutes(ACTION_GRANT_TTL_MINUTES),
        })
    }

    /// Rejects a missing, unconfigured, or invalid password whenever the guard is required.
    pub fn authorize(&self, password: Option<&str>) -> Result<()> {
        if !self.required {
            return Ok(());
        }
        let password = password.filter(|value| !value.is_empty()).ok_or_else(|| {
            EduMindError::Security("an action password is required for this operation".to_owned())
        })?;
        if self.status()? == ActionPasswordStatus::NotConfigured {
            return Err(EduMindError::Security(
                "an action password must be configured before sensitive operations".to_owned(),
            ));
        }
        if !self.verify(password)? {
            return Err(EduMindError::Security(
                "action password is invalid".to_owned(),
            ));
        }
        Ok(())
    }
}

fn storage_error(path: &Path, source: std::io::Error) -> EduMindError {
    EduMindError::StorageIo {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| storage_error(path, error))
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use chrono::{Duration, Utc};
    use uuid::Uuid;

    use super::{ActionPasswordService, ActionPasswordStatus};
    use crate::config::types::SecurityConfig;

    fn service() -> (ActionPasswordService, std::path::PathBuf) {
        let path = env::temp_dir().join(format!("edumind-action-password-{}.hash", Uuid::new_v4()));
        let config = SecurityConfig {
            action_password_hash_path: path.clone(),
            ..SecurityConfig::default()
        };
        (ActionPasswordService::from_config(&config), path)
    }

    #[test]
    fn hashes_verifies_and_issues_expiring_grants() {
        let (service, path) = service();
        let now = Utc::now();
        service.set_password("a safely long password").unwrap();

        assert_eq!(service.status().unwrap(), ActionPasswordStatus::Configured);
        assert!(service.verify("a safely long password").unwrap());
        assert!(!service.verify("not the password").unwrap());
        let grant = service.grant(Some("a safely long password"), now).unwrap();
        assert!(grant.is_valid(now + Duration::minutes(4)));
        assert!(!grant.is_valid(now + Duration::minutes(6)));
        assert!(fs::read_to_string(&path).unwrap().contains("$argon2id$"));

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn enforces_minimum_length_and_missing_passwords() {
        let (service, path) = service();

        assert!(service.set_password("short").is_err());
        assert!(service.authorize(None).is_err());
        assert_eq!(
            service.status().unwrap(),
            ActionPasswordStatus::NotConfigured
        );
        assert!(!path.exists());
    }
}
