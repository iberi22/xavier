//! Cryptographically secure API key generation bound to the hardware vault.
//!
//! Design rules enforced here:
//!
//! - entropy comes from the OS CSPRNG ([`rand::rngs::OsRng`]), never from a
//!   userspace PRNG;
//! - the plaintext key is registered in the global Clavis log masker *before*
//!   it can reach any log statement or any writer;
//! - the generated value is only ever written to an explicit writer by
//!   `xavier keys export`, never by the generation path.

use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::clavis;
use crate::secrets::vault::HardwareVault;
use crate::secrets::{SecretError, SecretResult};

/// Default base prefix for issued keys (`xavier_live_`, `xavier_test_`).
pub const DEFAULT_KEY_PREFIX: &str = "xavier";
/// Environment label baked into the key body so a leaked key is attributable.
pub const ENV_LIVE: &str = "live";
/// Test-scope label baked into the key body.
pub const ENV_TEST: &str = "test";
/// 32 random bytes = 256 bits of entropy per key.
pub const KEY_ENTROPY_BYTES: usize = 32;
/// Default lifetime of a generated key: 90 days.
pub const DEFAULT_TTL_SECS: u64 = 90 * 24 * 60 * 60;

/// Environment variable overriding [`DEFAULT_KEY_PREFIX`].
pub const ENV_KEY_PREFIX: &str = "XAVIER_KEY_PREFIX";
/// Environment variable overriding [`DEFAULT_TTL_SECS`].
pub const ENV_KEY_TTL: &str = "XAVIER_DEFAULT_KEY_TTL_SECS";

/// Vault entry holding the key body itself.
pub const VALUE_ENTRY_PREFIX: &str = "api_key_";
/// Vault entry holding the non-secret metadata JSON.
pub const META_ENTRY_SUFFIX: &str = "_meta";

/// Which environment scope a key is issued for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyScope {
    Live,
    Test,
}

impl KeyScope {
    /// The environment label used in the key body.
    pub fn label(self) -> &'static str {
        match self {
            Self::Live => ENV_LIVE,
            Self::Test => ENV_TEST,
        }
    }

    fn from_label(label: &str) -> Option<Self> {
        match label {
            ENV_LIVE => Some(Self::Live),
            ENV_TEST => Some(Self::Test),
            _ => None,
        }
    }
}

/// Non-secret metadata stored next to the key in the vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyMetadata {
    pub name: String,
    pub scope: String,
    /// Lifetime in seconds; `0` means "never expires".
    pub ttl_secs: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub rotation_count: u32,
    pub fingerprint: String,
}

/// A generated key. The `value` field must never be printed by the default
/// (generation) flow; it is returned to the caller so a deliberate export can
/// use it without a second vault read.
#[derive(Debug, Clone)]
pub struct GeneratedKey {
    pub metadata: KeyMetadata,
    pub value: String,
}

/// Minimal vault surface the generator needs. Implemented for
/// [`HardwareVault`] and faked in tests.
pub trait KeyVault {
    fn store(&self, key: &str, value: &str) -> SecretResult<()>;
    fn read(&self, key: &str) -> SecretResult<String>;
    fn remove(&self, key: &str) -> SecretResult<()>;
}

impl KeyVault for HardwareVault {
    fn store(&self, key: &str, value: &str) -> SecretResult<()> {
        self.store_secret(key, value)
    }
    fn read(&self, key: &str) -> SecretResult<String> {
        self.get_secret(key)
    }
    fn remove(&self, key: &str) -> SecretResult<()> {
        self.delete_secret(key)
    }
}

/// Errors specific to key generation, surfaced as [`SecretError`] so callers
/// keep a single error type.
#[derive(Debug, thiserror::Error)]
pub enum KeyGenError {
    #[error("invalid key name '{0}': use 1-64 chars of [A-Za-z0-9_-]")]
    InvalidName(String),
    #[error("invalid key prefix from {ENV_KEY_PREFIX}: {0}")]
    InvalidPrefix(String),
    #[error("invalid {ENV_KEY_TTL}: {0}")]
    InvalidTtl(String),
}

