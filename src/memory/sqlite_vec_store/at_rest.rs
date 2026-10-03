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
//! `revisions` (which holds earlier copies of content and metadata) is sealed
//! the same way (kind `memory.revisions`) for `XDK2` rows, and under the record
//! DEK for `XRK1` rows: the column then holds a single sentinel revision whose
//! `content` is the sealed JSON of the real revisions.
//!
//! Once the default keystore exists, a private write that cannot obtain the
//! data key fails (`encrypt_for_write` returns `Err`): it never falls back to
//! the node record key and never stores plaintext.
//!
//! Plaintext that remains by design (listed by [`REMAINING_PLAINTEXT`] in the
//! dry-run / apply output): embeddings (kept in clear locally so vector search
//! works; owner-accepted, never sent to cloud), `path`, timestamps and the
//! plaintext `clearance_level` integer. Derived graph rows, timeline details
//! and chain hashes of migrated rows are scrubbed (see `scrub_residue`).

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
    // `revisions` holds earlier copies of content and metadata: seal it too.
    let plain_revs = serde_json::to_string(&record.revisions)?;
    let col = seal_revisions_xrk1(&dek, &plain_revs)?;
    record.revisions = serde_json::from_str(&col)?;
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

    if !record.revisions.is_empty() {
        let col = serde_json::to_string(&record.revisions)?;
        if sealed_payload(&col).is_some() {
            match open_revisions_xrk1(&dek, &col)
                .and_then(|plain| serde_json::from_str(&plain).map_err(Into::into))
            {
                Ok(revs) => record.revisions = revs,
                Err(e) => {
                    record.revisions.clear();
                    return Err(e);
                }
            }
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
/// Fails closed: a keystore path that cannot be inspected (permissions, I/O
/// error) counts as existing, so private writes are refused instead of
/// silently falling back to a weaker envelope.
pub fn default_keystore_exists() -> bool {
    match std::fs::metadata(default_keystore_path()) {
        Ok(m) => m.is_file() || m.is_dir(),
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    }
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

/// Which secret wraps the node unlock of the default keystore right now.
fn current_kek_source() -> &'static str {
    if env_record_key().is_some() {
        "env"
    } else {
        "master"
    }
}

fn default_keyring() -> Result<Arc<KeyRing>> {
    Ok(KeyRing::new(default_spaces_dir(), default_node_kek()?))
}

/// SHA-256 of the keystore file bytes (the file is tiny): a keystore restored
/// with `cp -p` / `rsync -t` to the same size and mtime still invalidates.
type KeyFingerprint = [u8; 32];
type KeyCache = Mutex<std::collections::HashMap<PathBuf, (KeyFingerprint, KeyHandle)>>;

fn key_cache() -> &'static KeyCache {
    static CACHE: OnceLock<KeyCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// Create the default-space keystore (node unlock + recovery code). The caller
/// must show the recovery code exactly once. Refuses to overwrite.
///
/// The KEK source (`env` / `master`) is recorded in the keystore, and a fresh
/// keyring must be able to unlock the new keystore (self-check). When it
/// cannot, the keystore just created is removed and the call fails, so no
/// row is ever sealed under a key the daemon could not open.
pub fn create_default_keystore() -> Result<(KeyHandle, RecoveryCode)> {
    let ring = default_keyring()?;
    let (handle, code) = ring.create_keys(DEFAULT_SPACE_ID, UnlockMode::NodeUnlock, None)?;
    let source = current_kek_source();
    let check = (|| -> Result<()> {
        ring.record_kek_source(DEFAULT_SPACE_ID, source)?;
        let again = default_keyring()?.unlock_with_node(DEFAULT_SPACE_ID)?;
        let probe = again.verify_mac("selfcheck", b"xavier");
        if probe != handle.verify_mac("selfcheck", b"xavier") {
            anyhow::bail!("self-check unlocked a different key");
        }
        Ok(())
    })();
    if let Err(e) = check {
        let _ = std::fs::remove_file(default_keystore_path());
        return Err(e.context(
            "default-space keystore self-check failed (is the node key the same for the CLI and the daemon?); the new keystore was removed",
        ));
    }
    Ok((handle, code))
}

/// Unlock the default-space key with the node wrapper, cached per keystore
/// file (a changed or deleted keystore invalidates the cache). Missing
/// keystore => `KeysError::KeystoreMissing`; any unlock failure is an error.
pub fn default_space_key() -> Result<KeyHandle> {
    let path = default_keystore_path();
    match std::fs::metadata(&path) {
        Ok(_) => {}
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
    let fp: KeyFingerprint = match std::fs::read(&path) {
        Ok(b) => Sha256::digest(&b).into(),
        Err(e) => return Err(KeysError::Corrupt(e.to_string()).into()),
    };
    if let Some((cached_fp, h)) = key_cache()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&path)
    {
        if *cached_fp == fp {
            return Ok(h.clone());
        }
    }
    let ring = default_keyring()?;
    let handle = ring.unlock_with_node(DEFAULT_SPACE_ID).map_err(|e| {
        let recorded = ring.kek_source(DEFAULT_SPACE_ID).ok().flatten();
        match recorded {
            Some(r) if r != current_kek_source() => e.context(format!(
                "keystore was created with KEK source '{r}' but this process resolves '{}'",
                current_kek_source()
            )),
            _ => e,
        }
    })?;
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

// ---- sealed revisions ----

const KIND_REVISIONS: &str = "memory.revisions";
const SEALED_REV_FLAG: &str = "sealed_revisions";
const XRK1_REV_PREFIX: &str = "xrk1:";

/// No revisions to protect (NULL, empty, `[]`, `null`).
fn revisions_empty(json: &str) -> bool {
    let t = json.trim();
    t.is_empty() || t == "[]" || t == "null"
}

/// Column value holding one sentinel revision that carries the sealed JSON.
fn sentinel_column(sealed: &str) -> String {
    serde_json::json!([{
        "revision": 0,
        "recorded_at": "1970-01-01T00:00:00Z",
        "path": "",
        "content": sealed,
        "metadata": { SEALED_REV_FLAG: true },
    }])
    .to_string()
}

/// The sealed payload of a sentinel `revisions` column, `None` for any other value.
fn sealed_payload(col: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(col).ok()?;
    let arr = v.as_array()?;
    if arr.len() != 1 {
        return None;
    }
    let e = &arr[0];
    if e.get("metadata")
        .and_then(|m| m.get(SEALED_REV_FLAG))
        .and_then(|b| b.as_bool())
        != Some(true)
    {
        return None;
    }
    e.get("content")?.as_str().map(str::to_string)
}

/// True when the column carries revision content that is not sealed.
fn revisions_unsealed(col: Option<&str>) -> bool {
    match col {
        None => false,
        Some(c) => !revisions_empty(c) && sealed_payload(c).is_none(),
    }
}

/// Seal the JSON of a revisions array under the default-space key. Always
/// seals (a user-supplied sentinel lookalike is sealed like anything else).
pub fn seal_revisions_xdk2(dek: &KeyHandle, record_id: &str, plain_json: &str) -> Result<String> {
    if revisions_empty(plain_json) {
        return Ok("[]".to_string());
    }
    let sealed = dek.seal_record(DEFAULT_SPACE_ID, KIND_REVISIONS, record_id, plain_json)?;
    Ok(sentinel_column(&sealed))
}

/// Inverse of [`seal_revisions_xdk2`]. A column that is not a sentinel is
/// returned as-is (pre-fix rows; `encrypt-private --apply` seals them).
pub fn open_revisions_xdk2(dek: &KeyHandle, record_id: &str, col: &str) -> Result<String> {
    match sealed_payload(col) {
        Some(p) => dek.open_record(DEFAULT_SPACE_ID, KIND_REVISIONS, record_id, &p),
        None => Ok(col.to_string()),
    }
}

fn seal_revisions_xrk1(dek: &[u8; 32], plain_json: &str) -> Result<String> {
    if revisions_empty(plain_json) {
        return Ok("[]".to_string());
    }
    let nonce = NonceBytes::generate();
    let blob = encrypt_data(plain_json.as_bytes(), dek, &nonce)
        .map_err(|e| anyhow::anyhow!("revisions encryption failed: {e}"))?;
    let mut raw = nonce.as_bytes().to_vec();
    raw.extend_from_slice(&blob.ciphertext);
    Ok(sentinel_column(&format!(
        "{XRK1_REV_PREFIX}{}",
        crate::utils::crypto::hex_encode(&raw)
    )))
}

fn open_revisions_xrk1(dek: &[u8; 32], col: &str) -> Result<String> {
    let Some(p) = sealed_payload(col) else {
        return Ok(col.to_string());
    };
    let body = p
        .strip_prefix(XRK1_REV_PREFIX)
        .ok_or_else(|| anyhow::anyhow!("revisions are sealed under a different envelope"))?;
    let raw = crate::utils::crypto::hex_decode(body)?;
    if raw.len() < 12 {
        anyhow::bail!("revisions ciphertext is corrupt");
    }
    let (nonce, ct) = raw.split_at(12);
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| anyhow::anyhow!("bad nonce"))?;
    let plain = decrypt_data(ct, dek, &nonce)
        .map_err(|e| anyhow::anyhow!("revisions decryption failed: {e}"))?;
    Ok(String::from_utf8(plain)?)
}

/// Plaintext revisions JSON of an `XRK1` row column (for the migration).
fn open_revisions_xrk1_with_wrapped(
    wrapped: &[u8],
    node_key: &[u8; 32],
    col: &str,
) -> Result<String> {
    if sealed_payload(col).is_none() {
        return Ok(col.to_string());
    }
    let dek = unwrap_dek(wrapped, node_key)?;
    open_revisions_xrk1(&dek, col)
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
    // Revisions hold copies of content and metadata: never serve them locked.
    record.revisions.clear();
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
        let revs_col = serde_json::to_string(&record.revisions)?;
        let revs_plain = open_revisions_xdk2(dek, &record.id, &revs_col)?;
        let revisions = serde_json::from_str(&revs_plain)
            .map_err(|_| anyhow::anyhow!("XDK2 revisions column is not valid JSON"))?;
        record.clearance = crate::security::clearance::level_from_metadata(&metadata);
        record.content = content;
        record.metadata = metadata;
        record.revisions = revisions;
        Ok(())
    })();
    if res.is_err() {
        // Never leave ciphertext (or stale columns) where callers read content.
        mark_locked(record);
    }
    res
}

