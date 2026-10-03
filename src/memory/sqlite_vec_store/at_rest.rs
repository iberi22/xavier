//! Per-record encryption at rest (envelope) for the local vec store.
//!
//! Every memory record gets its own random 32-byte DEK:
//! - `content` is AES-256-GCM encrypted with the DEK, nonce stored in `content_iv`.
//! - `metadata` (JSON) is AES-256-GCM encrypted with the same DEK, nonce in `metadata_iv`.
//! - The DEK itself is wrapped (AES-256-GCM) with the **node record key** and
//!   stored in `encrypted_dek`, prefixed with a version magic (`XRK1`).
//!
//! Stored `content` column holds hex(ciphertext) — never plaintext.
//!
//! Node record key resolution order:
//! 1. `XAVIER_RECORD_KEY` env var (64 hex chars = 32 bytes, optional `0x` prefix).
//! 2. File `$XAVIER_DATA_DIR/node/record.key` (0600), generated randomly if missing.
//! 3. If neither resolves, the node keeps working WITHOUT encrypting and logs a
//!    warning (startup must never fail because of this).
//!
//! Rows without `encrypted_dek` are legacy plaintext rows: reads return them
//! as-is (compat), and the `encrypt-records` CLI command migrates them.
//!
//! ## Default-space encryption (`XDK2`)
//!
//! Private rows (everything except records explicitly marked public, see
//! [`is_private_record`]) can instead be sealed under the single data key of
//! the `default` space keystore (`$XAVIER_DATA_DIR/spaces/default/keystore.json`,
//! `espacio::keys`). Such rows carry the 4-byte marker `XDK2` in
//! `encrypted_dek` (empty payload). `content` becomes `xr1:` + hex(nonce||ct)
//! and `metadata` becomes `{"encrypted":"xr1:..."}`; AES-256-GCM, random
//! 12-byte nonce per value, AD = (space id, kind, record id) so values cannot
//! be swapped between rows or between content and metadata. `content_iv` and
//! `metadata_iv` stay NULL (the nonce is inside the `xr1:` string).
//!
//! Read path: `XDK2` rows with a locked or missing keystore never return
//! ciphertext as content: the record is replaced by a locked placeholder and
//! the call returns an error. Legacy `XRK1` rows stay readable.
//!
//! Leaks that remain by design (documented in the CLI help): embeddings (kept
//! in clear locally so vector search works; never sent to cloud), entities
//! and relations already extracted into the graph tables, the `revisions` and
//! `relation` columns, `path`, timestamps and `memory_chain` hashes.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once, OnceLock};

use anyhow::{Context, Result};
use rand::RngCore;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

use crate::crypto::encryption::{decrypt_data, encrypt_data, NonceBytes};
use crate::espacio::keys::{
    KeyHandle, KeyRing, KeysError, MasterNodeKek, NodeKek, RecoveryCode, StaticNodeKek, UnlockMode,
    DEFAULT_SPACE_ID,
};

/// Env var holding the node record key (64 hex chars).
pub const RECORD_KEY_ENV: &str = "XAVIER_RECORD_KEY";
/// Env var holding the data dir (shared with the rest of Xavier settings).
pub const DATA_DIR_ENV: &str = "XAVIER_DATA_DIR";
/// Relative path of the key file under the data dir.
const KEY_FILE_REL: &str = "node/record.key";
/// Version magic prefixing DEKs wrapped by this module (`XRK1` + nonce12 + ct48).
const WRAPPED_DEK_MAGIC: &[u8; 4] = b"XRK1";

static WARN_NO_KEY: Once = Once::new();

/// Where the node record key should be found / created.
pub fn record_key_path() -> PathBuf {
    let base = std::env::var(DATA_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::settings::XavierSettings::resolve_data_dir());
    base.join(KEY_FILE_REL)
}

/// How the node record key was obtained (for CLI reporting).
#[derive(Debug, Clone)]
pub enum KeySource {
    /// From `XAVIER_RECORD_KEY`.
    Env,
    /// From an existing key file.
    File(PathBuf),
    /// Freshly generated into the key file.
    Generated(PathBuf),
    /// Not available: reads stay compatible, writes stay plaintext.
    Unavailable(String),
}

/// Resolve the node key plus where it came from.
pub fn resolve_record_key_with_source() -> (Option<[u8; 32]>, KeySource) {
    // (a) Env var wins when present and well-formed.
    if let Ok(raw) = std::env::var(RECORD_KEY_ENV) {
        let hex = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
        match crate::crypto::hex_decode(hex) {
            Ok(bytes) if bytes.len() == 32 => {
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                return (Some(key), KeySource::Env);
            }
            _ => {
                tracing::warn!(
                    "{} is set but is not 64 hex chars (32 bytes); falling back to the key file",
                    RECORD_KEY_ENV
                );
            }
        }
    }

    // (b) Key file, generated on first use.
    let path = record_key_path();
    match read_key_file(&path) {
        Ok(Some(key)) => (Some(key), KeySource::File(path)),
        Ok(None) => match generate_key_file(&path) {
            Ok(key) => (Some(key), KeySource::Generated(path)),
            Err(e) => (None, KeySource::Unavailable(e.to_string())),
        },
        Err(e) => (None, KeySource::Unavailable(e.to_string())),
    }
}

/// Resolve the node record key, or `None` when unavailable (caller warns / skips).
pub fn resolve_record_key() -> Option<[u8; 32]> {
    resolve_record_key_with_source().0
}

fn warn_no_key_once(reason: &str) {
    static MSG: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let msg = MSG.get_or_init(|| reason.to_string());
    WARN_NO_KEY.call_once(|| {
        tracing::warn!(
            "at_rest: no node record key available ({}). Records will be stored WITHOUT encryption. Set {} or make {} writable.",
            msg,
            RECORD_KEY_ENV,
            record_key_path().display()
        );
    });
}

fn read_key_file(path: &std::path::Path) -> Result<Option<[u8; 32]>> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let hex = text
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X");
            let bytes = crate::crypto::hex_decode(hex)
                .with_context(|| format!("record key file {} is not valid hex", path.display()))?;
            if bytes.len() != 32 {
                anyhow::bail!(
                    "record key file {} must hold 32 bytes, found {}",
                    path.display(),
                    bytes.len()
                );
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            Ok(Some(key))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow::anyhow!(
            "cannot read record key file {}: {e}",
            path.display()
        )),
    }
}

fn generate_key_file(path: &std::path::Path) -> Result<[u8; 32]> {
    let mut key = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut key);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    std::fs::write(path, crate::crypto::hex_encode(key))
        .with_context(|| format!("cannot write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("cannot chmod 0600 {}", path.display()))?;
    }
    tracing::info!(
        "at_rest: generated new node record key at {}",
        path.display()
    );
    Ok(key)
}

/// Whether an `encrypted_dek` blob was wrapped by this module (vs the legacy KEK path).
pub fn is_wrapped_v2(wrapped: &[u8]) -> bool {
    wrapped.starts_with(WRAPPED_DEK_MAGIC)
}

/// Wrap a per-record DEK with the node key: `XRK1 || nonce12 || ct`.
pub fn wrap_dek(dek: &[u8; 32], node_key: &[u8; 32]) -> Result<Vec<u8>> {
    let nonce = NonceBytes::generate();
    let ct = crate::crypto::encryption::aes_encrypt(dek, node_key, &nonce)
        .map_err(|e| anyhow::anyhow!("DEK wrap failed: {e}"))?;
    let mut out = Vec::with_capacity(4 + ct.len());
    out.extend_from_slice(WRAPPED_DEK_MAGIC);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Unwrap a DEK wrapped by [`wrap_dek`]. Wrong key fails here (AES-GCM auth).
pub fn unwrap_dek(wrapped: &[u8], node_key: &[u8; 32]) -> Result<[u8; 32]> {
    let blob = wrapped
        .strip_prefix(WRAPPED_DEK_MAGIC)
        .ok_or_else(|| anyhow::anyhow!("not a v2 wrapped DEK"))?;
    let dek = crate::crypto::encryption::aes_decrypt(blob, node_key)
        .map_err(|e| anyhow::anyhow!("DEK unwrap failed (wrong node key?): {e}"))?;
    if dek.len() != 32 {
        anyhow::bail!("unwrapped DEK has wrong length {}", dek.len());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&dek);
    Ok(out)
}

/// Encrypt `record.content` + `record.metadata` in place (envelope).
/// Returns `true` when the record was encrypted, `false` when no node key is
/// available (record left as plaintext, stale crypto columns cleared).
pub fn encrypt_columns_for_write(record: &mut crate::memory::store::MemoryRecord) -> Result<bool> {
    let (key, source) = resolve_record_key_with_source();
    let Some(node_key) = key else {
        let reason = match &source {
            KeySource::Unavailable(r) => r.clone(),
            _ => "unknown".to_string(),
        };
        warn_no_key_once(&reason);
        record.encrypted_dek = None;
        record.content_iv = None;
        record.metadata_iv = None;
        return Ok(false);
    };
    encrypt_columns_for_write_with_key(record, &node_key).map(|()| true)
}

/// [`encrypt_columns_for_write`] with an explicit key (no env/file lookup; used by tests).
pub fn encrypt_columns_for_write_with_key(
    record: &mut crate::memory::store::MemoryRecord,
    node_key: &[u8; 32],
) -> Result<()> {
    let mut dek = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut dek);

    let content_nonce = NonceBytes::generate();
    let encrypted_content = encrypt_data(record.content.as_bytes(), &dek, &content_nonce)
        .map_err(|e| anyhow::anyhow!("content encryption failed: {e}"))?;

    let metadata_json = serde_json::to_string(&record.metadata)?;
    let metadata_nonce = NonceBytes::generate();
    let encrypted_metadata = encrypt_data(metadata_json.as_bytes(), &dek, &metadata_nonce)
        .map_err(|e| anyhow::anyhow!("metadata encryption failed: {e}"))?;

    record.content = crate::utils::crypto::hex_encode(&encrypted_content.ciphertext);
    record.metadata = serde_json::json!({
        "encrypted": crate::utils::crypto::hex_encode(&encrypted_metadata.ciphertext)
    });
    record.encrypted_dek = Some(wrap_dek(&dek, node_key)?);
    record.content_iv = Some(content_nonce.as_bytes().to_vec());
    record.metadata_iv = Some(metadata_nonce.as_bytes().to_vec());
    Ok(())
}

