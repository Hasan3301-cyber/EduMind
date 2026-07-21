use std::sync::Arc;

use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, OsRng, Payload, rand_core::RngCore},
};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    infra::{EduMindError, Result},
    memory::{MemoryId, MemoryStore},
    secrets::MemoryKeyProvider,
};

/// Privacy level inferred before content is placed in a protected envelope.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataClassification {
    Public,
    #[default]
    Internal,
    Sensitive,
    Restricted,
}

impl DataClassification {
    fn label(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Internal => "internal",
            Self::Sensitive => "sensitive",
            Self::Restricted => "restricted",
        }
    }
}

/// AES-256-GCM payload persisted separately from ordinary searchable memory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EncryptedMemoryEnvelope {
    pub version: u8,
    pub classification: DataClassification,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

/// Content-free audit proof that a protected memory record was deleted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SecureDeletionRecord {
    pub id: Uuid,
    pub memory_id: MemoryId,
    pub classification: DataClassification,
    pub reason: String,
    pub deleted_at: DateTime<Utc>,
}

/// Encrypts private values and records secure local deletion without exposing their plaintext.
#[derive(Clone)]
pub struct MemoryPrivacyService {
    store: MemoryStore,
    key_provider: Arc<dyn MemoryKeyProvider>,
}

impl MemoryPrivacyService {
    /// Creates a privacy service with an explicit local key provider.
    pub fn new(store: MemoryStore, key_provider: impl MemoryKeyProvider + 'static) -> Self {
        Self {
            store,
            key_provider: Arc::new(key_provider),
        }
    }

    /// Classifies content deterministically without retaining it in audit records.
    #[must_use]
    pub fn classify(content: &str) -> DataClassification {
        let normalized = content.to_ascii_lowercase();
        if contains_any(
            &normalized,
            &[
                "api key",
                "access token",
                "password",
                "bearer ",
                "secret key",
            ],
        ) {
            DataClassification::Restricted
        } else if contains_any(
            &normalized,
            &[
                "student id",
                "email",
                "phone",
                "medical",
                "disability",
                "address",
            ],
        ) {
            DataClassification::Sensitive
        } else {
            DataClassification::Internal
        }
    }

