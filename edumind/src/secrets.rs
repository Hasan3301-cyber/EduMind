use zeroize::Zeroizing;

use crate::{
    infra::{EduMindError, Result},
    security::KeyringSecretStore,
};

/// Fixed native-keychain entry used for the local memory encryption key.
pub const MEMORY_ENCRYPTION_KEY_NAME: &str = "memory-encryption-key";

/// A 256-bit key retained only for the duration of an encryption operation.
#[derive(Clone)]
pub struct MemoryEncryptionKey(Zeroizing<[u8; 32]>);

impl MemoryEncryptionKey {
    /// Creates a key from a caller-owned 256-bit value without serializing it.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// Decodes a 64-character hexadecimal key supplied by the native keychain.
    pub fn from_hex(value: &str) -> Result<Self> {
        let value = value.trim();
        if value.len() != 64 {
            return Err(EduMindError::Security(
                "memory encryption keys must contain exactly 32 bytes".to_owned(),
            ));
        }
        let mut bytes = [0_u8; 32];
        for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
            let high = decode_hex_nibble(chunk[0]).ok_or_else(|| {
                EduMindError::Security("memory encryption key is not hexadecimal".to_owned())
            })?;
            let low = decode_hex_nibble(chunk[1]).ok_or_else(|| {
                EduMindError::Security("memory encryption key is not hexadecimal".to_owned())
            })?;
            bytes[index] = (high << 4) | low;
        }
        Ok(Self::from_bytes(bytes))
    }

    /// Borrows the key only for the immediate authenticated-encryption call.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Resolves the memory encryption key immediately before private data is accessed.
pub trait MemoryKeyProvider: Send + Sync {
    /// Returns the active local key or an error when it is unavailable.
    fn load_key(&self) -> Result<MemoryEncryptionKey>;
}

/// Native-keychain provider for the local memory encryption key.
#[derive(Clone, Debug)]
pub struct KeyringMemoryKeyProvider {
    store: KeyringSecretStore,
    name: String,
}

impl KeyringMemoryKeyProvider {
    /// Creates a provider scoped to one application keychain service.
    pub fn new(service: impl Into<String>) -> Result<Self> {
        Ok(Self {
            store: KeyringSecretStore::new(service)?,
            name: MEMORY_ENCRYPTION_KEY_NAME.to_owned(),
        })
    }
}

impl MemoryKeyProvider for KeyringMemoryKeyProvider {
    fn load_key(&self) -> Result<MemoryEncryptionKey> {
        let value = self.store.get(&self.name)?.ok_or_else(|| {
            EduMindError::Security(
                "memory encryption key is unavailable in the native keychain".to_owned(),
            )
        })?;
        MemoryEncryptionKey::from_hex(value.expose())
    }
}

/// Explicit in-memory provider used only by deterministic tests and embedded callers.
#[derive(Clone)]
pub struct StaticMemoryKeyProvider {
    key: MemoryEncryptionKey,
}

impl StaticMemoryKeyProvider {
    /// Creates a provider from an already obtained 256-bit key.
    #[must_use]
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            key: MemoryEncryptionKey::from_bytes(key),
        }
    }
}

impl MemoryKeyProvider for StaticMemoryKeyProvider {
    fn load_key(&self) -> Result<MemoryEncryptionKey> {
        Ok(self.key.clone())
    }
}

fn decode_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::MemoryEncryptionKey;

    #[test]
    fn rejects_invalid_memory_key_encoding() {
        assert!(MemoryEncryptionKey::from_hex("not-a-key").is_err());
        assert!(MemoryEncryptionKey::from_hex(&"00".repeat(32)).is_ok());
    }
}