/// Decrypt a record in place. Legacy rows (`encrypted_dek` empty) pass through
/// untouched. Wrong key returns an error — never silent garbage. For `XDK2`
/// rows a failure replaces the record with a locked placeholder first, so a
/// caller that discards the error (`let _ =`) can never serve ciphertext as
/// content.
pub fn decrypt_record_in_place(record: &mut crate::memory::store::MemoryRecord) -> Result<()> {
    let wrapped = match &record.encrypted_dek {
        None => return Ok(()),
        Some(b) if b.is_empty() => return Ok(()),
        Some(b) => b.clone(),
    };
    if is_default_space_row(&wrapped) {
        return decrypt_default_space_in_place(record, None);
    }
    if is_wrapped_v2(&wrapped) {
        let Some(node_key) = resolve_record_key() else {
            anyhow::bail!(
                "at_rest: record {} is encrypted but no node key is available (set {} or provide {})",
                record.id,
                RECORD_KEY_ENV,
                record_key_path().display()
            );
        };
        decrypt_record_with_key(record, &node_key)
    } else {
        // Legacy rows wrapped with the security-service KEK.
        crate::memory::sqlite_store::SqliteMemoryStore::decrypt_record(record)
    }
}

/// [`decrypt_record_in_place`] with an explicit key (no env/file lookup; used by tests).
pub fn decrypt_record_with_key(
    record: &mut crate::memory::store::MemoryRecord,
    node_key: &[u8; 32],
) -> Result<()> {
    let wrapped = match &record.encrypted_dek {
        None => return Ok(()),
        Some(b) if b.is_empty() => return Ok(()),
        Some(b) => b.clone(),
    };
    if is_default_space_row(&wrapped) {
        anyhow::bail!(
            "at_rest: record {} belongs to the default-space keystore, not the node record key",
            record.id
        );
    }
    let dek = unwrap_dek(&wrapped, node_key)?;

    if let Some(content_iv) = &record.content_iv {
        let ciphertext = crate::utils::crypto::hex_decode(&record.content)?;
        let nonce: [u8; 12] = content_iv
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid content IV"))?;
        let plaintext = decrypt_data(&ciphertext, &dek, &nonce)
            .map_err(|e| anyhow::anyhow!("content decryption failed: {e}"))?;
        record.content = String::from_utf8(plaintext)?;
    }

    if let Some(metadata_iv) = &record.metadata_iv {
        if let Some(enc_hex) = record.metadata.get("encrypted").and_then(|v| v.as_str()) {
            let ciphertext = crate::utils::crypto::hex_decode(enc_hex)?;
            let nonce: [u8; 12] = metadata_iv
                .as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("invalid metadata IV"))?;
            let plaintext = decrypt_data(&ciphertext, &dek, &nonce)
                .map_err(|e| anyhow::anyhow!("metadata decryption failed: {e}"))?;
            record.metadata = serde_json::from_slice(&plaintext)?;
        }
    }
    Ok(())
}

/// Decrypt with a pre-resolved key (`None` = resolve now). Used inside SQL
/// closures so the key file is read once per query instead of once per row.
pub fn decrypt_with_resolved_key(
    record: &mut crate::memory::store::MemoryRecord,
    node_key: Option<&[u8; 32]>,
) -> Result<()> {
    let wrapped = match &record.encrypted_dek {
        None => return Ok(()),
        Some(b) if b.is_empty() => return Ok(()),
        Some(b) => b.clone(),
    };
    if is_default_space_row(&wrapped) {
        return decrypt_default_space_in_place(record, None);
    }
    if is_wrapped_v2(&wrapped) {
        match node_key {
            Some(k) => decrypt_record_with_key(record, k),
            None => decrypt_record_in_place(record),
        }
    } else {
        crate::memory::sqlite_store::SqliteMemoryStore::decrypt_record(record)
    }
}

/// Encrypt every legacy row (`encrypted_dek` NULL/empty) in `conn`.
/// Returns the number of migrated rows. Rows already carrying a DEK are skipped.
/// Runs `VACUUM` afterwards (best effort) so freed plaintext pages leave the file.
pub fn migrate_connection(conn: &rusqlite::Connection) -> Result<usize> {
    let (key, source) = resolve_record_key_with_source();
    let Some(node_key) = key else {
        let reason = match &source {
            KeySource::Unavailable(r) => r.clone(),
            _ => "unknown".to_string(),
        };
        anyhow::bail!("at_rest: cannot migrate without a node key ({reason})");
    };

    let pending: Vec<(String, String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, content, metadata FROM memory_records WHERE encrypted_dek IS NULL OR length(encrypted_dek) = 0",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };

    let tx = conn.unchecked_transaction()?;
    let mut migrated = 0usize;
    for (id, content, metadata_str) in &pending {
        let mut record = crate::memory::store::MemoryRecord {
            id: id.clone(),
            content: content.clone(),
            metadata: serde_json::from_str(metadata_str).unwrap_or_default(),
            ..Default::default()
        };
        encrypt_columns_for_write_with_key(&mut record, &node_key)?;
        tx.execute(
            "UPDATE memory_records SET content = ?1, metadata = ?2, encrypted_dek = ?3, content_iv = ?4, metadata_iv = ?5 WHERE id = ?6",
            rusqlite::params![
                record.content,
                serde_json::to_string(&record.metadata).unwrap_or_default(),
                record.encrypted_dek,
                record.content_iv,
                record.metadata_iv,
                id,
            ],
        )?;
        migrated += 1;
    }
    tx.commit()?;

    // Best effort: wipe freed plaintext pages from the file.
    if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;") {
        tracing::warn!("at_rest: post-migration VACUUM failed (data is encrypted, file may retain old free pages): {e}");
    }
    Ok(migrated)
}

// ===========================================================================
// Default-space encryption (`XDK2`)
// ===========================================================================

/// Marker in `encrypted_dek` for rows sealed under the default-space key.
const DEFAULT_SPACE_MAGIC: &[u8; 4] = b"XDK2";
const KIND_CONTENT: &str = "memory.content";
const KIND_METADATA: &str = "memory.metadata";
/// Content served for an `XDK2` row whose key is locked or missing.
pub const LOCKED_PLACEHOLDER: &str = "[locked: private record, key not available]";

/// Whether an `encrypted_dek` blob marks a default-space (`XDK2`) row.
pub fn is_default_space_row(wrapped: &[u8]) -> bool {
    wrapped.starts_with(DEFAULT_SPACE_MAGIC)
}

fn data_dir() -> PathBuf {
    std::env::var(DATA_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::settings::XavierSettings::resolve_data_dir())
}

/// `$XAVIER_DATA_DIR/spaces` (parent of `default/keystore.json`).
pub fn default_spaces_dir() -> PathBuf {
    data_dir().join("spaces")
}

/// Path of the default-space keystore file.
pub fn default_keystore_path() -> PathBuf {
    default_spaces_dir()
        .join(DEFAULT_SPACE_ID)
        .join(crate::espacio::keys::KEYSTORE_FILE)
}

/// Whether the default-space keystore exists (never creates anything).
pub fn default_keystore_exists() -> bool {
    default_keystore_path().is_file()
}