impl From<KeyGenError> for SecretError {
    fn from(e: KeyGenError) -> Self {
        SecretError::ProviderError(e.to_string())
    }
}

/// Validate a logical key name. Names become vault entry suffixes, so they are
/// restricted to characters that cannot escape the vault directory.
pub fn validate_name(name: &str) -> Result<(), KeyGenError> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok {
        Ok(())
    } else {
        Err(KeyGenError::InvalidName(name.to_string()))
    }
}

/// Base prefix, honouring [`ENV_KEY_PREFIX`].
pub fn key_prefix() -> Result<String, KeyGenError> {
    let raw = match std::env::var(ENV_KEY_PREFIX) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => DEFAULT_KEY_PREFIX.to_string(),
    };
    let ok = raw
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !raw.is_empty()
        && !raw.starts_with('_');
    if ok {
        Ok(raw)
    } else {
        Err(KeyGenError::InvalidPrefix(raw))
    }
}

/// Key body prefix, e.g. `xavier_live_` or `xavier_test_`.
pub fn key_body_prefix(scope: KeyScope) -> Result<String, KeyGenError> {
    Ok(format!("{}_{}_", key_prefix()?, scope.label()))
}

/// Default TTL, honouring [`ENV_KEY_TTL`]. `0` disables expiry.
pub fn default_ttl_secs() -> Result<u64, KeyGenError> {
    match std::env::var(ENV_KEY_TTL) {
        Ok(v) if !v.trim().is_empty() => v
            .trim()
            .parse::<u64>()
            .map_err(|e| KeyGenError::InvalidTtl(e.to_string())),
        _ => Ok(DEFAULT_TTL_SECS),
    }
}

