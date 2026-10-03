//! Per-space persistence (WP-13d).
//!
//! Layout, one self-contained directory per space with no cross-references:
//!
//! ```text
//! {root}/spaces/{space_id}/space.json      descriptor, no secrets, atomic write
//! {root}/spaces/{space_id}/espacio.sqlite  members, invites, messages
//! ```
//!
//! `espacio.sqlite` is versioned through `PRAGMA user_version`; later packages
//! (e.g. WP-13l `tokens`) append a migration step instead of editing existing
//! tables. Deleting a space is a directory delete.
//!
//! Encryption seam: message content passes through a [`RecordCodec`]. The
//! default is plaintext; WP-13j supplies an AEAD codec per space through a
//! [`KeyRing`] ([`SpaceStores::with_key_ring`]): message content is sealed
//! with the space data key and a locked space refuses every store access.

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::collections::HashMap;
use std::fmt::Debug;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use super::channel::ChannelMessage;
use super::invite::{SpaceInvite, SpaceRole};
use super::keys::{looks_encrypted, KeyRing, KeysError, NodeKek};
use super::manager::SpaceError;
use super::permissions::SpaceMembership;

/// File names inside a space directory.
pub const DESCRIPTOR_FILE: &str = "space.json";
pub const DB_FILE: &str = "espacio.sqlite";

const MAX_ID_LEN: usize = 64;

/// Allowlist check for space ids: 1..=64 chars of `[A-Za-z0-9_-]`. Anything
/// else (dots, slashes, NUL, unicode) is rejected, so an id can never escape
/// the `spaces/` directory.
pub fn validate_space_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > MAX_ID_LEN
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(anyhow!(SpaceError::InvalidId(id.to_string())));
    }
    Ok(())
}

/// Seam for record-level encryption (WP-13j). Applied to message content.
/// `record_id` identifies the row inside its space (message seq) and is bound
/// into the authenticated data, so rows cannot be swapped.
pub trait RecordCodec: Send + Sync + Debug {
    /// Plain text to the stored representation.
    fn seal(&self, plain: &str, record_id: &str) -> Result<String>;
    /// Stored representation back to plain text.
    fn open(&self, stored: &str, record_id: &str) -> Result<String>;
}

/// Default codec: stores records unchanged. It refuses to serve something
/// that is encrypted (ciphertext is never content).
#[derive(Debug, Default)]
pub struct PlaintextCodec;

impl RecordCodec for PlaintextCodec {
    fn seal(&self, plain: &str, _record_id: &str) -> Result<String> {
        Ok(plain.to_string())
    }
    fn open(&self, stored: &str, _record_id: &str) -> Result<String> {
        if looks_encrypted(stored) {
            return Err(anyhow!("record is encrypted; no key available"));
        }
        Ok(stored.to_string())
    }
}

/// Write `bytes` to `path` atomically: temp file in the same directory,
/// fsync, rename, fsync of the directory.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("path {} has no parent", path.display()))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("path {} has no file name", path.display()))?;
    let tmp = dir.join(format!(".{name}.tmp"));
    {
        let mut f =
            std::fs::File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path).with_context(|| format!("rename to {}", path.display()))?;
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

fn ts(t: &DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn parse_ts(s: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(s)
        .with_context(|| format!("bad timestamp {s:?}"))?
        .with_timezone(&Utc))
}

fn role_from(s: &str) -> Result<SpaceRole> {
    match s {
        "admin" => Ok(SpaceRole::Admin),
        "moderator" => Ok(SpaceRole::Moderator),
        "member" => Ok(SpaceRole::Member),
        "reader" => Ok(SpaceRole::Reader),
        other => Err(anyhow!("unknown role {other:?}")),
    }
}

/// Result of a revoke attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeOutcome {
    Revoked,
    AlreadyRevoked,
    NotFound,
}

/// One space's `espacio.sqlite`.
#[derive(Debug)]
pub struct SpaceStore {
    conn: Mutex<Connection>,
    codec: Arc<dyn RecordCodec>,
}