fn env_record_key() -> Option<[u8; 32]> {
    let raw = std::env::var(RECORD_KEY_ENV).ok()?;
    let hex = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
    let bytes = crate::crypto::hex_decode(hex).ok()?;
    <[u8; 32]>::try_from(bytes.as_slice()).ok()
}

/// Node KEK of the default space: an explicit `XAVIER_RECORD_KEY` (operator
/// supplied secret, also the test path) wins; otherwise the node master key
/// (OS keyring / master-key fallback file, which lives outside the data dir).
fn default_node_kek() -> Result<Arc<dyn NodeKek>> {
    if let Some(k) = env_record_key() {
        return Ok(Arc::new(StaticNodeKek::new(k)));
    }
    Ok(Arc::new(MasterNodeKek::load_or_init()?))
}

fn default_keyring() -> Result<Arc<KeyRing>> {
    Ok(KeyRing::new(default_spaces_dir(), default_node_kek()?))
}

type KeyFingerprint = (Option<std::time::SystemTime>, u64);
type KeyCache = Mutex<std::collections::HashMap<PathBuf, (KeyFingerprint, KeyHandle)>>;

fn key_cache() -> &'static KeyCache {
    static CACHE: OnceLock<KeyCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// Create the default-space keystore (node unlock + recovery code). The caller
/// must show the recovery code exactly once. Refuses to overwrite.
pub fn create_default_keystore() -> Result<(KeyHandle, RecoveryCode)> {
    default_keyring()?.create_keys(DEFAULT_SPACE_ID, UnlockMode::NodeUnlock, None)
}

/// Unlock the default-space key with the node wrapper, cached per keystore
/// file (a changed or deleted keystore invalidates the cache). Missing
/// keystore => `KeysError::KeystoreMissing`; any unlock failure is an error.
pub fn default_space_key() -> Result<KeyHandle> {
    let path = default_keystore_path();
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => {
            key_cache()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&path);
            return Err(if e.kind() == std::io::ErrorKind::NotFound {
                KeysError::KeystoreMissing(DEFAULT_SPACE_ID.to_string()).into()
            } else {
                KeysError::Corrupt(e.to_string()).into()
            });
        }
    };
    let fp: KeyFingerprint = (meta.modified().ok(), meta.len());
    if let Some((cached_fp, h)) = key_cache()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&path)
    {
        if *cached_fp == fp {
            return Ok(h.clone());
        }
    }
    let handle = default_keyring()?.unlock_with_node(DEFAULT_SPACE_ID)?;
    key_cache()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(path, (fp, handle.clone()));
    Ok(handle)
}

/// Drop the cached default-space key (lock it for this process).
pub fn lock_default_space() {
    key_cache()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clear();
}

/// Unlock the default-space key with the recovery code (CLI recovery path).
pub fn unlock_default_with_recovery(code: &str) -> Result<KeyHandle> {
    default_keyring()?.unlock_with_recovery(DEFAULT_SPACE_ID, code)
}

// ---- classification ----

/// Explicitly public: `metadata.clearance` is the string `UNCLASSIFIED` or
/// `PUBLIC` (case-insensitive). Everything else, including a missing or
/// non-string clearance and unknown values, is NOT explicit public.
pub fn is_explicitly_public(metadata: &serde_json::Value) -> bool {
    matches!(
        metadata
            .get("clearance")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_uppercase())
            .as_deref(),
        Some("UNCLASSIFIED") | Some("PUBLIC")
    )
}

/// Private = everything except records explicitly marked public. Fail closed:
/// missing/garbled metadata (not a JSON object), a missing or unknown
/// clearance, INTERNAL (the default) and anything under `segments/` are private.
pub fn is_private_record(metadata: &serde_json::Value, path: &str) -> bool {
    if crate::security::groups::segment_level_from_path(path).is_some() {
        return true;
    }
    !(metadata.is_object() && is_explicitly_public(metadata))
}

/// Integer stored in the plaintext `clearance_level` column (0 = explicit
/// public, 1..=5 = INTERNAL..TOP_SECRET; unknown values store 5). Never
/// returns 0 for a private record.
pub fn clearance_level_for(metadata: &serde_json::Value, path: &str) -> i64 {
    let level = crate::security::clearance::level_from_metadata(metadata) as i64;
    if is_private_record(metadata, path) {
        level.max(1)
    } else {
        0
    }
}

// ---- XDK2 codec ----

/// Seal a record under the default-space key. Returns the values for the
/// `content` and `metadata` columns. `metadata_json` is sealed verbatim.
pub fn seal_default_space(
    dek: &KeyHandle,
    record_id: &str,
    content: &str,
    metadata_json: &str,
) -> Result<(String, String)> {
    let c = dek.seal_record(DEFAULT_SPACE_ID, KIND_CONTENT, record_id, content)?;
    let m = dek.seal_record(DEFAULT_SPACE_ID, KIND_METADATA, record_id, metadata_json)?;
    Ok((c, serde_json::json!({ "encrypted": m }).to_string()))
}

/// Open the `content` and `metadata` columns of an `XDK2` row. Returns the
/// plaintext content and the exact metadata JSON string that was sealed.
pub fn open_default_space(
    dek: &KeyHandle,
    record_id: &str,
    content_col: &str,
    metadata_col: &str,
) -> Result<(String, String)> {
    let meta: serde_json::Value = serde_json::from_str(metadata_col)
        .map_err(|_| anyhow::anyhow!("XDK2 metadata column is not JSON"))?;
    let sealed_meta = meta
        .get("encrypted")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("XDK2 metadata column is not encrypted"))?;
    let content = dek.open_record(DEFAULT_SPACE_ID, KIND_CONTENT, record_id, content_col)?;
    let metadata = dek.open_record(DEFAULT_SPACE_ID, KIND_METADATA, record_id, sealed_meta)?;
    Ok((content, metadata))
}

/// Whether `record` is the locked placeholder produced for an undecryptable row.
pub fn is_locked_placeholder(record: &crate::memory::store::MemoryRecord) -> bool {
    record.content == LOCKED_PLACEHOLDER
        && record.metadata.get("locked").and_then(|v| v.as_bool()) == Some(true)
}

fn mark_locked(record: &mut crate::memory::store::MemoryRecord) {
    record.content = LOCKED_PLACEHOLDER.to_string();
    record.metadata = serde_json::json!({ "locked": true });
}

fn decrypt_default_space_in_place(
    record: &mut crate::memory::store::MemoryRecord,
    dek: Option<&KeyHandle>,
) -> Result<()> {
    let res = (|| -> Result<()> {
        let owned;
        let dek = match dek {
            Some(d) => d,
            None => {
                owned = default_space_key()?;
                &owned
            }
        };
        let meta_col = serde_json::to_string(&record.metadata)?;
        let (content, metadata_json) =
            open_default_space(dek, &record.id, &record.content, &meta_col)?;
        let metadata: serde_json::Value =
            serde_json::from_str(&metadata_json).unwrap_or_else(|_| serde_json::json!({}));
        record.clearance = crate::security::clearance::level_from_metadata(&metadata);
        record.content = content;
        record.metadata = metadata;
        Ok(())
    })();
    if res.is_err() {
        // Never leave ciphertext (or stale columns) where callers read content.
        mark_locked(record);
    }
    res
}

/// Classification-aware write encryption. Private records use the
/// default-space key (`XDK2`) once its keystore exists; public records and
/// private records while no keystore exists keep the node-record-key
/// envelope (`XRK1`) exactly as before. If the keystore exists but cannot be
/// unlocked, the write falls back to `XRK1` (still ciphertext) with a warning
/// and the row is picked up by `encrypt-private --resume`.
/// Also sets `record.clearance` to the resolved level (written to the
/// plaintext `clearance_level` column by the store).
pub fn encrypt_for_write(record: &mut crate::memory::store::MemoryRecord) -> Result<bool> {
    let private = is_private_record(&record.metadata, &record.path);
    record.clearance = crate::security::clearance::ClearanceLevel::from(clearance_level_for(
        &record.metadata,
        &record.path,
    ) as u8);
    if private && default_keystore_exists() {
        match default_space_key() {
            Ok(dek) => {
                let metadata_json = serde_json::to_string(&record.metadata)?;
                let (c, m) = seal_default_space(&dek, &record.id, &record.content, &metadata_json)?;
                record.content = c;
                record.metadata = serde_json::from_str(&m)?;
                record.encrypted_dek = Some(DEFAULT_SPACE_MAGIC.to_vec());
                record.content_iv = None;
                record.metadata_iv = None;
                return Ok(true);
            }
            Err(e) => {
                tracing::warn!(
                    "at_rest: default-space key unavailable ({e}); private write falls back to the node record key"
                );
            }
        }
    }
    encrypt_columns_for_write(record)
}

// ---- schema ----

