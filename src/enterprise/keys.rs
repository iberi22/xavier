//! API Key management
//!
//! Handles creation, validation, and revocation of API keys for tenant authentication.

use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use thiserror::Error;

use crate::enterprise::tenant::TenantId;

/// API key types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiKeyType {
    Live,
    Test,
}

/// API key entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    /// Key prefix for identification (pk_live_xxx)
    pub id: String,
    /// Hashed version for storage
    pub hash: String,
    /// Associated tenant
    pub tenant_id: TenantId,
    /// Human-readable name
    pub name: String,
    /// Key type (live/test)
    pub key_type: ApiKeyType,
    /// Custom rate limit override (0 = use plan default)
    pub rate_limit: u32,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last used timestamp
    pub last_used: Option<DateTime<Utc>>,
    /// Expiration (None = never)
    pub expires_at: Option<DateTime<Utc>>,
    /// Revoked flag
    pub revoked: bool,
    /// Optional metadata
    pub metadata: HashMap<String, String>,
}

impl ApiKey {
    /// Generate a new API key
    pub fn generate(
        tenant_id: TenantId,
        name: impl Into<String>,
        key_type: ApiKeyType,
    ) -> (String, Self) {
        let raw_key = generate_random_key();
        let prefix = match key_type {
            ApiKeyType::Live => "pk_live",
            ApiKeyType::Test => "pk_test",
        };
        let id = format!("{}_{}", prefix, &raw_key[..8]);
        let hash = hash_key(&raw_key);

        let key = Self {
            id,
            hash,
            tenant_id,
            name: name.into(),
            key_type,
            rate_limit: 0, // Use plan default
            created_at: Utc::now(),
            last_used: None,
            expires_at: None,
            revoked: false,
            metadata: HashMap::new(),
        };

        (raw_key, key)
    }

    /// Validate a raw key against this API key
    pub fn validate(&self, raw_key: &str) -> bool {
        if self.revoked {
            return false;
        }
        if let Some(expires) = self.expires_at {
            if Utc::now() > expires {
                return false;
            }
        }
        self.hash == hash_key(raw_key)
    }

    /// Revoke this API key
    pub fn revoke(&mut self) {
        self.revoked = true;
    }

    /// Update last used timestamp
    pub fn mark_used(&mut self) {
        self.last_used = Some(Utc::now());
    }

    /// Check if key is active
    pub fn is_active(&self) -> bool {
        !self.revoked && self.expires_at.is_none_or(|e| Utc::now() <= e)
    }
}

/// API key store
pub struct ApiKeyStore {
    keys: HashMap<String, ApiKey>,
    tenant_keys: HashMap<TenantId, Vec<String>>,
}

impl ApiKeyStore {
    /// New.
    pub fn new() -> Self {
        Self {
            keys: HashMap::new(),
            tenant_keys: HashMap::new(),
        }
    }

    /// Create a new API key (returns raw key - only shown once!)
    pub fn create(
        &mut self,
        tenant_id: TenantId,
        name: impl Into<String>,
        key_type: ApiKeyType,
    ) -> (String, ApiKey) {
        let (raw_key, key) = ApiKey::generate(tenant_id, name, key_type);

        self.keys.insert(key.id.clone(), key.clone());
        self.tenant_keys
            .entry(tenant_id)
            .or_default()
            .push(key.id.clone());

        (raw_key, key)
    }

    /// Get API key by ID
    pub fn get(&self, id: &str) -> Option<&ApiKey> {
        self.keys.get(id)
    }

    /// Get mutable API key
    pub fn get_mut(&mut self, id: &str) -> Option<&mut ApiKey> {
        self.keys.get_mut(id)
    }

    /// Validate a raw key and return the API key if valid
    pub fn validate(&self, raw_key: &str) -> Option<ApiKey> {
        for key in self.keys.values() {
            if key.validate(raw_key) {
                return Some(key.clone());
            }
        }
        None
    }

    /// Validate by key ID and raw key
    pub fn validate_key(&self, key_id: &str, raw_key: &str) -> Option<ApiKey> {
        self.keys.get(key_id).and_then(|key| {
            if key.validate(raw_key) {
                Some(key.clone())
            } else {
                None
            }
        })
    }