/// Classification-aware write encryption. Private records use the
/// default-space key (`XDK2`) once its keystore exists; public records, and
/// private records while no keystore exists yet, keep the node-record-key
/// envelope (`XRK1`) exactly as before (those rows are migrated by a fresh
/// `encrypt-private --apply` once the keystore is created).
///
/// Fail closed: once the keystore exists, a private write that cannot get the
/// data key returns `Err`. It never falls back to `XRK1` and never stores
/// plaintext; the caller must retry after unlocking.
/// Also sets `record.clearance` to the resolved level (written to the
/// plaintext `clearance_level` column by the store).
pub fn encrypt_for_write(record: &mut crate::memory::store::MemoryRecord) -> Result<bool> {
    let private = is_private_record(&record.metadata, &record.path);
    record.clearance = crate::security::clearance::ClearanceLevel::from(clearance_level_for(
        &record.metadata,
        &record.path,
    ) as u8);
    if private && default_keystore_exists() {
        let dek = default_space_key().map_err(|e| {
            e.context(
                "at_rest: refusing a private write: the default-space key is unavailable (nothing was stored)",
            )
        })?;
        let metadata_json = serde_json::to_string(&record.metadata)?;
        let revisions_json = serde_json::to_string(&record.revisions)?;
        let (c, m) = seal_default_space(&dek, &record.id, &record.content, &metadata_json)?;
        let revs = seal_revisions_xdk2(&dek, &record.id, &revisions_json)?;
        record.content = c;
        record.metadata = serde_json::from_str(&m)?;
        record.revisions = serde_json::from_str(&revs)?;
        record.encrypted_dek = Some(DEFAULT_SPACE_MAGIC.to_vec());
        record.content_iv = None;
        record.metadata_iv = None;
        return Ok(true);
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
            updated_at TEXT,
            anchor_id TEXT,
            initial_count INTEGER,
            initial_max_rowid INTEGER
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
    // Jobs created by an earlier build lack the renumbering anchor columns.
    for (col, ty) in [
        ("anchor_id", "TEXT"),
        ("initial_count", "INTEGER"),
        ("initial_max_rowid", "INTEGER"),
    ] {
        let has: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('memory_encrypt_job') WHERE name = ?1",
                params![col],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)?;
        if !has {
            conn.execute_batch(&format!(
                "ALTER TABLE memory_encrypt_job ADD COLUMN {col} {ty}"
            ))?;
        }
    }
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
    /// Derived plaintext removed for the rows of this batch.
    pub scrub: ScrubStats,
}