/// Idempotent: plaintext `clearance_level` column plus the job, failure and
/// verification tables used by `encrypt-private` / `decrypt-private`.
pub fn ensure_private_encryption_schema(conn: &Connection) -> Result<()> {
    let has_col: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('memory_records') WHERE name = 'clearance_level'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)?;
    if !has_col {
        conn.execute_batch("ALTER TABLE memory_records ADD COLUMN clearance_level INTEGER")?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memory_encrypt_job (
            job_id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL DEFAULT 'encrypt',
            phase TEXT NOT NULL,
            last_rowid INTEGER NOT NULL DEFAULT 0,
            total INTEGER NOT NULL DEFAULT 0,
            done INTEGER NOT NULL DEFAULT 0,
            failed INTEGER NOT NULL DEFAULT 0,
            backup_path TEXT,
            backup_sha256 TEXT,
            started_at TEXT,
            updated_at TEXT
        );
        CREATE TABLE IF NOT EXISTS memory_encrypt_failures (
            id TEXT NOT NULL,
            job_id INTEGER NOT NULL,
            reason TEXT NOT NULL,
            PRIMARY KEY (job_id, id)
        );
        CREATE TABLE IF NOT EXISTS memory_encrypt_plain_hash (
            job_id INTEGER NOT NULL,
            id TEXT NOT NULL,
            mac TEXT NOT NULL,
            PRIMARY KEY (job_id, id)
        );",
    )?;
    Ok(())
}

// ---- batches ----

/// Result of one batch.
#[derive(Debug, Clone, Default)]
pub struct BatchOutcome {
    /// Highest rowid scanned (the new cursor).
    pub last_rowid: i64,
    pub scanned: u64,
    /// Rows rewritten (encrypted / decrypted / restored).
    pub changed: u64,
    /// Rows classified public and left alone.
    pub skipped_public: u64,
    /// Rows already in the target state (or legacy-KEK rows we do not touch).
    pub skipped_done: u64,
    pub failed: u64,
}

struct RawRow {
    rowid: i64,
    id: String,
    path: String,
    content: String,
    metadata: String,
    dek: Option<Vec<u8>>,
    content_iv: Option<Vec<u8>>,
    metadata_iv: Option<Vec<u8>>,
}

enum RowOutcome {
    Changed,
    Public,
    Done,
    Failed(String),
}

fn fetch_rows(
    conn: &Connection,
    after: i64,
    limit: usize,
) -> Result<Vec<std::result::Result<RawRow, (i64, String)>>> {
    let mut stmt = conn.prepare(
        "SELECT rowid, id, path, content, metadata, encrypted_dek, content_iv, metadata_iv \
         FROM memory_records WHERE rowid > ?1 ORDER BY rowid LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![after, limit as i64], |row| {
        let rowid: i64 = row.get(0)?;
        let parsed = (|| -> rusqlite::Result<RawRow> {
            Ok(RawRow {
                rowid,
                id: row.get(1)?,
                path: row.get(2)?,
                content: row.get(3)?,
                metadata: row.get(4)?,
                dek: row.get(5)?,
                content_iv: row.get(6)?,
                metadata_iv: row.get(7)?,
            })
        })();
        Ok(parsed.map_err(|_| {
            (
                rowid,
                "unreadable row (unexpected column types or invalid UTF-8)".to_string(),
            )
        }))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn record_failure(conn: &Connection, job_id: i64, id: &str, reason: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO memory_encrypt_failures(id, job_id, reason) VALUES (?1, ?2, ?3)",
        params![id, job_id, reason],
    )?;
    Ok(())
}

fn fts_rewrite(conn: &Connection, id: &str, path: &str, plain_content: Option<&str>) -> Result<()> {
    conn.execute("DELETE FROM memory_fts WHERE id = ?", params![id])?;
    let (fts_content, tokens) = match plain_content {
        None => (String::new(), super::fts::code_tokens(path).join(" ")),
        Some(c) => (
            c.to_string(),
            super::fts::code_tokens(&format!("{path} {c}")).join(" "),
        ),
    };
    conn.execute(
        "INSERT INTO memory_fts(id, path, content, code_tokens) VALUES (?, ?, ?, ?)",
        params![id, path, fts_content, tokens],
    )?;
    Ok(())
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn update_cursor(conn: &Connection, job_id: i64, o: &BatchOutcome) -> Result<()> {
    conn.execute(
        "UPDATE memory_encrypt_job SET last_rowid = ?1, done = done + ?2, failed = failed + ?3, updated_at = ?4 WHERE job_id = ?5",
        params![o.last_rowid, o.changed as i64, o.failed as i64, now_rfc3339(), job_id],
    )?;
    Ok(())
}

fn with_immediate_tx<T>(conn: &Connection, f: impl FnOnce() -> Result<T>) -> Result<T> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    match f() {
        Ok(v) => match conn.execute_batch("COMMIT") {
            Ok(()) => Ok(v),
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e.into())
            }
        },
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

fn row_mac(dek: &KeyHandle, content: &str, metadata_json: &str) -> String {
    let mut data = Vec::with_capacity(content.len() + metadata_json.len() + 1);
    data.extend_from_slice(content.as_bytes());
    data.push(0);
    data.extend_from_slice(metadata_json.as_bytes());
    dek.verify_mac("row", &data)
}

fn migrate_one(
    conn: &Connection,
    dek: &KeyHandle,
    node_key: Option<&[u8; 32]>,
    job_id: i64,
    row: &RawRow,
) -> Result<RowOutcome> {
    let wrapped = row.dek.as_deref().unwrap_or(&[]);
    let is_xrk1 = !wrapped.is_empty() && is_wrapped_v2(wrapped);
    if is_default_space_row(wrapped) || (!wrapped.is_empty() && !is_xrk1) {
        return Ok(RowOutcome::Done);
    }
    let (content, metadata_json) = if is_xrk1 {
        let Some(nk) = node_key else {
            return Ok(RowOutcome::Failed(
                "node record key unavailable for XRK1 row".into(),
            ));
        };
        let mut rec = crate::memory::store::MemoryRecord {
            id: row.id.clone(),
            content: row.content.clone(),
            metadata: serde_json::from_str(&row.metadata).unwrap_or_default(),
            encrypted_dek: row.dek.clone(),
            content_iv: row.content_iv.clone(),
            metadata_iv: row.metadata_iv.clone(),
            ..Default::default()
        };
        if decrypt_record_with_key(&mut rec, nk).is_err() {
            return Ok(RowOutcome::Failed(
                "XRK1 row did not decrypt with the node record key".into(),
            ));
        }
        (rec.content, serde_json::to_string(&rec.metadata)?)
    } else {
        (row.content.clone(), row.metadata.clone())
    };
    // Garbled metadata parses to Null: classified private (fail closed).
    let meta_val: serde_json::Value =
        serde_json::from_str(&metadata_json).unwrap_or(serde_json::Value::Null);
    let level = clearance_level_for(&meta_val, &row.path);
    if !is_private_record(&meta_val, &row.path) {
        conn.execute(
            "UPDATE memory_records SET clearance_level = ?1 WHERE rowid = ?2",
            params![level, row.rowid],
        )?;
        return Ok(RowOutcome::Public);
    }
    conn.execute(
        "INSERT OR REPLACE INTO memory_encrypt_plain_hash(job_id, id, mac) VALUES (?1, ?2, ?3)",
        params![job_id, row.id, row_mac(dek, &content, &metadata_json)],
    )?;
    let (c, m) = seal_default_space(dek, &row.id, &content, &metadata_json)?;
    conn.execute(
        "UPDATE memory_records SET content = ?1, metadata = ?2, encrypted_dek = ?3, content_iv = NULL, metadata_iv = NULL, clearance_level = ?4 WHERE rowid = ?5",
        params![c, m, DEFAULT_SPACE_MAGIC.to_vec(), level, row.rowid],
    )?;
    fts_rewrite(conn, &row.id, &row.path, None)?;
    Ok(RowOutcome::Changed)
}

/// Encrypt one batch of private rows (`rowid > last_rowid`) in a single
/// `BEGIN IMMEDIATE` transaction that also advances the job cursor. A row
/// error that is not a database error is recorded in
/// `memory_encrypt_failures` and the row is left unchanged. Embeddings,
/// `memory_chain` and graph tables are never touched.
/// `abort_after_rows` is a crash-injection point for tests (rolls back).
pub fn migrate_batch(
    conn: &Connection,
    dek: &KeyHandle,
    node_key: Option<&[u8; 32]>,
    job_id: i64,
    last_rowid: i64,
    limit: usize,
    abort_after_rows: Option<usize>,
) -> Result<BatchOutcome> {
    with_immediate_tx(conn, || {
        let rows = fetch_rows(conn, last_rowid, limit)?;
        let mut out = BatchOutcome {
            last_rowid,
            ..Default::default()
        };
        for (n, row) in rows.iter().enumerate() {
            if abort_after_rows.is_some_and(|k| n >= k) {
                anyhow::bail!("injected abort inside batch");
            }
            out.scanned += 1;
            match row {
                Err((rowid, reason)) => {
                    out.last_rowid = *rowid;
                    record_failure(conn, job_id, &format!("rowid:{rowid}"), reason)?;
                    out.failed += 1;
                }
                Ok(r) => {
                    out.last_rowid = r.rowid;
                    match migrate_one(conn, dek, node_key, job_id, r)? {
                        RowOutcome::Changed => out.changed += 1,
                        RowOutcome::Public => out.skipped_public += 1,
                        RowOutcome::Done => out.skipped_done += 1,
                        RowOutcome::Failed(reason) => {
                            record_failure(conn, job_id, &r.id, &reason)?;
                            out.failed += 1;
                        }
                    }
                }
            }
        }
        update_cursor(conn, job_id, &out)?;
        Ok(out)
    })
}

/// Reverse of [`migrate_batch`] by key: `XDK2` rows go back to plaintext
/// columns (content and metadata restored verbatim, `encrypted_dek` NULL) and
/// the FTS row gets its full-text form again. Rows that do not decrypt are
/// recorded as failures and left encrypted.
pub fn reverse_batch(
    conn: &Connection,
    dek: &KeyHandle,
    job_id: i64,
    last_rowid: i64,
    limit: usize,
) -> Result<BatchOutcome> {
    with_immediate_tx(conn, || {
        let rows = fetch_rows(conn, last_rowid, limit)?;
        let mut out = BatchOutcome {
            last_rowid,
            ..Default::default()
        };
        for row in &rows {
            out.scanned += 1;
            let r = match row {
                Err((rowid, reason)) => {
                    out.last_rowid = *rowid;
                    record_failure(conn, job_id, &format!("rowid:{rowid}"), reason)?;
                    out.failed += 1;
                    continue;
                }
                Ok(r) => r,
            };
            out.last_rowid = r.rowid;
            if !r.dek.as_deref().is_some_and(is_default_space_row) {
                out.skipped_done += 1;
                continue;
            }
            match open_default_space(dek, &r.id, &r.content, &r.metadata) {
                Err(_) => {
                    record_failure(
                        conn,
                        job_id,
                        &r.id,
                        "XDK2 row did not decrypt with the default-space key",
                    )?;
                    out.failed += 1;
                }
                Ok((content, metadata_json)) => {
                    conn.execute(
                        "UPDATE memory_records SET content = ?1, metadata = ?2, encrypted_dek = NULL, content_iv = NULL, metadata_iv = NULL WHERE rowid = ?3",
                        params![content, metadata_json, r.rowid],
                    )?;
                    fts_rewrite(conn, &r.id, &r.path, Some(&content))?;
                    out.changed += 1;
                }
            }
        }
        update_cursor(conn, job_id, &out)?;
        Ok(out)
    })
}

/// Restore `XDK2` rows from a verified backup (key lost path). Content,
/// metadata and the three crypto columns are copied byte-for-byte from the
/// backup row of the same id; rows missing from the backup, or encrypted
/// under `XDK2` there, are recorded as failures and left as they are.
pub fn restore_batch_from_backup(
    conn: &Connection,
    backup: &Connection,
    job_id: i64,
    last_rowid: i64,
    limit: usize,
) -> Result<BatchOutcome> {
    type BackupCols = (
        String,
        String,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
    );
    with_immediate_tx(conn, || {
        let rows = fetch_rows(conn, last_rowid, limit)?;
        let mut out = BatchOutcome {
            last_rowid,
            ..Default::default()
        };
        for row in &rows {
            out.scanned += 1;
            let r = match row {
                Err((rowid, reason)) => {
                    out.last_rowid = *rowid;
                    record_failure(conn, job_id, &format!("rowid:{rowid}"), reason)?;
                    out.failed += 1;
                    continue;
                }
                Ok(r) => r,
            };
            out.last_rowid = r.rowid;
            if !r.dek.as_deref().is_some_and(is_default_space_row) {
                out.skipped_done += 1;
                continue;
            }
            let orig: Option<BackupCols> = backup
                .query_row(
                    "SELECT content, metadata, encrypted_dek, content_iv, metadata_iv FROM memory_records WHERE id = ?1",
                    params![r.id],
                    |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?, x.get(3)?, x.get(4)?)),
                )
                .ok();
            let Some((content, metadata, dek, civ, miv)) = orig else {
                record_failure(conn, job_id, &r.id, "row not present in the backup")?;
                out.failed += 1;
                continue;
            };
            if dek.as_deref().is_some_and(is_default_space_row) {
                record_failure(
                    conn,
                    job_id,
                    &r.id,
                    "backup row is itself XDK2 (no plaintext source)",
                )?;
                out.failed += 1;
                continue;
            }
            let plain_for_fts = if dek.as_deref().is_none_or(|d| d.is_empty()) {
                Some(content.as_str())
            } else {
                None
            };
            conn.execute(
                "UPDATE memory_records SET content = ?1, metadata = ?2, encrypted_dek = ?3, content_iv = ?4, metadata_iv = ?5 WHERE rowid = ?6",
                params![content, metadata, dek, civ, miv, r.rowid],
            )?;
            fts_rewrite(conn, &r.id, &r.path, plain_for_fts)?;
            out.changed += 1;
        }
        update_cursor(conn, job_id, &out)?;
        Ok(out)
    })
}