    /// Revoke an API key
    pub fn revoke(&mut self, key_id: &str) -> Result<(), ApiKeyError> {
        match self.keys.get_mut(key_id) {
            Some(key) => {
                key.revoke();
                Ok(())
            }
            None => Err(ApiKeyError::NotFound(key_id.to_string())),
        }
    }

    /// List all keys for a tenant
    pub fn list_for_tenant(&self, tenant_id: &TenantId) -> Vec<&ApiKey> {
        self.tenant_keys
            .get(tenant_id)
            .map(|ids| ids.iter().filter_map(|id| self.keys.get(id)).collect())
            .unwrap_or_default()
    }

    /// Delete an API key
    pub fn delete(&mut self, key_id: &str) -> Result<ApiKey, ApiKeyError> {
        match self.keys.remove(key_id) {
            Some(key) => {
                if let Some(ids) = self.tenant_keys.get_mut(&key.tenant_id) {
                    ids.retain(|id| id != key_id);
                }
                Ok(key)
            }
            None => Err(ApiKeyError::NotFound(key_id.to_string())),
        }
    }

    /// Count keys for a tenant
    pub fn count_for_tenant(&self, tenant_id: &TenantId) -> usize {
        self.tenant_keys
            .get(tenant_id)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Insert an API key directly into the store (used when loading from persistence).
    /// Unlike `create`, this does NOT generate a new key — it inserts the key as-is
    /// and rebuilds the tenant_keys index.
    pub fn insert_existing(&mut self, key: ApiKey) {
        let id = key.id.clone();
        let tenant_id = key.tenant_id;
        self.keys.insert(id.clone(), key);
        self.tenant_keys.entry(tenant_id).or_default().push(id);
    }

    /// Bulk insert API keys from persistence.
    pub fn load_from_iter(&mut self, keys: impl IntoIterator<Item = ApiKey>) {
        for key in keys {
            let id = key.id.clone();
            let tenant_id = key.tenant_id;
            self.keys.insert(id.clone(), key);
            self.tenant_keys.entry(tenant_id).or_default().push(id);
        }
    }
}

impl Default for ApiKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

/// API key errors
#[derive(Error, Debug)]
pub enum ApiKeyError {
    #[error("API key not found: {0}")]
    NotFound(String),
    #[error("API key revoked")]
    Revoked,
    #[error("API key expired")]
    Expired,
    #[error("Max keys reached for tenant")]
    MaxKeysReached,
    #[error("Invalid key format")]
    InvalidFormat,
}

/// Generate a random key string
fn generate_random_key() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
    crate::crypto::hex_encode(bytes)
}

/// Hash a key for storage
fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    crate::crypto::hex_encode(hasher.finalize())
}

/// Extract key prefix from full key (pk_live_xxxxxxxx)
pub fn extract_prefix(full_key: &str) -> Option<(&str, &str)> {
    let parts: Vec<&str> = full_key.split('_').collect();
    if parts.len() >= 3 {
        Some((parts[0], parts[1]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_api_key_generation() {
        let mut store = ApiKeyStore::new();
        let tenant_id = Uuid::new_v4();

        let (raw_key, key) = store.create(tenant_id, "Test Key", ApiKeyType::Live);

        assert!(raw_key.starts_with(|c: char| c.is_ascii_hexdigit()));
        assert_eq!(key.tenant_id, tenant_id);
        assert!(!key.revoked);
    }

    #[test]
    fn test_api_key_validation() {
        let mut store = ApiKeyStore::new();
        let tenant_id = Uuid::new_v4();

        let (raw_key, key) = store.create(tenant_id, "Test Key", ApiKeyType::Live);

        // Valid
        assert!(store.validate_key(&key.id, &raw_key).is_some());

        // Invalid raw key
        assert!(store.validate_key(&key.id, "wrong").is_none());

        // Revoke and test
        store.revoke(&key.id).unwrap();
        assert!(store.validate_key(&key.id, &raw_key).is_none());
    }
}