/// Counts of derived plaintext removed or re-keyed while migrating rows.
#[derive(Debug, Clone, Default)]
pub struct ScrubStats {
    pub graph_links: u64,
    pub timeline_events: u64,
    pub chain_hashes: u64,
    pub orphan_entities: u64,
    pub graph_snapshots: u64,
    pub revisions_sealed: u64,
}

impl ScrubStats {
    fn add(&mut self, o: &ScrubStats) {
        self.graph_links += o.graph_links;
        self.timeline_events += o.timeline_events;
        self.chain_hashes += o.chain_hashes;
        self.orphan_entities += o.orphan_entities;
        self.graph_snapshots += o.graph_snapshots;
        self.revisions_sealed += o.revisions_sealed;
    }
}

/// Plaintext that remains after a run, printed by the dry run and by apply.
pub const REMAINING_PLAINTEXT: &[&str] = &[
    "embeddings (memory_embeddings*, embedding column): kept in clear, local only (owner-accepted); never sent to cloud backup/sync",
    "path, timestamps, revision counters, embedding status and the clearance_level integer of every row",
    "one graph node per record carrying its path only (no extracted entities for migrated rows)",
    "tables not keyed to a memory id and NOT scrubbed (may hold copies written by other features): belief_states, checkpoint_records, notifications, conversations / working-memory snapshots, bus_events payloads",
    "old backups, rclone/cloud copies and the backup created by this job (plaintext: keep offline, shred after verification)",
];
struct RawRow {
    rowid: i64,
    id: String,
    workspace_id: String,
    path: String,
    content: String,
    metadata: String,
    dek: Option<Vec<u8>>,
    content_iv: Option<Vec<u8>>,
    metadata_iv: Option<Vec<u8>>,
    revisions: Option<String>,
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
        "SELECT rowid, id, workspace_id, path, content, metadata, encrypted_dek, content_iv, metadata_iv, revisions \
         FROM memory_records WHERE rowid > ?1 ORDER BY rowid LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![after, limit as i64], |row| {
        let rowid: i64 = row.get(0)?;
        let parsed = (|| -> rusqlite::Result<RawRow> {
            Ok(RawRow {
                rowid,
                id: row.get(1)?,
                workspace_id: row.get(2)?,
                path: row.get(3)?,
                content: row.get(4)?,
                metadata: row.get(5)?,
                dek: row.get(6)?,
                content_iv: row.get(7)?,
                metadata_iv: row.get(8)?,
                revisions: row.get(9)?,
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
    // anchor_id = id of the last scanned row: on --resume it proves the rowids
    // were not renumbered (VACUUM) since the cursor was written.
    conn.execute(
        "UPDATE memory_encrypt_job SET last_rowid = ?1, done = done + ?2, failed = failed + ?3, updated_at = ?4, \
         anchor_id = (SELECT id FROM memory_records WHERE rowid = ?1) WHERE job_id = ?5",
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

fn row_mac(dek: &KeyHandle, content: &str, metadata_json: &str, revisions_json: &str) -> String {
    let mut data =
        Vec::with_capacity(content.len() + metadata_json.len() + revisions_json.len() + 2);
    data.extend_from_slice(content.as_bytes());
    data.push(0);
    data.extend_from_slice(metadata_json.as_bytes());
    data.push(0);
    data.extend_from_slice(revisions_json.as_bytes());
    dek.verify_mac("row", &data)
}

/// Normalised plaintext revisions JSON (`[]` when there is nothing to protect).
fn norm_revisions(raw: &str) -> String {
    if revisions_empty(raw) {
        "[]".to_string()
    } else {
        raw.to_string()
    }
}

/// Raw-metadata classification: duplicate `clearance` keys are ambiguous
/// (`serde_json` keeps the last, SQLite's `json_extract` may read the first),
/// so such a row is private regardless of the values.
fn metadata_has_duplicate_clearance(raw: &str) -> bool {
    raw.matches("\"clearance\"").count() > 1
}

fn is_private_raw(raw_metadata: &str, path: &str) -> bool {
    if metadata_has_duplicate_clearance(raw_metadata) {
        return true;
    }
    let val: serde_json::Value =
        serde_json::from_str(raw_metadata).unwrap_or(serde_json::Value::Null);
    is_private_record(&val, path)
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE name = ?1 AND type IN ('table','view')",
        params![name],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

/// Remove or re-key the plaintext derived from a row that was just sealed:
/// graph links, timeline details and the unsalted content hashes in
/// `memory_chain` / `timeline_events`. Runs inside the batch transaction.
fn scrub_private_row(
    conn: &Connection,
    dek: &KeyHandle,
    row: &RawRow,
    content: &str,
    revisions_plain: &str,
    stats: &mut ScrubStats,
) -> Result<()> {
    let node_id = format!("mem:{}:{}", row.workspace_id, row.id);
    if table_exists(conn, "memory_entities") {
        stats.graph_links += conn.execute(
            "DELETE FROM memory_entities WHERE memory_id = ?1",
            params![row.id],
        )? as u64;
    }
    if table_exists(conn, "relations") {
        stats.graph_links += conn.execute(
            "DELETE FROM relations WHERE source_id = ?1 OR provenance_id = ?2",
            params![node_id, row.id],
        )? as u64;
    }
    if table_exists(conn, "memory_chain") {
        let mut plains: Vec<String> = vec![content.to_string()];
        if let Ok(serde_json::Value::Array(revs)) = serde_json::from_str(revisions_plain) {
            for r in revs {
                if let Some(c) = r.get("content").and_then(|c| c.as_str()) {
                    plains.push(c.to_string());
                }
            }
        }
        for plain in plains {
            let old = crate::crypto::hex_encode(Sha256::digest(plain.as_bytes()));
            let new = dek.verify_mac("chain", plain.as_bytes());
            stats.chain_hashes += conn.execute(
                "UPDATE memory_chain SET content_hash = ?1 WHERE content_hash = ?2",
                params![new, old],
            )? as u64;
            conn.execute(
                "UPDATE memory_chain SET prev_hash = ?1 WHERE prev_hash = ?2",
                params![new, old],
            )?;
        }
    }
    if table_exists(conn, "timeline_events") {
        let events: Vec<(String, String)> = {
            let mut stmt =
                conn.prepare("SELECT id, curr_hash FROM timeline_events WHERE memory_id = ?1")?;
            let rows = stmt.query_map(params![row.id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        for (eid, old_hash) in events {
            // The hash covered sha256(content): re-key it, drop the metadata copy.
            let new = dek.verify_mac("timeline", old_hash.as_bytes());
            conn.execute(
                "UPDATE timeline_events SET details = ?1, curr_hash = ?2 WHERE id = ?3",
                params![r#"{"scrubbed":"encrypt-private"}"#, new, eid],
            )?;
            stats.timeline_events += 1;
        }
    }
    Ok(())
}

fn migrate_one(
    conn: &Connection,
    dek: &KeyHandle,
    node_key: Option<&[u8; 32]>,
    job_id: i64,
    row: &RawRow,
    stats: &mut ScrubStats,
) -> Result<RowOutcome> {
    let wrapped = row.dek.as_deref().unwrap_or(&[]);
    let is_xrk1 = !wrapped.is_empty() && is_wrapped_v2(wrapped);
    if is_default_space_row(wrapped) {
        // Row sealed by an earlier build: content and metadata are sealed but
        // `revisions` may still hold plaintext copies. Seal it and scrub.
        if revisions_unsealed(row.revisions.as_deref()) {
            let Ok((content, _)) = open_default_space(dek, &row.id, &row.content, &row.metadata)
            else {
                return Ok(RowOutcome::Failed(
                    "XDK2 row did not decrypt with the default-space key".into(),
                ));
            };
            let plain = norm_revisions(row.revisions.as_deref().unwrap_or("[]"));
            let sealed = seal_revisions_xdk2(dek, &row.id, &plain)?;
            conn.execute(
                "UPDATE memory_records SET revisions = ?1 WHERE rowid = ?2",
                params![sealed, row.rowid],
            )?;
            scrub_private_row(conn, dek, row, &content, &plain, stats)?;
            stats.revisions_sealed += 1;
            return Ok(RowOutcome::Changed);
        }
        return Ok(RowOutcome::Done);
    }
    if !wrapped.is_empty() && !is_xrk1 {
        return Ok(RowOutcome::Done);
    }
    let (content, metadata_json, revisions_plain) = if is_xrk1 {
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
        let Ok(revs) =
            open_revisions_xrk1_with_wrapped(wrapped, nk, row.revisions.as_deref().unwrap_or("[]"))
        else {
            return Ok(RowOutcome::Failed(
                "XRK1 revisions did not decrypt with the node record key".into(),
            ));
        };
        (
            rec.content,
            serde_json::to_string(&rec.metadata)?,
            norm_revisions(&revs),
        )
    } else {
        (
            row.content.clone(),
            row.metadata.clone(),
            norm_revisions(row.revisions.as_deref().unwrap_or("[]")),
        )
    };
    // Garbled metadata parses to Null: classified private (fail closed).
    let meta_val: serde_json::Value =
        serde_json::from_str(&metadata_json).unwrap_or(serde_json::Value::Null);
    let private = is_private_raw(&metadata_json, &row.path);
    let level = if private {
        clearance_level_for(&meta_val, &row.path).max(1)
    } else {
        0
    };
    if !private {
        conn.execute(
            "UPDATE memory_records SET clearance_level = ?1 WHERE rowid = ?2",
            params![level, row.rowid],
        )?;
        return Ok(RowOutcome::Public);
    }
    conn.execute(
        "INSERT OR REPLACE INTO memory_encrypt_plain_hash(job_id, id, mac) VALUES (?1, ?2, ?3)",
        params![
            job_id,
            row.id,
            row_mac(dek, &content, &metadata_json, &revisions_plain)
        ],
    )?;
    let (c, m) = seal_default_space(dek, &row.id, &content, &metadata_json)?;
    let sealed_revs = seal_revisions_xdk2(dek, &row.id, &revisions_plain)?;
    conn.execute(
        "UPDATE memory_records SET content = ?1, metadata = ?2, encrypted_dek = ?3, content_iv = NULL, metadata_iv = NULL, clearance_level = ?4, revisions = ?5 WHERE rowid = ?6",
        params![c, m, DEFAULT_SPACE_MAGIC.to_vec(), level, sealed_revs, row.rowid],
    )?;
    fts_rewrite(conn, &row.id, &row.path, None)?;
    scrub_private_row(conn, dek, row, &content, &revisions_plain, stats)?;
    Ok(RowOutcome::Changed)
}

/// Encrypt one batch of private rows (`rowid > last_rowid`) in a single
/// `BEGIN IMMEDIATE` transaction that also advances the job cursor. A row
/// error that is not a database error is recorded in
/// `memory_encrypt_failures` and the row is left unchanged. Embeddings are
/// never touched; graph links, timeline details and chain hashes derived from
/// the old plaintext of a migrated row are removed or re-keyed.
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
                    match migrate_one(conn, dek, node_key, job_id, r, &mut out.scrub)? {
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
/// columns (content, metadata and revisions restored verbatim, `encrypted_dek`
/// NULL) and the FTS row gets its full-text form again. Rows that do not
/// decrypt are recorded as failures and left encrypted.
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
            let opened = open_default_space(dek, &r.id, &r.content, &r.metadata).and_then(|cm| {
                open_revisions_xdk2(dek, &r.id, r.revisions.as_deref().unwrap_or("[]"))
                    .map(|rv| (cm, rv))
            });
            match opened {
                Err(_) => {
                    record_failure(
                        conn,
                        job_id,
                        &r.id,
                        "XDK2 row did not decrypt with the default-space key",
                    )?;
                    out.failed += 1;
                }
                Ok(((content, metadata_json), revisions)) => {
                    conn.execute(
                        "UPDATE memory_records SET content = ?1, metadata = ?2, encrypted_dek = NULL, content_iv = NULL, metadata_iv = NULL, revisions = ?3 WHERE rowid = ?4",
                        params![content, metadata_json, revisions, r.rowid],
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
/// metadata, revisions and the three crypto columns are copied byte-for-byte
/// from the backup row of the same id; rows missing from the backup, or
/// encrypted under `XDK2` there, are recorded as failures and left as they
/// are. A row whose `updated_at` or `revision` differs from the backup was
/// edited after the backup: it is refused (failure) unless `force`.
pub fn restore_batch_from_backup(
    conn: &Connection,
    backup: &Connection,
    job_id: i64,
    last_rowid: i64,
    limit: usize,
    force: bool,
) -> Result<BatchOutcome> {
    type BackupCols = (
        String,
        String,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<String>,
    );
    type Stamp = (rusqlite::types::Value, rusqlite::types::Value);
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
                    "SELECT content, metadata, encrypted_dek, content_iv, metadata_iv, revisions FROM memory_records WHERE id = ?1",
                    params![r.id],
                    |x| {
                        Ok((
                            x.get(0)?,
                            x.get(1)?,
                            x.get(2)?,
                            x.get(3)?,
                            x.get(4)?,
                            x.get(5)?,
                        ))
                    },
                )
                .ok();
            let Some((content, metadata, dek, civ, miv, revs)) = orig else {
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
            if !force {
                let stamp = |c: &Connection, sql: &str, key: rusqlite::types::Value| {
                    c.query_row(sql, params![key], |x| Ok((x.get(0)?, x.get(1)?)))
                        .ok()
                };
                let now: Option<Stamp> = stamp(
                    conn,
                    "SELECT updated_at, revision FROM memory_records WHERE rowid = ?1",
                    rusqlite::types::Value::Integer(r.rowid),
                );
                let then: Option<Stamp> = stamp(
                    backup,
                    "SELECT updated_at, revision FROM memory_records WHERE id = ?1",
                    rusqlite::types::Value::Text(r.id.clone()),
                );
                if now.is_none() || now != then {
                    record_failure(
                        conn,
                        job_id,
                        &r.id,
                        "row changed since the backup was taken (use --force to overwrite)",
                    )?;
                    out.failed += 1;
                    continue;
                }
            }
            let plain_for_fts = if dek.as_deref().is_none_or(|d| d.is_empty()) {
                Some(content.as_str())
            } else {
                None
            };
            conn.execute(
                "UPDATE memory_records SET content = ?1, metadata = ?2, encrypted_dek = ?3, content_iv = ?4, metadata_iv = ?5, revisions = ?6 WHERE rowid = ?7",
                params![content, metadata, dek, civ, miv, revs, r.rowid],
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
        "SELECT h.id, h.mac, r.content, r.metadata, r.encrypted_dek, r.revisions FROM memory_encrypt_plain_hash h \
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
        let revisions: Option<String> = row.get(5)?;
        let ok = match (content, metadata, wrapped) {
            (Some(c), Some(m), Some(w)) if is_default_space_row(&w) => {
                let revs_col = revisions.unwrap_or_else(|| "[]".to_string());
                // A sealed column must be a sentinel: plaintext revisions here
                // would be exactly the leak this job exists to remove.
                let sealed_ok = revisions_empty(&revs_col) || sealed_payload(&revs_col).is_some();
                sealed_ok
                    && open_default_space(dek, &id, &c, &m)
                        .and_then(|(pc, pm)| {
                            open_revisions_xdk2(dek, &id, &revs_col)
                                .map(|pr| row_mac(dek, &pc, &pm, &norm_revisions(&pr)) == mac)
                        })
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
    /// `XDK2` rows whose `revisions` still hold plaintext (sealed by `--apply`).
    pub xdk2_plain_revisions: u64,
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
        .prepare("SELECT path, metadata, encrypted_dek, revisions FROM memory_records")
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
            if is_private_raw(&metadata, &path) {
                rep.plaintext_private += 1;
            } else {
                rep.plaintext_public += 1;
            }
        } else if is_default_space_row(&wrapped) {
            rep.already_xdk2 += 1;
            let revs: Option<String> = row.get(3)?;
            if revisions_unsealed(revs.as_deref()) {
                rep.xdk2_plain_revisions += 1;
            }
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
    /// Allow a backup path inside a cloud-synced directory (the backup is plaintext).
    pub allow_synced_backup: bool,
    /// `decrypt-private --from-backup`: overwrite rows edited since the backup.
    pub force: bool,
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
            allow_synced_backup: false,
            force: false,
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
    /// Private rows still not `XDK2` after the final sweep (encrypt jobs).
    pub pending_private: u64,
    /// True when the job ended in phase `incomplete` (pending rows or failures):
    /// the CLI exits non-zero.
    pub incomplete: bool,
    /// Derived plaintext removed or re-keyed.
    pub scrub: ScrubStats,
    /// Loud notices for the operator (backup is plaintext, rowid renumbering, ...).
    pub notices: Vec<String>,
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

/// First other process (Linux `/proc`) that holds the DB, its WAL or its SHM
/// open (fd or memory mapping). Fails closed: `/proc` must be readable. Only
/// processes of the same user can be inspected; processes of other users are
/// not visible (the exclusive-lock probe in [`open_job_conn`] still catches an
/// active writer, and `--online` is the explicit alternative).
fn other_process_with_db_open(db_path: &Path) -> Result<Option<u32>> {
    let me = std::process::id();
    let base = std::fs::canonicalize(db_path)
        .with_context(|| format!("cannot canonicalize {}", db_path.display()))?;
    let mut names = vec![base.clone()];
    let mut shm_name = String::new();
    for suffix in ["-wal", "-shm"] {
        let mut o = base.clone().into_os_string();
        o.push(suffix);
        if suffix == "-shm" {
            shm_name = o.to_string_lossy().into_owned();
        }
        names.push(PathBuf::from(o));
    }
    let procs = std::fs::read_dir("/proc").map_err(|e| {
        anyhow::anyhow!(
            "cannot read /proc ({e}): cannot prove the database is not in use; refusing (use --online to accept that)"
        )
    })?;
    for entry in procs.flatten() {
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
        if let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) {
            for fd in fds.flatten() {
                if let Ok(target) = std::fs::read_link(fd.path()) {
                    if names.contains(&target) {
                        return Ok(Some(pid));
                    }
                }
            }
        }
        // A WAL-mode reader keeps the -shm mapped even after closing the fd.
        if !shm_name.is_empty() {
            if let Ok(maps) = std::fs::read_to_string(entry.path().join("maps")) {
                if maps.contains(&shm_name) {
                    return Ok(Some(pid));
                }
            }
        }
    }
    Ok(None)
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
        if let Some(pid) = other_process_with_db_open(&opts.db_path)? {
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

/// Why `path` (already canonicalised, or its existing parent) must not hold a
/// plaintext backup: it is inside a cloud-synced directory or on a cloud mount.
fn synced_location_reason(canon: &Path) -> Option<String> {
    const MARKERS: &[&str] = &[
        "dropbox",
        "google drive",
        "googledrive",
        "google-drive",
        "gdrive",
        "onedrive",
        "icloud",
        "mobile documents",
        "nextcloud",
        "owncloud",
        "pcloud",
        "megasync",
        "syncthing",
    ];
    for comp in canon.components() {
        let c = comp.as_os_str().to_string_lossy().to_ascii_lowercase();
        if let Some(m) = MARKERS.iter().find(|m| c.contains(**m)) {
            return Some(format!("path component '{c}' matches cloud sync '{m}'"));
        }
    }
    if let Some(home) = dirs::home_dir() {
        let synced = home.join("xavier-backups");
        let synced = std::fs::canonicalize(&synced).unwrap_or(synced);
        if canon.starts_with(&synced) {
            return Some(format!(
                "{} is synced to the cloud by rclone",
                synced.display()
            ));
        }
    }
    if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
        let best = mounts
            .lines()
            .filter_map(|l| {
                let mut f = l.split_whitespace();
                let (src, mp, ty) = (f.next()?, f.next()?, f.next()?);
                let mp = mp.replace("\\040", " ");
                canon.starts_with(&mp).then_some((mp.len(), src, ty))
            })
            .max_by_key(|(len, _, _)| *len);
        if let Some((_, src, ty)) = best {
            let hay = format!("{src} {ty}").to_ascii_lowercase();
            if ty.starts_with("fuse")
                && ["rclone", "drive", "dropbox", "onedrive", "gcsfuse", "s3fs"]
                    .iter()
                    .any(|k| hay.contains(k))
            {
                return Some(format!("it is on a cloud mount ({src}, {ty})"));
            }
        }
    }
    None
}

/// Process-wide umask guard: the backup file is created 0600 from the first byte.
#[cfg(unix)]
struct UmaskGuard(libc::mode_t);

#[cfg(unix)]
#[allow(unsafe_code)] // single libc::umask call, no memory-safety preconditions
impl UmaskGuard {
    fn restrict() -> Self {
        // SAFETY: umask has no memory-safety preconditions.
        Self(unsafe { libc::umask(0o077) })
    }
}

#[cfg(unix)]
#[allow(unsafe_code)] // restores the previous umask
impl Drop for UmaskGuard {
    fn drop(&mut self) {
        // SAFETY: as above.
        unsafe { libc::umask(self.0) };
    }
}

/// Mandatory verified backup: `VACUUM INTO` + SHA-256 manifest + reopen with
/// `quick_check` and row-count comparison. Any failure aborts the job. The
/// backup is a PLAINTEXT copy: it is refused inside cloud-synced directories
/// unless `allow_synced`, is created with mode 0600 from the first byte
/// (umask 077 plus a pre-created empty file) and the caller prints a notice.
fn make_verified_backup(
    conn: &Connection,
    target: &Path,
    hooks: &TestHooks,
    allow_synced: bool,
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
    let parent_dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let canon_parent = std::fs::canonicalize(&parent_dir)
        .with_context(|| format!("cannot canonicalize {}", parent_dir.display()))?;
    if !allow_synced {
        if let Some(why) = synced_location_reason(&canon_parent) {
            anyhow::bail!(
                "refusing to write a PLAINTEXT backup to {}: {why}. Pick a local offline path or pass --allow-synced-backup",
                canon_parent.display()
            );
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let dev = |p: &Path| std::fs::metadata(p).ok().map(|m| m.dev());
        let db_dir = conn
            .path()
            .map(PathBuf::from)
            .and_then(|p| p.parent().map(Path::to_path_buf));
        if let (Some(a), Some(b)) = (db_dir.as_deref().and_then(dev), dev(&parent_dir)) {
            if a == b {
                eprintln!("warning: backup is on the same filesystem as the database; a disk failure would lose both");
            }
        }
    }
    let src_rows = count_rows(conn)?;
    #[cfg(unix)]
    let _umask = UmaskGuard::restrict();
    // Empty file first, mode 0600, so the plaintext copy never exists with wider
    // permissions (VACUUM INTO accepts an existing empty file).
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        o.open(&path)
            .with_context(|| format!("cannot create backup file {}", path.display()))?;
    }
    if let Err(e) = conn.execute("VACUUM INTO ?1", params![path.to_string_lossy()]) {
        let _ = std::fs::remove_file(&path);
        return Err(anyhow::Error::new(e).context("VACUUM INTO backup failed"));
    }
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

/// Operator notice printed whenever a backup was written.
pub fn backup_notice(path: &Path) -> String {
    format!(
        "WARNING: {} is a PLAINTEXT copy of every private record (and its .keystore.json). Keep it offline, never upload or sync it, and shred it (shred -u) once the job is done and verified.",
        path.display()
    )
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
    checkpoint_truncate(conn)?;
    conn.execute_batch("VACUUM;")
        .context("VACUUM failed: freed plaintext pages may remain in the file")?;
    checkpoint_truncate(conn)?;
    Ok(())
}

/// `wal_checkpoint(TRUNCATE)` that fails when the checkpoint was blocked by a
/// reader (busy = 1): plaintext frames could then stay in the WAL.
fn checkpoint_truncate(conn: &Connection) -> Result<()> {
    let (busy, _log, _done): (i64, i64, i64) =
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    if busy != 0 {
        anyhow::bail!(
            "wal_checkpoint(TRUNCATE) was blocked by another connection: plaintext frames may remain in the WAL. Stop the daemon and run again with --resume"
        );
    }
    Ok(())
}

struct OpenJob {
    job_id: i64,
    last_rowid: i64,
    anchor_id: Option<String>,
    backup_path: Option<String>,
    backup_sha256: Option<String>,
}

fn load_open_job(conn: &Connection, kind: &str) -> Result<Option<OpenJob>> {
    Ok(conn
        .query_row(
            "SELECT job_id, last_rowid, anchor_id, backup_path, backup_sha256 FROM memory_encrypt_job \
             WHERE kind = ?1 AND phase != 'done' ORDER BY job_id DESC LIMIT 1",
            params![kind],
            |r| {
                Ok(OpenJob {
                    job_id: r.get(0)?,
                    last_rowid: r.get(1)?,
                    anchor_id: r.get(2)?,
                    backup_path: r.get(3)?,
                    backup_sha256: r.get(4)?,
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

struct StartedJob {
    conn: Connection,
    job_id: i64,
    start: i64,
    backup_path: PathBuf,
    sha: String,
    fresh: bool,
    notices: Vec<String>,
}

/// Shared scaffolding: guard, preflight, backup (fresh) or backup re-check
/// (resume), job row. On `--resume` the cursor is only trusted when the row it
/// points at still has that rowid: `VACUUM` may renumber rowids of a table
/// without an integer primary key, which could put pending rows below the
/// cursor. Otherwise the scan restarts from rowid 0 (processing is idempotent).
fn start_job(opts: &JobOptions, kind: &str) -> Result<StartedJob> {
    let conn = open_job_conn(opts)?;
    ensure_private_encryption_schema(&conn)?;
    let existing = load_open_job(&conn, kind)?;
    let mut notices = Vec::new();
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
        let mut start = job.last_rowid;
        if start > 0 {
            let anchor_rowid: Option<i64> = job.anchor_id.as_deref().and_then(|a| {
                conn.query_row(
                    "SELECT rowid FROM memory_records WHERE id = ?1",
                    params![a],
                    |r| r.get(0),
                )
                .ok()
            });
            let max_now: i64 = conn.query_row(
                "SELECT COALESCE(MAX(rowid), 0) FROM memory_records",
                [],
                |r| r.get(0),
            )?;
            if anchor_rowid != Some(start) || max_now < start {
                let msg = format!(
                    "rowids changed since the cursor was written (VACUUM/renumbering?); rescanning from rowid 0 (job {})",
                    job.job_id
                );
                eprintln!("notice: {msg}");
                notices.push(msg);
                start = 0;
            }
        }
        return Ok(StartedJob {
            conn,
            job_id: job.job_id,
            start,
            backup_path: bp,
            sha,
            fresh: false,
            notices,
        });
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
    let (bp, sha) = make_verified_backup(&conn, target, &opts.hooks, opts.allow_synced_backup)?;
    let notice = backup_notice(&bp);
    eprintln!("{notice}");
    notices.push(notice);
    let total = count_rows(&conn)?;
    let max_rowid: i64 = conn.query_row(
        "SELECT COALESCE(MAX(rowid), 0) FROM memory_records",
        [],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO memory_encrypt_job(kind, phase, last_rowid, total, backup_path, backup_sha256, started_at, updated_at, initial_count, initial_max_rowid) \
         VALUES (?1, 'migrate', 0, ?2, ?3, ?4, ?5, ?5, ?2, ?6)",
        params![kind, total, bp.to_string_lossy(), sha, now_rfc3339(), max_rowid],
    )?;
    let job_id = conn.last_insert_rowid();
    Ok(StartedJob {
        conn,
        job_id,
        start: 0,
        backup_path: bp,
        sha,
        fresh: true,
        notices,
    })
}

fn run_batches(
    conn: &Connection,
    opts: &JobOptions,
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
        rep.scrub.add(&o.scrub);
        cursor = o.last_rowid;
        idx += 1;
        if o.scanned < limit as u64 {
            return Ok(true);
        }
        if opts.rate_limit_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(opts.rate_limit_ms));
        }
    }
}

/// Private rows that are still not sealed under `XDK2`, plus `XDK2` rows whose
/// `revisions` still hold plaintext. Looks at every row from the start (never
/// at the job cursor). `XRK1` rows count unless their plaintext
/// `clearance_level` says they were classified public.
fn count_pending_private(conn: &Connection) -> Result<u64> {
    let mut stmt = conn.prepare(
        "SELECT path, metadata, encrypted_dek, revisions, clearance_level FROM memory_records",
    )?;
    let mut rows = stmt.query([])?;
    let mut pending = 0u64;
    while let Some(row) = rows.next()? {
        let path: Option<String> = row.get(0).ok().flatten();
        let metadata: Option<String> = row.get(1).ok().flatten();
        let wrapped: Vec<u8> = row
            .get::<_, Option<Vec<u8>>>(2)
            .ok()
            .flatten()
            .unwrap_or_default();
        let revisions: Option<String> = row.get(3).ok().flatten();
        let level: Option<i64> = row.get(4).ok().flatten();
        if is_default_space_row(&wrapped) {
            if revisions_unsealed(revisions.as_deref()) {
                pending += 1;
            }
        } else if wrapped.is_empty() {
            // An unreadable (non-text) path/metadata is private by construction.
            if is_private_raw(
                metadata.as_deref().unwrap_or("not json"),
                path.as_deref().unwrap_or(""),
            ) {
                pending += 1;
            }
        } else if is_wrapped_v2(&wrapped) && level != Some(0) {
            pending += 1;
        }
    }
    Ok(pending)
}

/// Global residue after all rows were visited: entities no longer referenced by
/// any link, and cached entity-graph snapshots of workspaces holding private
/// rows (a derived cache that embeds extracted entity names).
fn scrub_global_residue(conn: &Connection, stats: &mut ScrubStats) -> Result<()> {
    if table_exists(conn, "entities")
        && table_exists(conn, "memory_entities")
        && table_exists(conn, "relations")
    {
        stats.orphan_entities += conn.execute(
            "DELETE FROM entities WHERE entity_type IN ('mention','topic','url','date') \
             AND id NOT IN (SELECT entity_id FROM memory_entities) \
             AND id NOT IN (SELECT source_id FROM relations) \
             AND id NOT IN (SELECT target_id FROM relations)",
            [],
        )? as u64;
    }
    if table_exists(conn, "entity_graph_snapshots") {
        stats.graph_snapshots += conn.execute(
            "DELETE FROM entity_graph_snapshots WHERE workspace_id IN \
             (SELECT DISTINCT workspace_id FROM memory_records WHERE substr(encrypted_dek,1,4) = X'58444B32')",
            [],
        )? as u64;
    }
    Ok(())
}

/// `encrypt-private --apply`. `on_recovery_code` receives the recovery code
/// exactly once, as soon as a new keystore was created (before any row is
/// touched).
///
/// Before the job may be marked `done`, a final sweep counts private rows that
/// are still not `XDK2` (rescanning from rowid 0 once when there are any). If
/// rows remain pending or any row failed, the phase becomes `incomplete`,
/// `JobReport::incomplete` is set (the CLI exits non-zero) and `--resume`
/// can be used again.
pub fn apply_encrypt(
    opts: &JobOptions,
    on_recovery_code: &mut dyn FnMut(&str),
) -> Result<JobReport> {
    let StartedJob {
        conn,
        job_id,
        start,
        backup_path,
        sha,
        fresh,
        notices,
    } = start_job(opts, "encrypt")?;
    let mut rep = JobReport {
        job_id,
        backup_path: Some(backup_path.clone()),
        backup_sha256: Some(sha),
        notices,
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&ks, std::fs::Permissions::from_mode(0o600));
        }
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
    let pass = |from: i64, rep: &mut JobReport| -> Result<bool> {
        run_batches(&conn, opts, from, rep, |c, cursor, limit, idx| {
            let abort = abort_in.and_then(|(b, n)| (b == idx).then_some(n));
            migrate_batch(c, &dek, node_key.as_ref(), job_id, cursor, limit, abort)
        })
    };
    let complete = pass(start, &mut rep)?;
    rep.complete = complete;
    if !complete {
        return Ok(rep);
    }

    // Final sweep: never trust the cursor alone.
    let mut pending = count_pending_private(&conn)?;
    if pending > 0 {
        let msg = format!(
            "final sweep found {pending} private row(s) not sealed yet; rescanning from rowid 0"
        );
        eprintln!("notice: {msg}");
        rep.notices.push(msg);
        // Failures are re-recorded by the rescan; stale ones would be misleading.
        conn.execute(
            "DELETE FROM memory_encrypt_failures WHERE job_id = ?1",
            params![job_id],
        )?;
        rep.failed = 0;
        let again = pass(0, &mut rep)?;
        rep.complete = again;
        if !again {
            return Ok(rep);
        }
        pending = count_pending_private(&conn)?;
    }
    let failed_rows: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memory_encrypt_failures WHERE job_id = ?1",
        params![job_id],
        |r| r.get(0),
    )?;
    rep.failed = failed_rows as u64;
    rep.pending_private = pending;

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
    scrub_global_residue(&conn, &mut rep.scrub)?;
    finalize_storage(&conn)?;
    if pending > 0 || rep.failed > 0 {
        set_phase(&conn, job_id, "incomplete")?;
        rep.incomplete = true;
        rep.notices.push(format!(
            "job {job_id} is INCOMPLETE: {pending} private row(s) not sealed, {} failed row(s) (table memory_encrypt_failures). Fix the cause and run with --resume.",
            rep.failed
        ));
        return Ok(rep);
    }
    conn.execute(
        "DELETE FROM memory_encrypt_plain_hash WHERE job_id = ?1",
        params![job_id],
    )?;
    set_phase(&conn, job_id, "done")?;
    Ok(rep)
}

/// `decrypt-private --apply`.
pub fn apply_decrypt(opts: &JobOptions, source: DecryptSource) -> Result<JobReport> {
    let StartedJob {
        conn,
        job_id,
        start,
        backup_path,
        sha,
        notices,
        ..
    } = start_job(opts, "decrypt")?;
    let mut rep = JobReport {
        job_id,
        backup_path: Some(backup_path),
        backup_sha256: Some(sha),
        notices,
        ..Default::default()
    };
    let complete = match source {
        DecryptSource::Key(handle) => {
            let dek = match handle {
                Some(h) => h,
                None => default_space_key()?,
            };
            run_batches(&conn, opts, start, &mut rep, |c, cursor, limit, _| {
                reverse_batch(c, &dek, job_id, cursor, limit)
            })?
        }
        DecryptSource::FromBackup(path) => {
            verify_source_backup_manifest(&path, opts.force)?;
            let b = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .with_context(|| format!("cannot open source backup {}", path.display()))?;
            let qc: String = b.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
            if qc != "ok" {
                anyhow::bail!("source backup failed quick_check: {qc}");
            }
            let force = opts.force;
            run_batches(&conn, opts, start, &mut rep, |c, cursor, limit, _| {
                restore_batch_from_backup(c, &b, job_id, cursor, limit, force)
            })?
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

/// The backup must still match the SHA-256 recorded in its manifest
/// (`<backup>.sha256`) before any row is copied from it. A missing manifest is
/// refused unless `force`.
fn verify_source_backup_manifest(path: &Path, force: bool) -> Result<()> {
    let mut manifest = path.as_os_str().to_owned();
    manifest.push(".sha256");
    let manifest = PathBuf::from(manifest);
    match std::fs::read_to_string(&manifest) {
        Ok(text) => {
            let want = text.split_whitespace().next().unwrap_or("");
            let have = sha256_file(path)?;
            if want != have {
                anyhow::bail!(
                    "source backup {} does not match the SHA-256 in {}; refusing to restore from it",
                    path.display(),
                    manifest.display()
                );
            }
            Ok(())
        }
        Err(_) if force => Ok(()),
        Err(e) => anyhow::bail!(
            "cannot read the backup manifest {} ({e}); refusing to restore from an unverified backup (--force to override)",
            manifest.display()
        ),
    }
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