/// Full decrypt-and-compare of every row recorded in the side table against
/// the keyed hash taken before the write. Returns (checked, mismatched ids).
pub fn verify_encrypt_job(
    conn: &Connection,
    dek: &KeyHandle,
    job_id: i64,
) -> Result<(u64, Vec<String>)> {
    let mut stmt = conn.prepare(
        "SELECT h.id, h.mac, r.content, r.metadata, r.encrypted_dek FROM memory_encrypt_plain_hash h \
         LEFT JOIN memory_records r ON r.id = h.id WHERE h.job_id = ?1",
    )?;
    let mut rows = stmt.query(params![job_id])?;
    let mut checked = 0u64;
    let mut bad = Vec::new();
    while let Some(row) = rows.next()? {
        checked += 1;
        let id: String = row.get(0)?;
        let mac: String = row.get(1)?;
        let content: Option<String> = row.get(2)?;
        let metadata: Option<String> = row.get(3)?;
        let wrapped: Option<Vec<u8>> = row.get(4)?;
        let ok = match (content, metadata, wrapped) {
            (Some(c), Some(m), Some(w)) if is_default_space_row(&w) => {
                open_default_space(dek, &id, &c, &m)
                    .map(|(pc, pm)| row_mac(dek, &pc, &pm) == mac)
                    .unwrap_or(false)
            }
            _ => false,
        };
        if !ok {
            bad.push(id);
        }
    }
    Ok((checked, bad))
}

// ---- job orchestration ----

/// Counts printed by a dry run (read-only; creates no key, writes nothing).
#[derive(Debug, Clone, Default)]
pub struct DryRunReport {
    pub total: u64,
    /// Plaintext rows by declared clearance (`<absent>` / `<garbled>` included).
    pub plaintext_by_clearance: std::collections::BTreeMap<String, u64>,
    pub plaintext_private: u64,
    pub plaintext_public: u64,
    pub already_xrk1: u64,
    pub already_xdk2: u64,
    pub legacy_other: u64,
    pub keystore_exists: bool,
    pub clearance_column: bool,
}

/// Read-only census of the database.
pub fn dry_run_report(db_path: &Path) -> Result<DryRunReport> {
    let conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("cannot open {} read-only", db_path.display()))?;
    let clearance_column = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('memory_records') WHERE name = 'clearance_level'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap_or(false);
    let mut rep = DryRunReport {
        keystore_exists: default_keystore_exists(),
        clearance_column,
        ..Default::default()
    };
    let mut stmt = conn
        .prepare("SELECT path, metadata, encrypted_dek FROM memory_records")
        .context("memory_records SELECT failed (is this a Xavier memory DB?)")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        rep.total += 1;
        let path: String = row.get::<_, Option<String>>(0)?.unwrap_or_default();
        let metadata: String = row.get::<_, Option<String>>(1)?.unwrap_or_default();
        let wrapped: Vec<u8> = row.get::<_, Option<Vec<u8>>>(2)?.unwrap_or_default();
        if wrapped.is_empty() {
            let val: serde_json::Value =
                serde_json::from_str(&metadata).unwrap_or(serde_json::Value::Null);
            let label = match val.get("clearance") {
                _ if !val.is_object() => "<garbled>".to_string(),
                None => "<absent>".to_string(),
                Some(v) => v
                    .as_str()
                    .map(|s| s.trim().to_ascii_uppercase())
                    .unwrap_or_else(|| "<garbled>".to_string()),
            };
            *rep.plaintext_by_clearance.entry(label).or_insert(0) += 1;
            if is_private_record(&val, &path) {
                rep.plaintext_private += 1;
            } else {
                rep.plaintext_public += 1;
            }
        } else if is_default_space_row(&wrapped) {
            rep.already_xdk2 += 1;
        } else if is_wrapped_v2(&wrapped) {
            rep.already_xrk1 += 1;
        } else {
            rep.legacy_other += 1;
        }
    }
    Ok(rep)
}

