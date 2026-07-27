//! Clavis Key Masking and Auto-Rotation System
//!
//! Provides automatic key masking in logs, key auto-rotation with configurable TTL,
//! and integration with the proxy.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock, RwLock};
use uuid::Uuid;

/// Mask a key safely showing only the first 4 and last 4 characters.
/// If the key is shorter than or equal to 8 characters, it handles masking gracefully.
pub fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    let len = chars.len();
    if len == 0 {
        return String::new();
    }
    if len <= 4 {
        "...".to_string()
    } else if len <= 8 {
        format!("{}...{}", chars[0], chars[len - 1])
    } else {
        let first_4: String = chars[..4].iter().collect();
        let last_4: String = chars[len - 4..].iter().collect();
        format!("{}...{}", first_4, last_4)
    }
}

/// Global thread-safe secret log masker registry.
pub struct ClavisLogMasker {
    secrets: RwLock<HashSet<String>>,
}

impl ClavisLogMasker {
    /// Create a new `ClavisLogMasker`.
    pub fn new() -> Self {
        Self {
            secrets: RwLock::new(HashSet::new()),
        }
    }

    /// Register a raw secret to be masked in logs.
    pub fn register_secret(&self, secret: &str) {
        if secret.len() >= 4 {
            let mut secrets = self.secrets.write().unwrap();
            secrets.insert(secret.to_string());
        }
    }

    /// Unregister a raw secret.
    pub fn unregister_secret(&self, secret: &str) {
        let mut secrets = self.secrets.write().unwrap();
        secrets.remove(secret);
    }

    /// Mask all occurrences of registered raw secrets inside the message.
    pub fn mask_message(&self, message: &str) -> String {
        let mut masked = message.to_string();
        let secrets = self.secrets.read().unwrap();
        
        // Sort secrets by length descending to replace longer secrets first
        let mut sorted_secrets: Vec<String> = secrets.iter().cloned().collect();
        sorted_secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));

        for secret in sorted_secrets {
            if secret.len() >= 4 {
                let masked_secret = mask_key(&secret);
                masked = masked.replace(&secret, &masked_secret);
            }
        }
        masked
    }
}

impl Default for ClavisLogMasker {
    fn default() -> Self {
        Self::new()
    }
}

static LOG_MASKER: OnceLock<Arc<ClavisLogMasker>> = OnceLock::new();

/// Retrieve the global log masker instance.
pub fn get_global_masker() -> Arc<ClavisLogMasker> {
    LOG_MASKER.get_or_init(|| Arc::new(ClavisLogMasker::new())).clone()
}

/// Register a raw secret to the global log masker.
pub fn register_secret(secret: &str) {
    get_global_masker().register_secret(secret);
}

/// Unregister a raw secret from the global log masker.
pub fn unregister_secret(secret: &str) {
    get_global_masker().unregister_secret(secret);
}

/// Mask all registered secrets inside a log message.
pub fn mask_log_message(msg: &str) -> String {
    get_global_masker().mask_message(msg)
}

/// Macro for logging info with automatic key masking.
#[macro_export]
macro_rules! clavis_info {
    ($($arg:tt)+) => {{
        let raw_msg = format!($($arg)+);
        let masked_msg = $crate::clavis::mask_log_message(&raw_msg);
        tracing::info!("{}", masked_msg);
    }};
}

/// Macro for logging warning with automatic key masking.
#[macro_export]
macro_rules! clavis_warn {
    ($($arg:tt)+) => {{
        let raw_msg = format!($($arg)+);
        let masked_msg = $crate::clavis::mask_log_message(&raw_msg);
        tracing::warn!("{}", masked_msg);
    }};
}

/// Macro for logging error with automatic key masking.
#[macro_export]
macro_rules! clavis_error {
    ($($arg:tt)+) => {{
        let raw_msg = format!($($arg)+);
        let masked_msg = $crate::clavis::mask_log_message(&raw_msg);
        tracing::error!("{}", masked_msg);
    }};
}

/// Struct representing a Clavis managed key with auto-rotation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClavisKey {
    pub id: String,
    pub name: String,
    pub value: String,
    pub ttl_secs: u64,
    pub last_rotated: DateTime<Utc>,
    pub rotation_count: usize,
}

pub type GeneratorFn = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// Clavis auto-rotating key engine.
pub struct ClavisEngine {
    keys: RwLock<HashMap<String, ClavisKey>>,
    generators: RwLock<HashMap<String, GeneratorFn>>,
}