    /// Encrypts content with AES-256-GCM and authenticated classification metadata.
    pub fn encrypt(
        &self,
        classification: DataClassification,
        plaintext: &str,
        now: DateTime<Utc>,
    ) -> Result<EncryptedMemoryEnvelope> {
        if plaintext.is_empty() {
            return Err(EduMindError::Security(
                "protected memory content must not be empty".to_owned(),
            ));
        }
        let key = self.key_provider.load_key()?;
        let cipher = Aes256Gcm::new_from_slice(key.as_bytes())
            .map_err(|_| EduMindError::Security("memory encryption key is invalid".to_owned()))?;
        let mut nonce = [0_u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let additional_data = envelope_additional_data(classification);
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: &additional_data,
                },
            )
            .map_err(|_| EduMindError::Security("memory encryption failed".to_owned()))?;
        Ok(EncryptedMemoryEnvelope {
            version: 1,
            classification,
            nonce: nonce.to_vec(),
            ciphertext,
            created_at: now,
        })
    }

    /// Decrypts an authenticated envelope into a short-lived zeroizing string.
    pub fn decrypt(&self, envelope: &EncryptedMemoryEnvelope) -> Result<Zeroizing<String>> {
        if envelope.version != 1 || envelope.nonce.len() != 12 {
            return Err(EduMindError::Security(
                "memory envelope version or nonce is invalid".to_owned(),
            ));
        }
        let key = self.key_provider.load_key()?;
        let cipher = Aes256Gcm::new_from_slice(key.as_bytes())
            .map_err(|_| EduMindError::Security("memory encryption key is invalid".to_owned()))?;
        let additional_data = envelope_additional_data(envelope.classification);
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&envelope.nonce),
                Payload {
                    msg: &envelope.ciphertext,
                    aad: &additional_data,
                },
            )
            .map_err(|_| EduMindError::Security("memory decryption failed".to_owned()))?;
        String::from_utf8(plaintext)
            .map(Zeroizing::new)
            .map_err(|_| EduMindError::Security("memory envelope is not UTF-8".to_owned()))
    }

    /// Encrypts and persists an envelope associated with an existing memory record.
    pub fn store_envelope(
        &self,
        memory_id: MemoryId,
        plaintext: &str,
        now: DateTime<Utc>,
    ) -> Result<EncryptedMemoryEnvelope> {
        let classification = Self::classify(plaintext);
        let envelope = self.encrypt(classification, plaintext, now)?;
        let connection = self.store.connection()?;
        connection.execute(
            "INSERT INTO memory_private_envelopes (memory_id, classification, envelope_json, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(memory_id) DO UPDATE SET
                classification = excluded.classification,
                envelope_json = excluded.envelope_json,
                updated_at = excluded.updated_at",
            params![
                memory_id.to_string(),
                classification.label(),
                serde_json::to_string(&envelope)?,
                format_timestamp(now),
            ],
        )?;
        Ok(envelope)
    }

    /// Loads an encrypted envelope without decrypting it.
    pub fn load_envelope(&self, memory_id: MemoryId) -> Result<Option<EncryptedMemoryEnvelope>> {
        let connection = self.store.connection()?;
        let encoded = connection
            .query_row(
                "SELECT envelope_json FROM memory_private_envelopes WHERE memory_id = ?1",
                params![memory_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        encoded
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(EduMindError::from)
    }

    /// Removes searchable and encrypted content, then stores a content-free deletion record.
    pub fn secure_delete(
        &self,
        memory_id: MemoryId,
        reason: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<Option<SecureDeletionRecord>> {
        let reason = reason.into().trim().to_owned();
        if reason.is_empty() {
            return Err(EduMindError::Security(
                "secure deletion requires a non-empty reason".to_owned(),
            ));
        }
        let Some(record) = self.store.get(memory_id)? else {
            return Ok(None);
        };
        let deletion = SecureDeletionRecord {
            id: Uuid::new_v4(),
            memory_id,
            classification: Self::classify(&record.content),
            reason,
            deleted_at: now,
        };
        let mut connection = self.store.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM memory_fts WHERE memory_id = ?1",
            params![memory_id.to_string()],
        )?;
        let deleted = transaction.execute(
            "DELETE FROM memory_entries WHERE id = ?1",
            params![memory_id.to_string()],
        )?;
        if deleted == 0 {
            return Ok(None);
        }
        transaction.execute(
            "INSERT INTO memory_secure_deletions (
                id, memory_id, classification, reason, deleted_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                deletion.id.to_string(),
                deletion.memory_id.to_string(),
                deletion.classification.label(),
                deletion.reason,
                format_timestamp(deletion.deleted_at),
            ],
        )?;
        transaction.commit()?;
        Ok(Some(deletion))
    }
}

fn contains_any(content: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| content.contains(pattern))
}

fn envelope_additional_data(classification: DataClassification) -> Vec<u8> {
    format!("edumind-memory-envelope-v1:{}", classification.label()).into_bytes()
}

fn format_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use crate::{
        memory::{MemoryPrivacyService, MemoryStore, NewMemory},
        secrets::StaticMemoryKeyProvider,
    };

    use super::DataClassification;

    #[test]
    fn encrypted_envelopes_reject_the_wrong_key() {
        let store = MemoryStore::in_memory().unwrap();
        let owner = MemoryPrivacyService::new(store.clone(), StaticMemoryKeyProvider::new([7; 32]));
        let other = MemoryPrivacyService::new(store, StaticMemoryKeyProvider::new([9; 32]));
        let now = Utc.with_ymd_and_hms(2026, 7, 19, 8, 0, 0).unwrap();
        let envelope = owner
            .encrypt(DataClassification::Sensitive, "student@example.edu", now)
            .unwrap();

        assert_eq!(
            owner.decrypt(&envelope).unwrap().as_str(),
            "student@example.edu"
        );
        assert!(other.decrypt(&envelope).is_err());
    }

    #[test]
    fn secure_delete_removes_memory_and_keeps_only_audit_metadata() {
        let store = MemoryStore::in_memory().unwrap();
        let privacy =
            MemoryPrivacyService::new(store.clone(), StaticMemoryKeyProvider::new([1; 32]));
        let now = Utc.with_ymd_and_hms(2026, 7, 19, 8, 0, 0).unwrap();
        let memory = store
            .store(
                NewMemory::new("student-os", "student id 123", "profile"),
                now,
            )
            .unwrap();

        let deletion = privacy
            .secure_delete(memory.id, "user requested removal", now)
            .unwrap();
        assert!(deletion.is_some());
        assert!(store.get(memory.id).unwrap().is_none());
    }
}