/// Full key body: `<prefix><64 hex chars>`, 256 bits from the OS CSPRNG.
pub fn generate_key_value(scope: KeyScope) -> Result<String, KeyGenError> {
    let prefix = key_body_prefix(scope)?;
    let mut bytes = [0u8; KEY_ENTROPY_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    Ok(format!(
        "{prefix}{}",
        crate::utils::crypto::hex_encode(&bytes)
    ))
}

/// `sha256:<16 hex>` fingerprint, byte-compatible with
/// `format_vault_get_output` in the CLI so both surfaces read identically.
pub fn fingerprint(value: &str) -> String {
    let hash = Sha256::digest(value.as_bytes());
    let fp: String = hash.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("sha256:{fp}")
}

/// Vault entry name holding the key body.
pub fn value_entry(name: &str) -> String {
    format!("{VALUE_ENTRY_PREFIX}{name}")
}

/// Vault entry name holding the metadata JSON.
pub fn meta_entry(name: &str) -> String {
    format!("{}{name}{META_ENTRY_SUFFIX}", VALUE_ENTRY_PREFIX)
}

/// Read existing metadata, if any. A corrupt entry is treated as "no
/// metadata" (rotation restarts at 0) rather than failing generation.
pub fn load_metadata(vault: &impl KeyVault, name: &str) -> Option<KeyMetadata> {
    let raw = vault.read(&meta_entry(name)).ok()?;
    serde_json::from_str::<KeyMetadata>(&raw).ok()
}

/// Persist the key and its metadata. The value is masked in logs *before* any
/// I/O so no error path can leak it.
pub fn store_generated(vault: &impl KeyVault, key: &GeneratedKey) -> SecretResult<()> {
    clavis::register_secret(&key.value);
    let meta_json = serde_json::to_string(&key.metadata)?;
    vault.store(&value_entry(&key.metadata.name), &key.value)?;
    vault.store(&meta_entry(&key.metadata.name), &meta_json)?;
    Ok(())
}

/// Generate a new key for `name`, persisting it to `vault`.
///
/// `now` is injected so expiry arithmetic is testable. When metadata already
/// exists the value is replaced and `rotation_count` is incremented, so a
/// rotation is always observable.
pub fn generate_key(
    vault: &impl KeyVault,
    name: &str,
    scope: KeyScope,
    ttl_secs: u64,
    now: chrono::DateTime<chrono::Utc>,
) -> SecretResult<GeneratedKey> {
    validate_name(name)?;
    let value = generate_key_value(scope)?;
    // Mask before anything else can log or format the plaintext.
    clavis::register_secret(&value);

    let previous = load_metadata(vault, name);
    let rotation_count = previous.map_or(0, |p| p.rotation_count.saturating_add(1));
    let expires_at = (ttl_secs > 0).then(|| now + chrono::Duration::seconds(ttl_secs as i64));
    let fingerprint = fingerprint(&value);

    let key = GeneratedKey {
        value,
        metadata: KeyMetadata {
            name: name.to_string(),
            scope: scope.label().to_string(),
            ttl_secs,
            created_at: now,
            expires_at,
            rotation_count,
            fingerprint,
        },
    };
    store_generated(vault, &key)?;
    Ok(key)
}

/// Redacted, user-facing rendering of a generated key. Contains the name, the
/// fingerprint, the expiry and the rotation count — never the value.
pub fn format_generate_output(key: &GeneratedKey) -> String {
    let expiry = key
        .metadata
        .expires_at
        .map(|t| t.to_rfc3339())
        .unwrap_or_else(|| "never".to_string());
    format!(
        "{}: env={} backend=hardware-vault fingerprint={} ttl_secs={} expires_at={} rotation={} value=<hidden>",
        key.metadata.name,
        key.metadata.scope,
        key.metadata.fingerprint,
        key.metadata.ttl_secs,
        expiry,
        key.metadata.rotation_count,
    )
}

/// Remove a key and its metadata. A missing metadata entry is not an error.
pub fn revoke_key(vault: &impl KeyVault, name: &str) -> SecretResult<()> {
    validate_name(name)?;
    vault.remove(&value_entry(name))?;
    match vault.remove(&meta_entry(name)) {
        Ok(()) | Err(SecretError::NotFound(_)) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Read back the plaintext of an existing key. Callers MUST register it in the
/// global masker and require an explicit confirmation before writing it out.
pub fn export_key(vault: &impl KeyVault, name: &str) -> SecretResult<String> {
    validate_name(name)?;
    vault.read(&value_entry(name))
}

/// Scope stored in the metadata of an existing key (defaults to live).
pub fn stored_scope(vault: &impl KeyVault, name: &str) -> KeyScope {
    load_metadata(vault, name)
        .and_then(|m| KeyScope::from_label(&m.scope))
        .unwrap_or(KeyScope::Live)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemVault {
        items: Mutex<HashMap<String, String>>,
    }

    impl KeyVault for MemVault {
        fn store(&self, key: &str, value: &str) -> SecretResult<()> {
            self.items
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }
        fn read(&self, key: &str) -> SecretResult<String> {
            self.items
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .ok_or_else(|| SecretError::NotFound(key.to_string()))
        }
        fn remove(&self, key: &str) -> SecretResult<()> {
            self.items
                .lock()
                .unwrap()
                .remove(key)
                .map(|_| ())
                .ok_or_else(|| SecretError::NotFound(key.to_string()))
        }
    }

    // Env vars are process-global, so the prefix/TTL overrides are pinned
    // inside these tests instead of relying on the ambient environment.
    // The crate targets edition 2021, where `set_var`/`remove_var` are safe.
    fn with_env<T>(key: &str, value: Option<&str>, f: impl FnOnce() -> T) -> T {
        let previous = std::env::var(key).ok();
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        let out = f();
        match previous {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        out
    }

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("valid timestamp")
    }

    #[test]
    fn keys_gen_value_has_scope_prefix_and_full_entropy() {
        with_env(ENV_KEY_PREFIX, None, || {
            let live = generate_key_value(KeyScope::Live).expect("live key");
            let test = generate_key_value(KeyScope::Test).expect("test key");

            assert!(live.starts_with("xavier_live_"), "got: {live}");
            assert!(test.starts_with("xavier_test_"), "got: {test}");

            for (key, prefix) in [(&live, "xavier_live_"), (&test, "xavier_test_")] {
                let body = key.strip_prefix(prefix).expect("prefix stripped");
                assert!(
                    body.len() >= 32,
                    "expected at least 32 hex chars, got {}: {key}",
                    body.len()
                );
                assert_eq!(body.len(), KEY_ENTROPY_BYTES * 2);
                assert!(
                    body.chars()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
                    "body must be lowercase hex: {key}"
                );
            }
        });
    }

    #[test]
    fn keys_gen_prefix_honours_env_override() {
        with_env(ENV_KEY_PREFIX, Some("acme"), || {
            assert_eq!(key_prefix().expect("prefix"), "acme");
            let key = generate_key_value(KeyScope::Live).expect("key");
            assert!(key.starts_with("acme_live_"), "got: {key}");
        });
        with_env(ENV_KEY_PREFIX, Some("BAD PREFIX"), || {
            assert!(key_prefix().is_err(), "invalid prefix must be rejected");
        });
    }

    #[test]
    fn keys_gen_two_generations_are_distinct() {
        with_env(ENV_KEY_PREFIX, None, || {
            let vault = MemVault::default();
            let a = generate_key(&vault, "dist", KeyScope::Live, 60, now()).expect("first");
            let b = generate_key(&vault, "other", KeyScope::Live, 60, now()).expect("second");
            assert_ne!(a.value, b.value, "two generations must differ");
            assert_ne!(a.metadata.fingerprint, b.metadata.fingerprint);
        });
    }

    #[test]
    fn keys_gen_persists_and_reads_back_from_vault() {
        with_env(ENV_KEY_PREFIX, None, || {
            let vault = MemVault::default();
            let key = generate_key(&vault, "android", KeyScope::Live, 3600, now()).expect("gen");

            let read_back = export_key(&vault, "android").expect("export");
            assert_eq!(read_back, key.value);
            assert!(read_back.starts_with("xavier_live_"));

            let meta = load_metadata(&vault, "android").expect("metadata round-trips");
            assert_eq!(meta.name, "android");
            assert_eq!(meta.scope, "live");
            assert_eq!(meta.ttl_secs, 3600);
            assert_eq!(meta.rotation_count, 0);
            assert_eq!(meta.fingerprint, key.metadata.fingerprint);
            assert!(meta.expires_at.is_some());
        });
    }

    #[test]
    fn keys_gen_output_never_contains_the_value() {
        with_env(ENV_KEY_PREFIX, None, || {
            let vault = MemVault::default();
            let key = generate_key(&vault, "swalvault", KeyScope::Live, 900, now()).expect("gen");
            let rendered = format_generate_output(&key);

            assert!(
                !rendered.contains(&key.value),
                "output must NOT contain the key; got: {rendered}"
            );
            assert!(!rendered.contains(&key.value[13..21]), "hex body leaked");
            assert!(rendered.contains("swalvault"), "got: {rendered}");
            assert!(rendered.contains("sha256:"), "got: {rendered}");
            assert!(rendered.contains("fingerprint="), "got: {rendered}");
            assert!(rendered.contains("value=<hidden>"), "got: {rendered}");
            assert!(rendered.contains("rotation=0"), "got: {rendered}");
        });
    }

    #[test]
    fn keys_gen_rotation_increments_count_and_changes_value() {
        with_env(ENV_KEY_PREFIX, None, || {
            let vault = MemVault::default();
            let first = generate_key(&vault, "rot", KeyScope::Live, 60, now()).expect("first");
            let second = generate_key(&vault, "rot", KeyScope::Live, 60, now()).expect("second");
            let third = generate_key(&vault, "rot", KeyScope::Live, 60, now()).expect("third");

            assert_eq!(first.metadata.rotation_count, 0);
            assert_eq!(second.metadata.rotation_count, 1);
            assert_eq!(third.metadata.rotation_count, 2);
            assert_ne!(first.value, second.value);
            assert_ne!(second.value, third.value);
            // Only the newest value is live in the vault.
            assert_eq!(export_key(&vault, "rot").expect("export"), third.value);
            assert_eq!(
                load_metadata(&vault, "rot").expect("meta").rotation_count,
                2
            );
        });
    }

    #[test]
    fn keys_gen_default_ttl_honours_env_override() {
        with_env(ENV_KEY_TTL, Some("120"), || {
            assert_eq!(default_ttl_secs().expect("ttl"), 120);
        });
        with_env(ENV_KEY_TTL, Some("not-a-number"), || {
            assert!(default_ttl_secs().is_err());
        });
        with_env(ENV_KEY_TTL, None, || {
            assert_eq!(default_ttl_secs().expect("ttl"), DEFAULT_TTL_SECS);
        });
    }

    #[test]
    fn keys_gen_ttl_zero_means_no_expiry() {
        with_env(ENV_KEY_PREFIX, None, || {
            let vault = MemVault::default();
            let key = generate_key(&vault, "forever", KeyScope::Live, 0, now()).expect("gen");
            assert!(key.metadata.expires_at.is_none());
            assert!(format_generate_output(&key).contains("expires_at=never"));
        });
    }

    #[test]
    fn keys_gen_rejects_traversal_names() {
        let vault = MemVault::default();
        for bad in [
            "",
            "..",
            "../etc/passwd",
            "a/b",
            "with space",
            &"x".repeat(65),
        ] {
            assert!(
                generate_key(&vault, bad, KeyScope::Live, 60, now()).is_err(),
                "name {bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn keys_gen_revoke_removes_value_and_metadata() {
        with_env(ENV_KEY_PREFIX, None, || {
            let vault = MemVault::default();
            generate_key(&vault, "temp", KeyScope::Test, 60, now()).expect("gen");
            revoke_key(&vault, "temp").expect("revoke");
            assert!(export_key(&vault, "temp").is_err());
            assert!(load_metadata(&vault, "temp").is_none());
            // Idempotent enough for a CLI: a second revoke on a missing body fails.
            assert!(revoke_key(&vault, "temp").is_err());
        });
    }

    #[test]
    fn keys_gen_stored_scope_round_trips() {
        with_env(ENV_KEY_PREFIX, None, || {
            let vault = MemVault::default();
            generate_key(&vault, "t", KeyScope::Test, 60, now()).expect("gen");
            assert_eq!(stored_scope(&vault, "t"), KeyScope::Test);
            assert_eq!(stored_scope(&vault, "missing"), KeyScope::Live);
        });
    }

    #[test]
    fn keys_gen_value_is_masked_in_logs() {
        with_env(ENV_KEY_PREFIX, None, || {
            let vault = MemVault::default();
            let key = generate_key(&vault, "mask", KeyScope::Live, 60, now()).expect("gen");
            let masked = clavis::mask_log_message(&format!("leak? {}", key.value));
            // The masker replaces the key with its first-4/last-4 form, so the
            // full value must be absent and the middle bytes must not survive.
            assert!(!masked.contains(&key.value), "key must be masked: {masked}");
            let body = &key.value["xavier_live_".len()..];
            assert!(
                !masked.contains(&body[..16]),
                "key body leaked into the log line: {masked}"
            );
            assert!(
                masked.contains(&clavis::mask_key(&key.value)),
                "expected the mask_key form; got: {masked}"
            );
        });
    }
}