/// Test-only fault injection (all off by default).
#[derive(Debug, Clone, Default)]
pub struct TestHooks {
    /// Flip a byte of the backup after it was written and hashed.
    pub corrupt_backup: bool,
    /// Return an error after N committed batches (simulates a kill between batches).
    pub abort_after_batches: Option<usize>,
    /// (batch index, rows) abort inside that batch before commit (rolls back).
    pub abort_in_batch: Option<(usize, usize)>,
}

/// Options shared by `apply_encrypt` / `apply_decrypt`.
#[derive(Debug, Clone)]
pub struct JobOptions {
    pub db_path: PathBuf,
    /// Backup target file (or existing directory). Required unless `resume`.
    pub backup_path: Option<PathBuf>,
    pub batch: usize,
    pub max_rows: Option<u64>,
    pub resume: bool,
    /// Allow running while another process has the DB open (busy timeout +
    /// IMMEDIATE transactions). Default is the offline guard.
    pub online: bool,
    pub busy_timeout_ms: u64,
    pub rate_limit_ms: u64,
    pub stop: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub hooks: TestHooks,
}

impl JobOptions {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
            backup_path: None,
            batch: 200,
            max_rows: None,
            resume: false,
            online: false,
            busy_timeout_ms: 5000,
            rate_limit_ms: 0,
            stop: None,
            hooks: TestHooks::default(),
        }
    }
}

/// Outcome of a job run.
#[derive(Debug, Clone, Default)]
pub struct JobReport {
    pub job_id: i64,
    pub backup_path: Option<PathBuf>,
    pub backup_sha256: Option<String>,
    pub scanned: u64,
    pub changed: u64,
    pub skipped_public: u64,
    pub skipped_done: u64,
    pub failed: u64,
    pub verified: u64,
    /// False when stopped early (SIGINT / `max_rows`); run again with resume.
    pub complete: bool,
    pub keystore_created: bool,
}

/// What `apply_decrypt` reads plaintext from.
pub enum DecryptSource {
    /// Decrypt with the default-space key (node unlock when `None`).
    Key(Option<KeyHandle>),
    /// Copy original columns from a plaintext backup.
    FromBackup(PathBuf),
}

fn sha256_file(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut f =
        std::fs::File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(crate::crypto::hex_encode(h.finalize()))
}

/// Free bytes on the filesystem holding `path` (longest matching mount point).
fn free_bytes(path: &Path) -> Option<u64> {
    let canon = std::fs::canonicalize(path).ok()?;
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| canon.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

/// First other process (Linux `/proc`) that holds the DB, its WAL or its SHM open.
fn other_process_with_db_open(db_path: &Path) -> Option<u32> {
    let me = std::process::id();
    let base = std::fs::canonicalize(db_path).ok()?;
    let mut names = vec![base.clone()];
    for suffix in ["-wal", "-shm"] {
        let mut o = base.clone().into_os_string();
        o.push(suffix);
        names.push(PathBuf::from(o));
    }
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == me {
            continue;
        }
        let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Ok(target) = std::fs::read_link(fd.path()) {
                if names.contains(&target) {
                    return Some(pid);
                }
            }
        }
    }
    None
}

fn open_job_conn(opts: &JobOptions) -> Result<Connection> {
    super::vector::register_sqlite_vec_extension()?;
    let conn = Connection::open(&opts.db_path)
        .with_context(|| format!("cannot open {}", opts.db_path.display()))?;
    if opts.online {
        conn.busy_timeout(std::time::Duration::from_millis(
            opts.busy_timeout_ms.max(1),
        ))?;
    } else {
        if let Some(pid) = other_process_with_db_open(&opts.db_path) {
            anyhow::bail!(
                "database is open by process {pid}: stop the daemon first, or pass --online to run with a busy timeout"
            );
        }
        conn.busy_timeout(std::time::Duration::from_millis(0))?;
        conn.execute_batch("BEGIN EXCLUSIVE; ROLLBACK;")
            .context("database is locked by another connection (offline mode refuses to run)")?;
        conn.busy_timeout(std::time::Duration::from_millis(2000))?;
    }
    let qc: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if qc != "ok" {
        anyhow::bail!("quick_check failed on the source database: {qc}");
    }
    if let Some(free) = free_bytes(opts.db_path.parent().unwrap_or(Path::new("."))) {
        let size = std::fs::metadata(&opts.db_path)
            .map(|m| m.len())
            .unwrap_or(0);
        if free < size.saturating_mul(5) / 2 {
            anyhow::bail!("not enough free disk space: need 2.5x the database size ({size} bytes), have {free}");
        }
    }
    Ok(conn)
}

fn count_rows(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM memory_records", [], |r| r.get(0))?)
}

/// Mandatory verified backup: `VACUUM INTO` + SHA-256 manifest + reopen with
/// `quick_check` and row-count comparison. Any failure aborts the job.
fn make_verified_backup(
    conn: &Connection,
    target: &Path,
    hooks: &TestHooks,
) -> Result<(PathBuf, String)> {
    let path = if target.is_dir() {
        target.join(format!(
            "xavier-pre-encrypt-{}.sqlite3",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
        ))
    } else {
        target.to_path_buf()
    };
    if path.exists() {
        anyhow::bail!(
            "backup path {} already exists; refusing to overwrite",
            path.display()
        );
    }
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() && !p.exists() => {
            anyhow::bail!("backup directory {} does not exist", p.display())
        }
        _ => {}
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let dev = |p: &Path| std::fs::metadata(p).ok().map(|m| m.dev());
        let db_dir = conn
            .path()
            .map(PathBuf::from)
            .and_then(|p| p.parent().map(Path::to_path_buf));
        if let (Some(a), Some(b)) = (
            db_dir.as_deref().and_then(dev),
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .and_then(dev),
        ) {
            if a == b {
                eprintln!("warning: backup is on the same filesystem as the database; a disk failure would lose both");
            }
        }
    }
    let src_rows = count_rows(conn)?;
    conn.execute("VACUUM INTO ?1", params![path.to_string_lossy()])
        .context("VACUUM INTO backup failed")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    let sha = sha256_file(&path)?;
    let mut manifest_name = path.clone().into_os_string();
    manifest_name.push(".sha256");
    std::fs::write(
        &manifest_name,
        format!(
            "{sha}  {}\nmemory_records={src_rows}\n",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("backup")
        ),
    )?;
    if hooks.corrupt_backup {
        use std::io::{Seek, SeekFrom, Write};
        let len = std::fs::metadata(&path)?.len();
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)?;
        f.seek(SeekFrom::Start(len / 2))?;
        f.write_all(&[0xFF, 0x00, 0xFF, 0x00])?;
        f.sync_all()?;
    }
    verify_backup(&path, &sha, src_rows)?;
    Ok((path, sha))
}

fn verify_backup(path: &Path, expected_sha: &str, expected_rows: i64) -> Result<()> {
    let actual = sha256_file(path)?;
    if actual != expected_sha {
        anyhow::bail!("backup verification failed: SHA-256 mismatch");
    }
    let b = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("backup verification failed: cannot reopen backup")?;
    let qc: String = b
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .context("backup verification failed: quick_check could not run")?;
    if qc != "ok" {
        anyhow::bail!("backup verification failed: quick_check = {qc}");
    }
    let rows = count_rows(&b).context("backup verification failed: row count")?;
    if rows != expected_rows {
        anyhow::bail!("backup verification failed: row count {rows} != source {expected_rows}");
    }
    Ok(())
}

fn finalize_storage(conn: &Connection) -> Result<()> {
    conn.execute_batch("INSERT INTO memory_fts(memory_fts) VALUES('rebuild')")
        .context("FTS rebuild failed: plaintext terms may remain in the index")?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    conn.execute_batch("VACUUM;")
        .context("VACUUM failed: freed plaintext pages may remain in the file")?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}

struct OpenJob {
    job_id: i64,
    last_rowid: i64,
    backup_path: Option<String>,
    backup_sha256: Option<String>,
}

fn load_open_job(conn: &Connection, kind: &str) -> Result<Option<OpenJob>> {
    Ok(conn
        .query_row(
            "SELECT job_id, last_rowid, backup_path, backup_sha256 FROM memory_encrypt_job \
             WHERE kind = ?1 AND phase != 'done' ORDER BY job_id DESC LIMIT 1",
            params![kind],
            |r| {
                Ok(OpenJob {
                    job_id: r.get(0)?,
                    last_rowid: r.get(1)?,
                    backup_path: r.get(2)?,
                    backup_sha256: r.get(3)?,
                })
            },
        )
        .ok())
}

