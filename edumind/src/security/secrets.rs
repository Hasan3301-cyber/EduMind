use std::fmt;

use keyring::{Entry, Error as KeyringError};
use zeroize::Zeroizing;

use crate::infra::{EduMindError, Result};

/// A short-lived secret value that zeroizes its owned memory on drop.
#[derive(Clone)]
pub struct SecretValue(Zeroizing<String>);

impl SecretValue {
    /// Returns the secret only to the caller that needs it for an immediate provider request.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

/// OS-keychain-backed secret storage. No secret value is persisted in EduMind config files.
#[derive(Clone, Debug)]
pub struct KeyringSecretStore {
    service: String,
}

impl KeyringSecretStore {
    /// Creates a store scoped to a stable application service name.
    pub fn new(service: impl Into<String>) -> Result<Self> {
        let service = service.into();
        validate_identifier("keychain service", &service)?;
        Ok(Self { service })
    }

    /// Stores a non-empty secret in the native OS keychain.
    pub fn set(&self, name: &str, secret: &str) -> Result<()> {
        validate_identifier("secret name", name)?;
        if secret.is_empty() {
            return Err(EduMindError::Security(
                "refusing to store an empty secret".to_owned(),
            ));
        }
        self.entry(name)?
            .set_password(secret)
            .map_err(keyring_error)
    }

    /// Loads a secret from the native OS keychain without exposing missing-entry errors.
    pub fn get(&self, name: &str) -> Result<Option<SecretValue>> {
        validate_identifier("secret name", name)?;
        match self.entry(name)?.get_password() {
            Ok(secret) => Ok(Some(SecretValue(Zeroizing::new(secret)))),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(keyring_error(error)),
        }
    }

    /// Deletes a secret and returns whether a matching credential existed.
    pub fn delete(&self, name: &str) -> Result<bool> {
        validate_identifier("secret name", name)?;
        match self.entry(name)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(KeyringError::NoEntry) => Ok(false),
            Err(error) => Err(keyring_error(error)),
        }
    }

    fn entry(&self, name: &str) -> Result<Entry> {
        Entry::new(&self.service, name).map_err(keyring_error)
    }
}

fn validate_identifier(kind: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(EduMindError::Security(format!(
            "{kind} must contain only ASCII letters, digits, '.', '_' or '-'"
        )));
    }
    Ok(())
}

fn keyring_error(error: KeyringError) -> EduMindError {
    EduMindError::Security(format!("native keychain operation failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::KeyringSecretStore;

    #[test]
    fn validates_keychain_identifiers_without_touching_the_os_keychain() {
        assert!(KeyringSecretStore::new("edumind.desktop").is_ok());
        assert!(KeyringSecretStore::new("invalid service").is_err());
    }
}