/// Ordered migration steps; index + 1 is the resulting `user_version`.
/// Append only. Step 2 adds `meta` (WP-13j encryption flag); step 3 adds the
/// WP-13l `tokens` table.
const MIGRATIONS: &[&str] = &[
    "
    CREATE TABLE members (
        node_id   TEXT PRIMARY KEY,
        role      TEXT NOT NULL,
        joined_at TEXT NOT NULL
    );
    CREATE TABLE invites (
        id           TEXT PRIMARY KEY,
        space_id     TEXT NOT NULL,
        inviter_node TEXT NOT NULL,
        target_node  TEXT NOT NULL,
        role         TEXT NOT NULL,
        created_at   TEXT NOT NULL,
        expires_at   TEXT NOT NULL,
        signature    TEXT,
        revoked      INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE messages (
        seq        INTEGER PRIMARY KEY,
        space_id   TEXT NOT NULL,
        author     TEXT NOT NULL,
        content    TEXT NOT NULL,
        created_at TEXT NOT NULL
    );
",
    "
    CREATE TABLE meta (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
",
    "
    CREATE TABLE tokens (
        token_id   TEXT PRIMARY KEY,
        token_hash TEXT NOT NULL UNIQUE,
        member_id  TEXT NOT NULL,
        role       TEXT NOT NULL,
        created_at TEXT NOT NULL,
        expires_at TEXT,
        revoked    INTEGER NOT NULL DEFAULT 0
    );
",
];

const META_ENCRYPTION: &str = "encryption";

fn migrate(conn: &Connection) -> Result<()> {
    let current: usize = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current > MIGRATIONS.len() {
        return Err(anyhow!(
            "espacio.sqlite schema version {current} is newer than supported {}",
            MIGRATIONS.len()
        ));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        conn.execute_batch(&format!(
            "BEGIN; {sql} PRAGMA user_version = {}; COMMIT;",
            i + 1
        ))?;
    }
    Ok(())
}

impl SpaceStore {
    fn from_conn(conn: Connection, codec: Arc<dyn RecordCodec>) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            codec,
        })
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    // ---- meta ----

    /// Encryption flag persisted in the database (absent counts as false).
    pub fn encryption_flag(&self) -> Result<bool> {
        let v: Option<String> = self
            .lock()
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![META_ENCRYPTION],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v.as_deref() == Some("1"))
    }

    /// Record the encryption decision. Turning it on is sticky: it can never
    /// be turned off again through this API.
    pub fn mark_encryption(&self, enabled: bool) -> Result<()> {
        let sql = if enabled {
            "INSERT INTO meta (key, value) VALUES (?1, '1')
             ON CONFLICT(key) DO UPDATE SET value = '1'"
        } else {
            "INSERT OR IGNORE INTO meta (key, value) VALUES (?1, '0')"
        };
        self.lock().execute(sql, params![META_ENCRYPTION])?;
        Ok(())
    }

    // ---- members ----

    pub fn add_member(&self, node_id: &str, role: SpaceRole) -> Result<SpaceMembership> {
        let m = SpaceMembership {
            node_id: node_id.to_string(),
            role,
            joined_at: Utc::now(),
        };
        self.lock().execute(
            "INSERT INTO members (node_id, role, joined_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(node_id) DO UPDATE SET role = excluded.role",
            params![m.node_id, m.role.as_str(), ts(&m.joined_at)],
        )?;
        // Return what is stored (joined_at is kept on role change).
        self.member(node_id)?
            .ok_or_else(|| anyhow!("member {node_id} vanished"))
    }

    pub fn member(&self, node_id: &str) -> Result<Option<SpaceMembership>> {
        let row = self
            .lock()
            .query_row(
                "SELECT node_id, role, joined_at FROM members WHERE node_id = ?1",
                params![node_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(n, r, j)| {
            Ok(SpaceMembership {
                node_id: n,
                role: role_from(&r)?,
                joined_at: parse_ts(&j)?,
            })
        })
        .transpose()
    }

    pub fn remove_member(&self, node_id: &str) -> Result<bool> {
        let n = self
            .lock()
            .execute("DELETE FROM members WHERE node_id = ?1", params![node_id])?;
        Ok(n > 0)
    }

    pub fn members(&self) -> Result<Vec<SpaceMembership>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare("SELECT node_id, role, joined_at FROM members ORDER BY joined_at, node_id")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(n, r, j)| {
                Ok(SpaceMembership {
                    node_id: n,
                    role: role_from(&r)?,
                    joined_at: parse_ts(&j)?,
                })
            })
            .collect()
    }

    // ---- invites ----

    pub fn insert_invite(&self, inv: &SpaceInvite) -> Result<()> {
        self.lock().execute(
            "INSERT INTO invites (id, space_id, inviter_node, target_node, role,
                                  created_at, expires_at, signature, revoked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                inv.id,
                inv.space_id,
                inv.inviter_node,
                inv.target_node,
                inv.role.as_str(),
                ts(&inv.created_at),
                ts(&inv.expires_at),
                inv.signature,
                inv.revoked as i64
            ],
        )?;
        Ok(())
    }

    fn invite_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<[String; 8]> {
        Ok([
            r.get(0)?,
            r.get(1)?,
            r.get(2)?,
            r.get(3)?,
            r.get(4)?,
            r.get(5)?,
            r.get(6)?,
            r.get::<_, i64>(8)?.to_string(),
        ])
    }

    fn build_invite(cols: [String; 8], signature: Option<String>) -> Result<SpaceInvite> {
        let [id, space_id, inviter_node, target_node, role, created, expires, revoked] = cols;
        Ok(SpaceInvite {
            id,
            space_id,
            inviter_node,
            target_node,
            role: role_from(&role)?,
            created_at: parse_ts(&created)?,
            expires_at: parse_ts(&expires)?,
            signature,
            revoked: revoked != "0",
        })
    }

    const INVITE_COLS: &'static str =
        "id, space_id, inviter_node, target_node, role, created_at, expires_at, signature, revoked";

    pub fn get_invite(&self, id: &str) -> Result<Option<SpaceInvite>> {
        let row = self
            .lock()
            .query_row(
                &format!("SELECT {} FROM invites WHERE id = ?1", Self::INVITE_COLS),
                params![id],
                |r| Ok((Self::invite_from_row(r)?, r.get::<_, Option<String>>(7)?)),
            )
            .optional()?;
        row.map(|(c, s)| Self::build_invite(c, s)).transpose()
    }

    pub fn list_invites(&self) -> Result<Vec<SpaceInvite>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM invites ORDER BY created_at, id",
            Self::INVITE_COLS
        ))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((Self::invite_from_row(r)?, r.get::<_, Option<String>>(7)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(c, s)| Self::build_invite(c, s))
            .collect()
    }

    pub fn invite_ids(&self) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT id FROM invites")?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ids)
    }

    pub fn revoke_invite(&self, id: &str) -> Result<RevokeOutcome> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: Option<i64> = tx
            .query_row(
                "SELECT revoked FROM invites WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        let out = match state {
            None => RevokeOutcome::NotFound,
            Some(0) => {
                tx.execute("UPDATE invites SET revoked = 1 WHERE id = ?1", params![id])?;
                RevokeOutcome::Revoked
            }
            Some(_) => RevokeOutcome::AlreadyRevoked,
        };
        tx.commit()?;
        Ok(out)
    }

    pub fn set_invite_signature(&self, id: &str, signature: &str) -> Result<bool> {
        let n = self.lock().execute(
            "UPDATE invites SET signature = ?2 WHERE id = ?1",
            params![id, signature],
        )?;
        Ok(n > 0)
    }

    // ---- messages ----

    fn message_from(
        &self,
        seq: i64,
        space: String,
        author: String,
        content: String,
        at: String,
    ) -> Result<ChannelMessage> {
        Ok(ChannelMessage {
            seq: seq as u64,
            space_id: space,
            author,
            content: self.codec.open(&content, &seq.to_string())?,
            created_at: parse_ts(&at)?,
        })
    }

    /// Append with the next sequence (`max + 1`, or 0 for an empty log).
    pub fn append_message(
        &self,
        space_id: &str,
        author: &str,
        content: &str,
    ) -> Result<ChannelMessage> {
        let now = Utc::now();
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let seq: i64 = tx.query_row("SELECT COALESCE(MAX(seq) + 1, 0) FROM messages", [], |r| {
            r.get(0)
        })?;
        let stored = self.codec.seal(content, &seq.to_string())?;
        tx.execute(
            "INSERT INTO messages (seq, space_id, author, content, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![seq, space_id, author, stored, ts(&now)],
        )?;
        tx.commit()?;
        Ok(ChannelMessage {
            seq: seq as u64,
            space_id: space_id.to_string(),
            author: author.to_string(),
            content: content.to_string(),
            created_at: now,
        })
    }

    /// Messages with `seq > since` (all when `None`), ascending.
    pub fn messages_since(&self, since: Option<u64>) -> Result<Vec<ChannelMessage>> {
        let floor: i64 = match since {
            Some(s) => i64::try_from(s).unwrap_or(i64::MAX),
            None => -1,
        };
        let rows = {
            let conn = self.lock();
            let mut stmt = conn.prepare(
                "SELECT seq, space_id, author, content, created_at FROM messages
                 WHERE seq > ?1 ORDER BY seq",
            )?;
            let rows = stmt
                .query_map(params![floor], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })?
                .collect::<rusqlite::Result<Vec<(i64, String, String, String, String)>>>()?;
            rows
        };
        rows.into_iter()
            .map(|(s, sp, a, c, t)| self.message_from(s, sp, a, c, t))
            .collect()
    }

    pub fn message_count(&self) -> Result<usize> {
        let n: i64 = self
            .lock()
            .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Merge remote messages: append only those with `seq` above the running
    /// maximum (0 for an empty log). Rows are stamped with this space's id.
    pub fn merge_messages(&self, space_id: &str, remote: &[ChannelMessage]) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut max_seq: i64 =
            tx.query_row("SELECT COALESCE(MAX(seq), 0) FROM messages", [], |r| {
                r.get(0)
            })?;
        for m in remote {
            let seq = i64::try_from(m.seq).unwrap_or(i64::MAX);
            if seq > max_seq {
                max_seq = seq;
                tx.execute(
                    "INSERT INTO messages (seq, space_id, author, content, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        seq,
                        space_id,
                        m.author,
                        self.codec.seal(&m.content, &seq.to_string())?,
                        ts(&m.created_at)
                    ],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

/// Registry of open per-space stores, shared by the three managers.
#[derive(Debug)]
pub struct SpaceStores {
    /// `{root}/spaces`; `None` means in-memory (tests).
    spaces_dir: Option<PathBuf>,
    codec: Arc<dyn RecordCodec>,
    /// Per-space keys (WP-13j). When set, each space gets its own keyed codec
    /// and locked spaces are refused.
    keys: Option<Arc<KeyRing>>,
    /// True when a caller supplied its own codec (`with_codec`); the
    /// plaintext-only guard below then does not apply.
    custom_codec: bool,
    open: Mutex<HashMap<String, Arc<SpaceStore>>>,
}

impl Default for SpaceStores {
    fn default() -> Self {
        Self {
            spaces_dir: None,
            codec: Arc::new(PlaintextCodec),
            keys: None,
            custom_codec: false,
            open: Mutex::new(HashMap::new()),
        }
    }
}

impl SpaceStores {
    /// In-memory stores (nothing touches disk).
    pub fn in_memory() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Persistent stores under `{root}/spaces/{space_id}/espacio.sqlite`.
    pub fn open(root: impl AsRef<Path>) -> Arc<Self> {
        Arc::new(Self {
            spaces_dir: Some(root.as_ref().join("spaces")),
            ..Self::default()
        })
    }

    /// Persistent stores with a custom record codec (WP-13j seam).
    pub fn with_codec(root: impl AsRef<Path>, codec: Arc<dyn RecordCodec>) -> Arc<Self> {
        Arc::new(Self {
            spaces_dir: Some(root.as_ref().join("spaces")),
            codec,
            keys: None,
            custom_codec: true,
            open: Mutex::new(HashMap::new()),
        })
    }

    /// Persistent stores with per-space envelope keys (WP-13j). `node`
    /// supplies the node-bound KEK (injected, so tests need no master key).
    pub fn with_key_ring(root: impl AsRef<Path>, node: Arc<dyn NodeKek>) -> Arc<Self> {
        let spaces_dir = root.as_ref().join("spaces");
        Arc::new(Self {
            keys: Some(KeyRing::new(spaces_dir.clone(), node)),
            spaces_dir: Some(spaces_dir),
            ..Self::default()
        })
    }

    /// The key ring, when per-space keys are enabled.
    pub fn key_ring(&self) -> Option<Arc<KeyRing>> {
        self.keys.clone()
    }

    pub fn is_persistent(&self) -> bool {
        self.spaces_dir.is_some()
    }

    pub fn spaces_dir(&self) -> Option<&Path> {
        self.spaces_dir.as_deref()
    }

    fn cache(&self) -> MutexGuard<'_, HashMap<String, Arc<SpaceStore>>> {
        self.open.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn open_new(&self, space_id: &str) -> Result<Arc<SpaceStore>> {
        let conn = match &self.spaces_dir {
            Some(dir) => {
                let sdir = dir.join(space_id);
                std::fs::create_dir_all(&sdir)
                    .with_context(|| format!("create {}", sdir.display()))?;
                Connection::open(sdir.join(DB_FILE))?
            }
            None => Connection::open_in_memory()?,
        };
        let codec = match &self.keys {
            Some(ring) => ring.codec(space_id, "message"),
            None => self.codec.clone(),
        };
        Ok(Arc::new(SpaceStore::from_conn(conn, codec)?))
    }

    /// Store for a space, creating directory and database when missing.
    pub fn get(&self, space_id: &str) -> Result<Arc<SpaceStore>> {
        self.get_inner(space_id, true)
    }

    /// Shared open path. The cache mutex is held across the open and migrate
    /// step, so two callers can never open the same database twice.
    fn get_inner(&self, space_id: &str, create: bool) -> Result<Arc<SpaceStore>> {
        validate_space_id(space_id)?;
        if let Some(ring) = &self.keys {
            // Locked: no store access at all, never a plaintext fallback.
            ring.check_unlocked(space_id)?;
        }
        let mut cache = self.cache();
        if let Some(s) = cache.get(space_id) {
            return Ok(s.clone());
        }
        if !create {
            let exists = self
                .spaces_dir
                .as_ref()
                .is_some_and(|d| d.join(space_id).join(DB_FILE).is_file());
            if !exists {
                return Err(anyhow!(SpaceError::NotFound(space_id.to_string())));
            }
        }
        // Fail closed: anything that says "encrypted" wins over a missing
        // keystore, and a plaintext-only registry refuses such a space.
        if self.encryption_evidence(space_id)? {
            match &self.keys {
                Some(ring) => {
                    ring.require_encryption(space_id);
                    ring.check_unlocked(space_id)?;
                }
                None if !self.custom_codec => {
                    return Err(KeysError::KeystoreMissing(space_id.to_string()).into());
                }
                None => {}
            }
        }
        let store = self.open_new(space_id)?;
        cache.insert(space_id.to_string(), store.clone());
        Ok(store)
    }

    /// At boot: for a space without a keystore file, look for encryption
    /// evidence now so that it reports as locked before any access.
    pub(crate) fn prime_encryption(&self, space_id: &str) {
        let (Some(ring), Some(dir)) = (&self.keys, &self.spaces_dir) else {
            return;
        };
        if validate_space_id(space_id).is_err()
            || dir.join(space_id).join(super::keys::KEYSTORE_FILE).exists()
        {
            return;
        }
        match self.encryption_evidence(space_id) {
            Ok(false) => {}
            // Unreadable evidence is treated like positive evidence.
            Ok(true) | Err(_) => ring.require_encryption(space_id),
        }
    }

    /// Whether `space.json` (`encryption.enabled`), the database `meta` row
    /// or any stored `xr1:` row says the space is encrypted. Unreadable
    /// evidence is an error (fail closed), absent evidence is `false`.
    fn encryption_evidence(&self, space_id: &str) -> Result<bool> {
        let Some(dir) = &self.spaces_dir else {
            return Ok(false);
        };
        let sdir = dir.join(space_id);
        match std::fs::read(sdir.join(DESCRIPTOR_FILE)) {
            Ok(b) => {
                let v: serde_json::Value = serde_json::from_slice(&b)
                    .map_err(|e| anyhow!("{DESCRIPTOR_FILE} of {space_id} unreadable: {e}"))?;
                if v["encryption"]["enabled"].as_bool() == Some(true) {
                    return Ok(true);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(anyhow!("{DESCRIPTOR_FILE} of {space_id} unreadable: {e}")),
        }
        let db = sdir.join(DB_FILE);
        if !db.is_file() {
            return Ok(false);
        }
        let conn = Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let has_table = |name: &str| -> Result<bool> {
            Ok(conn
                .query_row(
                    "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    params![name],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
        };
        if has_table("meta")? {
            let v: Option<String> = conn
                .query_row(
                    "SELECT value FROM meta WHERE key = ?1",
                    params![META_ENCRYPTION],
                    |r| r.get(0),
                )
                .optional()?;
            if v.as_deref() == Some("1") {
                return Ok(true);
            }
        }
        if has_table("messages")? {
            let mut stmt =
                conn.prepare("SELECT content FROM messages WHERE content LIKE 'xr1:%'")?;
            let mut rows = stmt.query([])?;
            while let Some(r) = rows.next()? {
                if looks_encrypted(&r.get::<_, String>(0)?) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Store for a space only if it already exists (never creates anything).
    pub fn get_existing(&self, space_id: &str) -> Result<Option<Arc<SpaceStore>>> {
        match self.get_inner(space_id, false) {
            Ok(s) => Ok(Some(s)),
            Err(e) => match e.downcast_ref::<SpaceError>() {
                Some(SpaceError::NotFound(_)) => Ok(None),
                _ => Err(e),
            },
        }
    }

    /// Drop the cached handle (before deleting a space directory).
    pub fn evict(&self, space_id: &str) {
        self.cache().remove(space_id);
    }

    /// Space ids that have an `espacio.sqlite` on disk, plus cached ones.
    pub fn known_space_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.cache().keys().cloned().collect();
        if let Some(dir) = &self.spaces_dir {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    if validate_space_id(&name).is_ok()
                        && e.path().join(DB_FILE).is_file()
                        && !ids.contains(&name)
                    {
                        ids.push(name);
                    }
                }
            }
        }
        ids.sort();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_allowlist() {
        for ok in ["esp_a", "A-1", "x", &"a".repeat(64)] {
            assert!(validate_space_id(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "..",
            ".",
            "../x",
            "a/b",
            "a\\b",
            "a b",
            "a\0b",
            "é",
            "a.b",
            &"a".repeat(65),
        ] {
            assert!(validate_space_id(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn stores_reject_traversal_and_create_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let stores = SpaceStores::open(tmp.path());
        assert!(stores.get("../evil").is_err());
        assert!(stores.get_existing("../evil").is_err());
        assert!(!tmp.path().join("evil").exists());
    }

    #[test]
    fn write_atomic_replaces_and_leaves_no_tmp() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("space.json");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        let names: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn schema_is_versioned() {
        let tmp = tempfile::tempdir().unwrap();
        let stores = SpaceStores::open(tmp.path());
        stores.get("s1").unwrap();
        let conn = Connection::open(tmp.path().join("spaces/s1").join(DB_FILE)).unwrap();
        let v: usize = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, MIGRATIONS.len());
    }
}