impl ClavisEngine {
    /// Create a new `ClavisEngine`.
    pub fn new() -> Self {
        Self {
            keys: RwLock::new(HashMap::new()),
            generators: RwLock::new(HashMap::new()),
        }
    }

    /// Register a key with static or default UUID-based rotation.
    pub async fn register_key(&self, id: &str, name: &str, initial_value: &str, ttl_secs: u64) {
        let key = ClavisKey {
            id: id.to_string(),
            name: name.to_string(),
            value: initial_value.to_string(),
            ttl_secs,
            last_rotated: Utc::now(),
            rotation_count: 0,
        };
        register_secret(initial_value);
        let mut keys = self.keys.write().unwrap();
        keys.insert(id.to_string(), key);
    }

    /// Register a key with a custom generator callback for rotation.
    pub async fn register_key_with_generator<F>(&self, id: &str, name: &str, initial_value: &str, ttl_secs: u64, generator: F)
    where
        F: Fn(&str) -> String + Send + Sync + 'static,
    {
        let key = ClavisKey {
            id: id.to_string(),
            name: name.to_string(),
            value: initial_value.to_string(),
            ttl_secs,
            last_rotated: Utc::now(),
            rotation_count: 0,
        };
        register_secret(initial_value);
        
        {
            let mut keys = self.keys.write().unwrap();
            keys.insert(id.to_string(), key);
        }
        {
            let mut generators = self.generators.write().unwrap();
            generators.insert(id.to_string(), Arc::new(generator));
        }
    }

    /// Get active key value by key ID.
    pub async fn get_key_value(&self, id: &str) -> Option<String> {
        let keys = self.keys.read().unwrap();
        keys.get(id).map(|k| k.value.clone())
    }

    /// Get active key value by secret name.
    pub async fn get_key_value_by_name(&self, name: &str) -> Option<String> {
        let keys = self.keys.read().unwrap();
        keys.values()
            .find(|k| k.name == name)
            .map(|k| k.value.clone())
    }

    /// Get a cloned copy of the registered ClavisKey metadata.
    pub async fn get_key(&self, id: &str) -> Option<ClavisKey> {
        let keys = self.keys.read().unwrap();
        keys.get(id).cloned()
    }

    /// Set a key value directly, e.g. from an external source or rotation event.
    pub async fn set_key_value(&self, id: &str, new_value: &str) -> bool {
        let mut keys = self.keys.write().unwrap();
        if let Some(key) = keys.get_mut(id) {
            unregister_secret(&key.value);
            key.value = new_value.to_string();
            key.last_rotated = Utc::now();
            key.rotation_count += 1;
            register_secret(new_value);
            true
        } else {
            false
        }
    }

    /// Check all registered keys and perform automatic rotation if TTL is expired.
    /// Returns a list of rotated (key_id, new_value).
    pub async fn check_and_rotate_keys(&self) -> Vec<(String, String)> {
        let now = Utc::now();
        let mut rotated = Vec::new();
        
        // Find keys that need rotation
        let keys_to_rotate: Vec<(String, String, u64)> = {
            let keys = self.keys.read().unwrap();
            keys.iter()
                .filter(|(_, key)| {
                    let expiration = key.last_rotated + Duration::seconds(key.ttl_secs as i64);
                    now > expiration
                })
                .map(|(id, key)| (id.clone(), key.name.clone(), key.ttl_secs))
                .collect()
        };

        if keys_to_rotate.is_empty() {
            return Vec::new();
        }

        // Rotate selected keys
        for (id, name, _ttl) in keys_to_rotate {
            let generator = {
                let generators = self.generators.read().unwrap();
                generators.get(&id).cloned()
            };

            let new_value = match generator {
                Some(gen) => gen(&name),
                None => {
                    // Default secure UUID-based generator
                    format!("clavis_{}_{}", name, Uuid::new_v4().to_string().replace("-", ""))
                }
            };

            if self.set_key_value(&id, &new_value).await {
                rotated.push((id, new_value));
            }
        }

        rotated
    }
}

impl Default for ClavisEngine {
    fn default() -> Self {
        Self::new()
    }
}

static CLAVIS_ENGINE: OnceLock<Arc<ClavisEngine>> = OnceLock::new();

/// Retrieve the global clavis auto-rotation engine instance.
pub fn get_global_engine() -> Arc<ClavisEngine> {
    CLAVIS_ENGINE.get_or_init(|| Arc::new(ClavisEngine::new())).clone()
}

