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

use std::path::PathBuf;
use std::sync::Once;

use anyhow::{Context, Result};
use rand::RngCore;

use crate::crypto::encryption::{decrypt_data, encrypt_data, NonceBytes};

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
/// untouched. Wrong key returns an error — never silent garbage.
pub fn decrypt_record_in_place(record: &mut crate::memory::store::MemoryRecord) -> Result<()> {
    let wrapped = match &record.encrypted_dek {
        None => return Ok(()),
        Some(b) if b.is_empty() => return Ok(()),
        Some(b) => b.clone(),
    };
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