fn set_phase(conn: &Connection, job_id: i64, phase: &str) -> Result<()> {
    conn.execute(
        "UPDATE memory_encrypt_job SET phase = ?1, updated_at = ?2 WHERE job_id = ?3",
        params![phase, now_rfc3339(), job_id],
    )?;
    Ok(())
}

/// Shared scaffolding: guard, preflight, backup (fresh) or backup re-check
/// (resume), job row. Returns (conn, job_id, start cursor, backup path, sha).
fn start_job(
    opts: &JobOptions,
    kind: &str,
) -> Result<(Connection, i64, i64, PathBuf, String, bool)> {
    let conn = open_job_conn(opts)?;
    ensure_private_encryption_schema(&conn)?;
    let existing = load_open_job(&conn, kind)?;
    if opts.resume {
        let job =
            existing.ok_or_else(|| anyhow::anyhow!("--resume: no unfinished {kind} job found"))?;
        let (Some(bp), Some(sha)) = (job.backup_path, job.backup_sha256) else {
            anyhow::bail!("--resume: job {} has no recorded backup", job.job_id);
        };
        let bp = PathBuf::from(bp);
        if sha256_file(&bp).map(|h| h != sha).unwrap_or(true) {
            anyhow::bail!(
                "--resume: recorded backup {} is missing or does not match its SHA-256",
                bp.display()
            );
        }
        return Ok((conn, job.job_id, job.last_rowid, bp, sha, false));
    }
    if let Some(job) = existing {
        anyhow::bail!(
            "unfinished {kind} job {} exists; run again with --resume",
            job.job_id
        );
    }
    let target = opts.backup_path.as_deref().ok_or_else(|| {
        anyhow::anyhow!("--backup-path is required for --apply (a verified backup is mandatory)")
    })?;
    let (bp, sha) = make_verified_backup(&conn, target, &opts.hooks)?;
    let total = count_rows(&conn)?;
    conn.execute(
        "INSERT INTO memory_encrypt_job(kind, phase, last_rowid, total, backup_path, backup_sha256, started_at, updated_at) \
         VALUES (?1, 'migrate', 0, ?2, ?3, ?4, ?5, ?5)",
        params![kind, total, bp.to_string_lossy(), sha, now_rfc3339()],
    )?;
    let job_id = conn.last_insert_rowid();
    Ok((conn, job_id, 0, bp, sha, true))
}

fn run_batches(
    conn: &Connection,
    opts: &JobOptions,
    job_id: i64,
    start: i64,
    rep: &mut JobReport,
    mut batch_fn: impl FnMut(&Connection, i64, usize, usize) -> Result<BatchOutcome>,
) -> Result<bool> {
    let limit = if opts.batch == 0 { 200 } else { opts.batch };
    let mut cursor = start;
    let mut idx = 0usize;
    loop {
        if opts
            .stop
            .as_ref()
            .is_some_and(|s| s.load(std::sync::atomic::Ordering::SeqCst))
        {
            return Ok(false);
        }
        if opts.max_rows.is_some_and(|m| rep.changed >= m) {
            return Ok(false);
        }
        if opts.hooks.abort_after_batches.is_some_and(|n| idx >= n) {
            anyhow::bail!("injected abort between batches");
        }
        let o = batch_fn(conn, cursor, limit, idx)?;
        rep.scanned += o.scanned;
        rep.changed += o.changed;
        rep.skipped_public += o.skipped_public;
        rep.skipped_done += o.skipped_done;
        rep.failed += o.failed;
        cursor = o.last_rowid;
        idx += 1;
        let _ = job_id;
        if o.scanned < limit as u64 {
            return Ok(true);
        }
        if opts.rate_limit_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(opts.rate_limit_ms));
        }
    }
}

/// `encrypt-private --apply`. `on_recovery_code` receives the recovery code
/// exactly once, as soon as a new keystore was created (before any row is
/// touched).
pub fn apply_encrypt(
    opts: &JobOptions,
    on_recovery_code: &mut dyn FnMut(&str),
) -> Result<JobReport> {
    let (conn, job_id, start, backup_path, sha, fresh) = start_job(opts, "encrypt")?;
    let mut rep = JobReport {
        job_id,
        backup_path: Some(backup_path.clone()),
        backup_sha256: Some(sha),
        ..Default::default()
    };
    let dek = if default_keystore_exists() {
        default_space_key()?
    } else {
        let (h, code) = create_default_keystore()?;
        on_recovery_code(code.expose());
        rep.keystore_created = true;
        h
    };
    if fresh && default_keystore_exists() {
        let mut ks = backup_path.clone().into_os_string();
        ks.push(".keystore.json");
        std::fs::copy(default_keystore_path(), &ks)
            .context("cannot copy the keystore next to the backup")?;
    }
    let has_xrk1: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memory_records WHERE encrypted_dek IS NOT NULL AND substr(encrypted_dek,1,4) = X'58524B31'",
        [],
        |r| r.get(0),
    )?;
    let node_key = if has_xrk1 > 0 {
        resolve_record_key()
    } else {
        None
    };

    let abort_in = opts.hooks.abort_in_batch;
    let complete = run_batches(
        &conn,
        opts,
        job_id,
        start,
        &mut rep,
        |c, cursor, limit, idx| {
            let abort = abort_in.and_then(|(b, n)| (b == idx).then_some(n));
            migrate_batch(c, &dek, node_key.as_ref(), job_id, cursor, limit, abort)
        },
    )?;
    rep.complete = complete;
    if !complete {
        return Ok(rep);
    }

    set_phase(&conn, job_id, "verify")?;
    let (checked, bad) = verify_encrypt_job(&conn, &dek, job_id)?;
    rep.verified = checked;
    if !bad.is_empty() {
        set_phase(&conn, job_id, "verify_failed")?;
        anyhow::bail!(
            "verification failed for {} of {checked} migrated row(s); first id: {}. The backup at {} is intact.",
            bad.len(),
            bad[0],
            backup_path.display()
        );
    }
    set_phase(&conn, job_id, "finalize")?;
    conn.execute(
        "DELETE FROM memory_encrypt_plain_hash WHERE job_id = ?1",
        params![job_id],
    )?;
    finalize_storage(&conn)?;
    set_phase(&conn, job_id, "done")?;
    Ok(rep)
}

/// `decrypt-private --apply`.
pub fn apply_decrypt(opts: &JobOptions, source: DecryptSource) -> Result<JobReport> {
    let (conn, job_id, start, backup_path, sha, _fresh) = start_job(opts, "decrypt")?;
    let mut rep = JobReport {
        job_id,
        backup_path: Some(backup_path),
        backup_sha256: Some(sha),
        ..Default::default()
    };
    let complete = match source {
        DecryptSource::Key(handle) => {
            let dek = match handle {
                Some(h) => h,
                None => default_space_key()?,
            };
            run_batches(
                &conn,
                opts,
                job_id,
                start,
                &mut rep,
                |c, cursor, limit, _| reverse_batch(c, &dek, job_id, cursor, limit),
            )?
        }
        DecryptSource::FromBackup(path) => {
            let b = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .with_context(|| format!("cannot open source backup {}", path.display()))?;
            let qc: String = b.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
            if qc != "ok" {
                anyhow::bail!("source backup failed quick_check: {qc}");
            }
            run_batches(
                &conn,
                opts,
                job_id,
                start,
                &mut rep,
                |c, cursor, limit, _| restore_batch_from_backup(c, &b, job_id, cursor, limit),
            )?
        }
    };
    rep.complete = complete;
    if !complete {
        return Ok(rep);
    }
    set_phase(&conn, job_id, "finalize")?;
    finalize_storage(&conn)?;
    set_phase(&conn, job_id, "done")?;
    Ok(rep)
}