/// Spawn a tokio background task that periodically checks and rotates keys.
pub fn start_auto_rotation_task(engine: Arc<ClavisEngine>, interval_ms: u64) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(interval_ms));
        loop {
            interval.tick().await;
            let rotated_keys = engine.check_and_rotate_keys().await;
            for (id, new_val) in rotated_keys {
                let masked = mask_key(&new_val);
                tracing::info!("Auto-rotated Clavis key '{}'. New masked value: {}", id, masked);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_key_scenarios() {
        // Long key (> 8 chars)
        assert_eq!(mask_key("sk-ant-api03-abcdef1234567890"), "sk-a...7890");
        assert_eq!(mask_key("super-secret-credential"), "supe...tial");

        // Medium key (5-8 chars)
        assert_eq!(mask_key("abcdefgh"), "a...h");
        assert_eq!(mask_key("1234567"), "1...7");

        // Short key (<= 4 chars)
        assert_eq!(mask_key("abcd"), "...");
        assert_eq!(mask_key("123"), "...");
        assert_eq!(mask_key(""), "");
    }

    #[test]
    fn test_clavis_log_masker() {
        let masker = ClavisLogMasker::new();
        let secret1 = "sk-ant-api03-abcdef1234567890";
        let secret2 = "another-secret-token";

        masker.register_secret(secret1);
        masker.register_secret(secret2);

        // Test masking multiple secrets in a single message
        let log_msg = format!("Sending request with API key: {} or backup key: {}", secret1, secret2);
        let masked = masker.mask_message(&log_msg);

        assert!(!masked.contains(secret1));
        assert!(!masked.contains(secret2));
        assert!(masked.contains(&mask_key(secret1)));
        assert!(masked.contains(&mask_key(secret2)));

        // Unregister secret
        masker.unregister_secret(secret1);
        let log_msg_2 = format!("Key is: {}", secret1);
        let masked_2 = masker.mask_message(&log_msg_2);
        assert!(masked_2.contains(secret1));
    }

    #[tokio::test]
    async fn test_clavis_engine_rotation() {
        let engine = ClavisEngine::new();
        let key_id = "test_key";
        let key_name = "test_name";
        let initial_val = "initial-secret-value-12345";

        // Register with small TTL for testing
        engine.register_key(key_id, key_name, initial_val, 1).await;

        // Verify initial registration and value
        assert_eq!(engine.get_key_value(key_id).await.unwrap(), initial_val);
        assert_eq!(engine.get_key_value_by_name(key_name).await.unwrap(), initial_val);

        // Register secret in global log masker
        let log_msg = format!("My secret is: {}", initial_val);
        assert!(mask_log_message(&log_msg).contains(&mask_key(initial_val)));

        // Wait for TTL expiration
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Trigger check & rotate
        let rotated = engine.check_and_rotate_keys().await;
        assert_eq!(rotated.len(), 1);
        assert_eq!(rotated[0].0, key_id);

        let new_val = engine.get_key_value(key_id).await.unwrap();
        assert_ne!(new_val, initial_val);
        assert!(new_val.starts_with("clavis_test_name_"));

        // Verify new value is registered, and old value is unregistered in log masker
        let log_msg_old = format!("Old key was {}", initial_val);
        let log_msg_new = format!("New key is {}", new_val);

        // Global masker check: we should see old value NOT masked (since it unregistered),
        // and new value masked.
        assert!(mask_log_message(&log_msg_old).contains(initial_val));
        assert!(!mask_log_message(&log_msg_new).contains(&new_val));
        assert!(mask_log_message(&log_msg_new).contains(&mask_key(&new_val)));
    }

    #[tokio::test]
    async fn test_custom_generator_rotation() {
        let engine = ClavisEngine::new();
        let key_id = "custom_key";
        let key_name = "custom_name";
        let initial_val = "custom-initial-val-98765";

        // Custom generator simply appends suffix
        engine.register_key_with_generator(key_id, key_name, initial_val, 1, |name| {
            format!("{}-custom-rotated", name)
        }).await;

        // Wait for TTL expiration
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let rotated = engine.check_and_rotate_keys().await;
        assert_eq!(rotated.len(), 1);
        assert_eq!(rotated[0].1, "custom_name-custom-rotated");

        let active_val = engine.get_key_value(key_id).await.unwrap();
        assert_eq!(active_val, "custom_name-custom-rotated");
    }
}