/// Number of failure rows recorded for a job (for CLI reporting).
pub fn job_failures(db_path: &Path, job_id: i64) -> Result<Vec<(String, String)>> {
    let conn = Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn
        .prepare("SELECT id, reason FROM memory_encrypt_failures WHERE job_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map(params![job_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {

    /// Fila cruda de `memory_records` como la devuelve rusqlite.
    type RawRecordRow = (
        String,
        String,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
    );
    use super::*;
    use crate::memory::store::MemoryRecord;
    use rusqlite::params;

    const CANARY: &str = "PLAINTEXT-CANARIO-12345";

    fn test_record(content: &str) -> MemoryRecord {
        MemoryRecord {
            id: "rec_test_1".to_string(),
            workspace_id: "ws1".to_string(),
            path: "notes/canary.md".to_string(),
            content: content.to_string(),
            metadata: serde_json::json!({"topic": "canary", "n": 7}),
            ..Default::default()
        }
    }

    fn test_key(fill: u8) -> [u8; 32] {
        [fill; 32]
    }

    #[test]
    fn at_rest_roundtrip() {
        let key = test_key(0xA5);
        let mut rec = test_record("texto unico de ida y vuelta 98765");
        let before_meta = rec.metadata.clone();
        encrypt_columns_for_write_with_key(&mut rec, &key).unwrap();

        // Nothing in claro: content column is hex ciphertext now.
        assert!(!rec.content.contains("ida y vuelta"));
        assert!(rec.encrypted_dek.is_some());
        assert!(rec.content_iv.is_some());
        assert!(rec.metadata_iv.is_some());
        assert!(is_wrapped_v2(rec.encrypted_dek.as_ref().unwrap()));

        decrypt_record_with_key(&mut rec, &key).unwrap();
        assert_eq!(rec.content, "texto unico de ida y vuelta 98765");
        assert_eq!(rec.metadata, before_meta);
    }

    #[test]
    fn at_rest_adversary_no_plaintext_in_file() {
        // Simulate exactly what put_store persists, on a real file DB.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("adversary.sqlite3");
        let key = test_key(0x3C);

        let canary_content = format!("nota secreta con {}", CANARY);
        let mut rec = test_record(&canary_content);
        encrypt_columns_for_write_with_key(&mut rec, &key).unwrap();

        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE memory_records (
                    id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, path TEXT NOT NULL,
                    content TEXT NOT NULL, metadata TEXT NOT NULL DEFAULT '{}',
                    embedding BLOB, encrypted_dek BLOB, content_iv BLOB, metadata_iv BLOB,
                    created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
                    revision INTEGER NOT NULL DEFAULT 1, primary_flag INTEGER DEFAULT 1
                );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO memory_records (id, workspace_id, path, content, metadata, encrypted_dek, content_iv, metadata_iv, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
                params![
                    rec.id, rec.workspace_id, rec.path, rec.content,
                    serde_json::to_string(&rec.metadata).unwrap(),
                    rec.encrypted_dek, rec.content_iv, rec.metadata_iv,
                ],
            )
            .unwrap();

            // Every written row carries DEK + IVs.
            let (n_total, n_dek, n_iv): (i64, i64, i64) = conn
                .query_row(
                    "SELECT COUNT(*), SUM(encrypted_dek IS NOT NULL AND length(encrypted_dek) > 0), SUM(content_iv IS NOT NULL AND length(content_iv) > 0) FROM memory_records",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(n_total, 1);
            assert_eq!(n_dek, 1, "all rows must carry encrypted_dek");
            assert_eq!(n_iv, 1, "all rows must carry content_iv");
        } // conn closed: buffers flushed.

        // ADVERSARY: raw file bytes must not contain the canary (nor the secret note).
        let raw = std::fs::read(&db_path).unwrap();
        assert!(!raw.is_empty());
        let needle = CANARY.as_bytes();
        let note = "nota secreta".as_bytes();
        assert!(
            raw.windows(needle.len()).all(|w| w != needle),
            "ADVERSARY FAIL: canary plaintext found in {}",
            db_path.display()
        );
        assert!(
            raw.windows(note.len()).all(|w| w != note),
            "ADVERSARY FAIL: note plaintext found in {}",
            db_path.display()
        );
    }

    #[test]
    fn at_rest_wrong_key_fails_loudly() {
        let mut rec = test_record("mensaje que no debe leerse con otra clave");
        encrypt_columns_for_write_with_key(&mut rec, &test_key(0x11)).unwrap();
        let res = decrypt_record_with_key(&mut rec, &test_key(0x22));
        assert!(
            res.is_err(),
            "decrypting with the wrong key must fail, not return garbage"
        );
        // And the content must NOT have been replaced by garbage.
        assert_ne!(rec.content, "mensaje que no debe leerse con otra clave");
    }

    #[test]
    fn at_rest_legacy_row_passthrough() {
        // Inherited row without encrypted_dek keeps being readable.
        let mut rec = test_record("fila heredada en claro");
        rec.encrypted_dek = None;
        rec.content_iv = None;
        rec.metadata_iv = None;
        decrypt_record_in_place(&mut rec).unwrap();
        assert_eq!(rec.content, "fila heredada en claro");
    }

    #[test]
    fn at_rest_key_resolution_order() {
        // Save/restore process env (tests run single-threaded for this module).
        let saved_key = std::env::var(RECORD_KEY_ENV).ok();
        let saved_dir = std::env::var(DATA_DIR_ENV).ok();

        let dir = tempfile::tempdir().unwrap();
        // NOTE: at_rest tests run single-threaded (--test-threads=1): env mutation is safe.
        std::env::set_var(DATA_DIR_ENV, dir.path().to_string_lossy().to_string());
        std::env::remove_var(RECORD_KEY_ENV);

        // (b) Missing file -> generated with 0600.
        let (key1, src1) = resolve_record_key_with_source();
        assert!(key1.is_some());
        let key_path = record_key_path();
        assert!(key_path.exists(), "key file must be generated");
        assert!(matches!(src1, KeySource::Generated(_)));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&key_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "key file must be 0600, got {:o}", mode);
        }
        // Second call reads the same key back from the file.
        let (key2, _) = resolve_record_key_with_source();
        assert_eq!(key1, key2);

        // (a) Env var wins over the file.
        let env_hex = "ab".repeat(32);
        std::env::set_var(RECORD_KEY_ENV, &env_hex);
        let (key3, src3) = resolve_record_key_with_source();
        assert!(matches!(src3, KeySource::Env));
        assert_eq!(key3.unwrap(), [0xABu8; 32]);

        // Restore env.
        match saved_key {
            Some(v) => std::env::set_var(RECORD_KEY_ENV, v),
            None => std::env::remove_var(RECORD_KEY_ENV),
        }
        match saved_dir {
            Some(v) => std::env::set_var(DATA_DIR_ENV, v),
            None => std::env::remove_var(DATA_DIR_ENV),
        }
    }

    #[test]
    fn at_rest_migrate_connection_counts() {
        let key_hex = "cd".repeat(32);
        let saved = std::env::var(RECORD_KEY_ENV).ok();
        // NOTE: at_rest tests run single-threaded (--test-threads=1): env mutation is safe.
        std::env::set_var(RECORD_KEY_ENV, &key_hex);

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE memory_records (
                id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, path TEXT NOT NULL,
                content TEXT NOT NULL, metadata TEXT NOT NULL DEFAULT '{}',
                embedding BLOB, encrypted_dek BLOB, content_iv BLOB, metadata_iv BLOB,
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
                revision INTEGER NOT NULL DEFAULT 1, primary_flag INTEGER DEFAULT 1
            );",
        )
        .unwrap();
        for (id, content) in [("m1", "legacy uno"), ("m2", "legacy dos")] {
            conn.execute(
                "INSERT INTO memory_records (id, workspace_id, path, content, metadata, created_at, updated_at) VALUES (?1,'ws','p',?2,'{\"a\":1}','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
                params![id, content],
            )
            .unwrap();
        }
        // One row already encrypted: must be skipped, not double-encrypted.
        let mut pre = test_record("ya cifrado");
        pre.id = "m3".to_string();
        encrypt_columns_for_write(&mut pre).unwrap();
        let pre_content = pre.content.clone();
        conn.execute(
            "INSERT INTO memory_records (id, workspace_id, path, content, metadata, encrypted_dek, content_iv, metadata_iv, created_at, updated_at) VALUES ('m3','ws','p',?1,?2,?3,?4,?5,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
            params![
                pre.content,
                serde_json::to_string(&pre.metadata).unwrap(),
                pre.encrypted_dek,
                pre.content_iv,
                pre.metadata_iv,
            ],
        )
        .unwrap();

        let migrated = migrate_connection(&conn).unwrap();
        assert_eq!(migrated, 2);

        let left: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memory_records WHERE encrypted_dek IS NULL OR length(encrypted_dek) = 0",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);

        // m3 untouched.
        let m3c: String = conn
            .query_row(
                "SELECT content FROM memory_records WHERE id = 'm3'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(m3c, pre_content);

        // Migrated rows decrypt back to the originals.
        for (id, want) in [("m1", "legacy uno"), ("m2", "legacy dos")] {
            let (c, m, dek, civ, miv): RawRecordRow = conn
                .query_row(
                    "SELECT content, metadata, encrypted_dek, content_iv, metadata_iv FROM memory_records WHERE id = ?1",
                    params![id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
                )
                .unwrap();
            let mut rec = MemoryRecord {
                id: id.to_string(),
                content: c,
                metadata: serde_json::from_str(&m).unwrap_or_default(),
                encrypted_dek: dek,
                content_iv: civ,
                metadata_iv: miv,
                ..Default::default()
            };
            decrypt_record_in_place(&mut rec).unwrap();
            assert_eq!(rec.content, want);
        }

        match saved {
            Some(v) => std::env::set_var(RECORD_KEY_ENV, v),
            None => std::env::remove_var(RECORD_KEY_ENV),
        }
    }
}
