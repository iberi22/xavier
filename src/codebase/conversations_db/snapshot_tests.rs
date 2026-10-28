//! Tests for sealed context snapshots: the frame format, the node and
//! default-space tiers, the capture transaction and the load matrix.
//!
//! Tests that resolve a key use [`Sandbox`], which pins the data dir and the
//! record key for the process, so a developer keystore cannot change the tier.
//! Every such test is `#[serial]`.

use super::*;
use crate::espacio::keys::{self, DEFAULT_SPACE_ID};
use crate::memory::sqlite_vec_store::at_rest;
use chrono::Utc;
use rusqlite::{params, Connection, ErrorCode};
use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

const WS: &str = "ws-a";
const SESSION_A: &str = "session-a";
const SESSION_B: &str = "session-b";
const NODE_KEY: [u8; 32] = [0x5a; 32];
const CID: &str = "123e4567-e89b-42d3-a456-426614174000";
const BODY: &[u8] = br#"[{"id":"m-1","role":"user","content":"hi"}]"#;

/// Process env pinned to a fresh data dir, with or without a record key, for
/// one test. The previous values come back on drop.
struct Sandbox {
    dir: tempfile::TempDir,
    saved_data_dir: Option<OsString>,
    saved_record_key: Option<OsString>,
}

impl Sandbox {
    fn new(record_key_hex: Option<String>) -> Self {
        let saved_data_dir = std::env::var_os(at_rest::DATA_DIR_ENV);
        let saved_record_key = std::env::var_os(at_rest::RECORD_KEY_ENV);
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(at_rest::DATA_DIR_ENV, dir.path());
        match record_key_hex {
            Some(hex) => std::env::set_var(at_rest::RECORD_KEY_ENV, hex),
            None => std::env::remove_var(at_rest::RECORD_KEY_ENV),
        }
        Self {
            dir,
            saved_data_dir,
            saved_record_key,
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Creates the default-space keystore, so the default-space tier applies.
    fn create_keystore(&self) {
        at_rest::create_default_keystore().unwrap();
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        at_rest::lock_default_space();
        restore_env(at_rest::DATA_DIR_ENV, self.saved_data_dir.take());
        restore_env(at_rest::RECORD_KEY_ENV, self.saved_record_key.take());
    }
}

fn restore_env(key: &str, value: Option<OsString>) {
    match value {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
}

fn node_key_hex() -> String {
    crate::crypto::hex_encode(NODE_KEY)
}

/// Direct connection to the database file, for planting and inspecting rows.
fn raw(path: &Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn
}

/// Main database file plus its WAL and shared-memory files.
fn raw_bytes(db_path: &Path) -> Vec<u8> {
    let mut all = std::fs::read(db_path).unwrap_or_default();
    for ext in ["-wal", "-shm"] {
        let mut p = db_path.as_os_str().to_owned();
        p.push(ext);
        all.extend(std::fs::read(std::path::PathBuf::from(p)).unwrap_or_default());
    }
    all
}

fn scope(project: &str, session: &str) -> SnapshotScope {
    SnapshotScope::new(project, project, project, session).unwrap()
}

async fn open_node_db(path: &Path, project: &str) -> ConversationsDb {
    ConversationsDb::open_at(project, path)
        .await
        .unwrap()
        .with_node_key(NODE_KEY)
}

async fn open_db(path: &Path, project: &str) -> ConversationsDb {
    ConversationsDb::open_at(project, path).await.unwrap()
}

/// A thread whose id is `id` (the session id), in `project`.
async fn insert_thread(db: &ConversationsDb, id: &str, project: Option<&str>) {
    db.create_schema().await.unwrap();
    let (id, project) = (id.to_string(), project.map(str::to_string));
    ConnectionManager::global()
        .with_conn(&db.full_project_id, move |conn| {
            conn.execute(
                "INSERT INTO conversation_threads (id, project_id, started_at, updated_at)
                 VALUES (?1, ?2, ?3, ?3)",
                params![id, project, Utc::now().to_rfc3339()],
            )?;
            Ok(())
        })
        .await
        .unwrap();
}

async fn store(db: &ConversationsDb, thread: &str, content: &str) -> Message {
    db.store_message(thread, "user", content, None, None, None, None, None)
        .await
        .unwrap()
}

async fn save(db: &ConversationsDb, session: &str) -> SavedSnapshot {
    db.create_checkpoint_with_snapshot(&scope(db.project_id(), session), None)
        .await
        .unwrap()
}

async fn load(
    db: &ConversationsDb,
    session: &str,
    cid: &str,
) -> Result<LoadedSnapshot, SnapshotError> {
    db.load_checkpoint_snapshot(&scope(db.project_id(), session), cid)
        .await
}

fn blob(path: &Path, cid: &str) -> String {
    raw(path)
        .query_row(
            "SELECT context_sealed FROM checkpoint_snapshots WHERE checkpoint_id = ?1",
            params![cid],
            |r| r.get(0),
        )
        .unwrap()
}

fn set_blob(path: &Path, cid: &str, value: &str) {
    raw(path)
        .execute(
            "UPDATE checkpoint_snapshots SET context_sealed = ?2 WHERE checkpoint_id = ?1",
            params![cid, value],
        )
        .unwrap();
}

/// Rows in `table`, or 0 while the table does not exist yet.
fn rows(path: &Path, table: &str) -> i64 {
    let conn = raw(path);
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            params![table],
            |r| r.get(0),
        )
        .unwrap();
    if !exists {
        return 0;
    }
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

/// Inserts an active checkpoint and a snapshot row holding `sealed`, as if a
/// snapshot had been captured for `session` in this database.
async fn plant(db: &ConversationsDb, path: &Path, cid: &str, session: &str, sealed: &str) {
    db.create_schema().await.unwrap();
    db.ensure_snapshot_schema().await.unwrap();
    raw(path)
        .execute(
            "INSERT INTO checkpoints (id, project_id, workspace_id, session_id, state, created_at, kind)
             VALUES (?1, ?2, ?2, ?3, 'active', ?4, 'context_save')",
            params![cid, db.project_id(), session, Utc::now().to_rfc3339()],
        )
        .unwrap();
    raw(path)
        .execute(
            "INSERT INTO checkpoint_snapshots (checkpoint_id, context_sealed) VALUES (?1, ?2)",
            params![cid, sealed],
        )
        .unwrap();
}

/// Runs `hook` once, inside the capture transaction of the pool `pool_id`.
fn install_capture_hook(pool_id: &str, hook: impl FnOnce() + Send + 'static) {
    CAPTURE_HOOKS
        .lock()
        .unwrap()
        .insert(pool_id.to_string(), Box::new(hook));
}

/// Serialized size of a one-message body with empty content.
fn envelope_len() -> usize {
    serde_json::to_vec(&[SnapshotMessage {
        id: Uuid::new_v4().to_string(),
        role: "user".to_string(),
        content: String::new(),
    }])
    .unwrap()
    .len()
}

/// The clear (unsealed) frame for `cid` and `BODY`, as text.
fn clear_frame(cid: &str) -> String {
    let frame = encode_frame(WS, WS, SESSION_A, cid, BODY).unwrap();
    String::from_utf8(frame.to_vec()).unwrap()
}

#[test]
fn snapshot_ad_frame_constants() {
    // Each derivation in the design, checked at compile time and by value.
    assert_eq!(MAX_HEADER, 445);
    assert_eq!(MAX_INNER, 65_981);
    assert_eq!(MAX_RAW, 66_009);
    assert_eq!(MAX_STORED_NODE, 132_023);
    assert_eq!(MAX_STORED_DEK, 132_022);
    assert_eq!(MIN_RAW, 28);
}

#[tokio::test]
#[serial_test::serial]
async fn snapshot_id_rules() {
    // Accepted session ids.
    for session in ["session/a", "a:b", "a.b", "s"] {
        assert!(SnapshotScope::new(WS, WS, WS, session).is_ok(), "{session}");
    }
    // Refused session ids: empty, traversal, too long, illegal character.
    let long = "a".repeat(129);
    for session in ["", "..", "a..b", "a b", long.as_str()] {
        assert!(
            matches!(
                SnapshotScope::new(WS, WS, WS, session),
                Err(SnapshotError::InvalidScope)
            ),
            "{session}"
        );
    }
    // Workspace and project ids: the same charset plus the 128-byte cap.
    let long_ws = "w".repeat(129);
    assert!(matches!(
        SnapshotScope::new(&long_ws, &long_ws, &long_ws, SESSION_A),
        Err(SnapshotError::InvalidScope)
    ));
    assert!(matches!(
        SnapshotScope::new("ws/a", "ws/a", "ws/a", SESSION_A),
        Err(SnapshotError::InvalidScope)
    ));
    // The three ids must agree, after validation.
    assert!(matches!(
        SnapshotScope::new("ws-a", "ws-b", "ws-a", SESSION_A),
        Err(SnapshotError::ScopeMismatch)
    ));
    assert!(matches!(
        SnapshotScope::new("ws-a", "ws-a", "ws-b", SESSION_A),
        Err(SnapshotError::ScopeMismatch)
    ));
    assert!(matches!(
        SnapshotScope::new("ws-a", "ws-b", "ws-a", ".."),
        Err(SnapshotError::InvalidScope)
    ));

    // Checkpoint ids must be canonical lowercase hyphenated UUIDs, checked
    // before any key is resolved: no key is configured here, yet the answer
    // is InvalidScope, not NoSealKey.
    let sb = Sandbox::new(None);
    let db = open_db(&sb.path().join("conv.db"), WS).await;
    let own = scope(WS, SESSION_A);
    let upper = CID.to_ascii_uppercase();
    for cid in [
        upper.as_str(),
        "not-a-uuid",
        "123e4567e89b42d3a456426614174000",
        "",
    ] {
        let err = db.load_checkpoint_snapshot(&own, cid).await.unwrap_err();
        assert!(matches!(err, SnapshotError::InvalidScope), "{cid}: {err:?}");
    }
    // A scope for another project is refused before the key lookup too.
    let other = scope("ws-b", SESSION_A);
    assert!(matches!(
        db.create_checkpoint_with_snapshot(&other, None).await,
        Err(SnapshotError::ScopeMismatch)
    ));
    assert!(matches!(
        db.load_checkpoint_snapshot(&other, CID).await,
        Err(SnapshotError::ScopeMismatch)
    ));
    // A valid id and matching scope reach the key lookup, which fails closed.
    assert!(matches!(
        db.load_checkpoint_snapshot(&own, CID).await,
        Err(SnapshotError::NoSealKey)
    ));
}

#[test]
fn snapshot_frame_parser_corruption() {
    let frame = encode_frame(WS, WS, SESSION_A, CID, BODY).unwrap();
    let body_text = std::str::from_utf8(BODY).unwrap();
    assert_eq!(
        parse_frame(frame.as_slice(), WS, WS, SESSION_A, CID).unwrap(),
        body_text
    );
    let tampered = |f: &[u8]| {
        matches!(
            parse_frame(f, WS, WS, SESSION_A, CID),
            Err(SnapshotError::Tampered)
        )
    };

    // Wrong magic, and a version other than 1.
    let mut f = frame.to_vec();
    f[0] ^= 0xff;
    assert!(tampered(&f));
    let mut f = frame.to_vec();
    f[4] = 2;
    assert!(tampered(&f));
    // Trailing byte after the body.
    let mut f = frame.to_vec();
    f.push(0);
    assert!(tampered(&f));
    // A length prefix larger than what remains.
    let mut f = frame.to_vec();
    f[5..9].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(tampered(&f));
    // Truncated at every position.
    for cut in 0..frame.len() {
        assert!(tampered(&frame[..cut]), "cut {cut}");
    }
    // Every corruption of the header is refused, never accepted.
    let header_len = frame.len() - BODY.len();
    for i in 0..header_len {
        let mut f = frame.to_vec();
        f[i] ^= 0x55;
        assert!(tampered(&f), "header byte {i}");
    }
    // A 129-byte id is refused by the header rule, even when it matches.
    let long = "w".repeat(129);
    let f = encode_frame(&long, &long, SESSION_A, CID, BODY).unwrap();
    assert!(matches!(
        parse_frame(f.as_slice(), &long, &long, SESSION_A, CID),
        Err(SnapshotError::Tampered)
    ));
    // A control character in the session id.
    let f = encode_frame(WS, WS, "a\u{1}b", CID, BODY).unwrap();
    assert!(matches!(
        parse_frame(f.as_slice(), WS, WS, "a\u{1}b", CID),
        Err(SnapshotError::Tampered)
    ));
    // Header ids that differ from the expected scope.
    assert!(matches!(
        parse_frame(frame.as_slice(), WS, WS, SESSION_B, CID),
        Err(SnapshotError::Tampered)
    ));
    // Body that is not UTF-8.
    let f = encode_frame(WS, WS, SESSION_A, CID, &[0xff, 0xfe, 0x00]).unwrap();
    assert!(tampered(f.as_slice()));
    // Body with an unknown JSON field, or with a wrong field type.
    let unknown: &[u8] = br#"[{"id":"m","role":"user","content":"c","extra":1}]"#;
    let f = encode_frame(WS, WS, SESSION_A, CID, unknown).unwrap();
    assert!(tampered(f.as_slice()));
    let wrong_type: &[u8] = br#"[{"id":1,"role":"user","content":"c"}]"#;
    let f = encode_frame(WS, WS, SESSION_A, CID, wrong_type).unwrap();
    assert!(tampered(f.as_slice()));
    // A body over the limit is refused by the encoder.
    let big = vec![b'a'; MAX_BODY + 1];
    assert!(matches!(
        encode_frame(WS, WS, SESSION_A, CID, &big),
        Err(SnapshotError::TooLarge)
    ));
    // Every single-byte corruption returns without panicking.
    for i in 0..frame.len() {
        let mut f = frame.to_vec();
        f[i] ^= 0x55;
        let _ = parse_frame(&f, WS, WS, SESSION_A, CID);
    }
}

#[tokio::test]
#[serial_test::serial]
async fn node_relocated_blob_set() {
    let sb = Sandbox::new(Some(node_key_hex()));
    let path = sb.path().join("conv.db");
    let db = open_node_db(&path, WS).await;
    insert_thread(&db, SESSION_A, Some(WS)).await;
    insert_thread(&db, SESSION_B, Some(WS)).await;
    store(&db, SESSION_A, "node alpha").await;
    store(&db, SESSION_B, "node beta").await;

    // A legacy `xcn1:` value relabelled as a snapshot is refused.
    let a = save(&db, SESSION_A).await;
    assert_eq!(a.tier, SnapshotTier::Node);
    let legacy = crate::crypto::encryption::aes_encrypt(
        b"[]",
        &NODE_KEY,
        &crate::crypto::encryption::NonceBytes::generate(),
    )
    .unwrap();
    let relabelled = format!("xsn1:{}", crate::crypto::hex_encode(legacy));
    set_blob(&path, &a.checkpoint_id, &relabelled);
    assert!(matches!(
        load(&db, SESSION_A, &a.checkpoint_id).await,
        Err(SnapshotError::Tampered)
    ));

    // The same legacy value under its own prefix is clear under a seal policy.
    let a = save(&db, SESSION_A).await;
    let xcn1 = format!(
        "{NODE_SEAL_PREFIX}{}",
        crate::crypto::hex_encode(
            crate::crypto::encryption::aes_encrypt(
                b"[]",
                &NODE_KEY,
                &crate::crypto::encryption::NonceBytes::generate(),
            )
            .unwrap()
        )
    );
    set_blob(&path, &a.checkpoint_id, &xcn1);
    assert!(matches!(
        load(&db, SESSION_A, &a.checkpoint_id).await,
        Err(SnapshotError::ClearUnderSealPolicy)
    ));

    // A blob moved to another checkpoint is refused: the AD binds the id.
    let a = save(&db, SESSION_A).await;
    let b = save(&db, SESSION_B).await;
    let a_blob = blob(&path, &a.checkpoint_id);
    set_blob(&path, &b.checkpoint_id, &a_blob);
    assert!(matches!(
        load(&db, SESSION_B, &b.checkpoint_id).await,
        Err(SnapshotError::Tampered)
    ));

    // Re-pointing a checkpoint at another session, then restoring as that session.
    let c = save(&db, SESSION_A).await;
    raw(&path)
        .execute(
            "UPDATE checkpoints SET session_id = ?2 WHERE id = ?1",
            params![c.checkpoint_id, SESSION_B],
        )
        .unwrap();
    assert!(matches!(
        load(&db, SESSION_B, &c.checkpoint_id).await,
        Err(SnapshotError::Tampered)
    ));

    // Row and checkpoint moved into a `ws-b` database with matching metadata
    // and session B membership.
    let d = save(&db, SESSION_A).await;
    let path_b = sb.path().join("ws-b.db");
    let db_b = open_node_db(&path_b, "ws-b").await;
    insert_thread(&db_b, SESSION_B, Some("ws-b")).await;
    plant(
        &db_b,
        &path_b,
        &d.checkpoint_id,
        SESSION_B,
        &blob(&path, &d.checkpoint_id),
    )
    .await;
    assert!(matches!(
        load(&db_b, SESSION_B, &d.checkpoint_id).await,
        Err(SnapshotError::Tampered)
    ));
}

#[tokio::test]
#[serial_test::serial]
async fn dek_relocated_blob_set() {
    let sb = Sandbox::new(Some(node_key_hex()));
    sb.create_keystore();
    let path = sb.path().join("conv.db");
    let db = open_db(&path, WS).await;
    insert_thread(&db, SESSION_A, Some(WS)).await;
    insert_thread(&db, SESSION_B, Some(WS)).await;
    let msg = store(&db, SESSION_A, "dek alpha").await;
    store(&db, SESSION_B, "dek beta").await;
    let a = save(&db, SESSION_A).await;
    let b = save(&db, SESSION_B).await;
    assert_eq!(a.tier, SnapshotTier::Dek);
    let a_blob = blob(&path, &a.checkpoint_id);
    assert!(a_blob.starts_with("xr1:"));

    // A message column (same key, different kind and record id) in the snapshot slot.
    let content: String = raw(&path)
        .query_row(
            "SELECT content FROM conversation_messages WHERE id = ?1",
            params![msg.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(content.starts_with("xr1:"));
    set_blob(&path, &a.checkpoint_id, &content);
    assert!(matches!(
        load(&db, SESSION_A, &a.checkpoint_id).await,
        Err(SnapshotError::Tampered)
    ));

    // A snapshot blob copied to another checkpoint.
    set_blob(&path, &b.checkpoint_id, &a_blob);
    assert!(matches!(
        load(&db, SESSION_B, &b.checkpoint_id).await,
        Err(SnapshotError::Tampered)
    ));
}

#[tokio::test]
#[serial_test::serial]
async fn raw_sql_clear_row_node() {
    let sb = Sandbox::new(Some(node_key_hex()));
    let path = sb.path().join("conv.db");
    let db = open_node_db(&path, WS).await;
    insert_thread(&db, SESSION_A, Some(WS)).await;
    store(&db, SESSION_A, "node clear").await;
    let saved = save(&db, SESSION_A).await;
    // A well-formed clear frame, plain text, and a legacy prefix: never served.
    let clear = clear_frame(&saved.checkpoint_id);
    for value in [clear.as_str(), "plain text", "xcn1:00"] {
        set_blob(&path, &saved.checkpoint_id, value);
        assert!(
            matches!(
                load(&db, SESSION_A, &saved.checkpoint_id).await,
                Err(SnapshotError::ClearUnderSealPolicy)
            ),
            "{value}"
        );
    }
}

#[tokio::test]
#[serial_test::serial]
async fn raw_sql_clear_row_dek() {
    let sb = Sandbox::new(Some(node_key_hex()));
    sb.create_keystore();
    let path = sb.path().join("conv.db");
    let db = open_db(&path, WS).await;
    insert_thread(&db, SESSION_A, Some(WS)).await;
    store(&db, SESSION_A, "dek clear").await;
    let saved = save(&db, SESSION_A).await;
    let clear = clear_frame(&saved.checkpoint_id);
    for value in [clear.as_str(), "plain text", "xcn1:00"] {
        set_blob(&path, &saved.checkpoint_id, value);
        assert!(
            matches!(
                load(&db, SESSION_A, &saved.checkpoint_id).await,
                Err(SnapshotError::ClearUnderSealPolicy)
            ),
            "{value}"
        );
    }
}

#[tokio::test]
#[serial_test::serial]
async fn snapshot_aad_binding_header_mismatch() {
    let sb = Sandbox::new(Some(node_key_hex()));
    sb.create_keystore();
    let path = sb.path().join("conv.db");
    let node_db = open_node_db(&path, WS).await;

    // Node: header names the destination session, the AD is the source's.
    let cid = Uuid::new_v4().to_string();
    let frame = encode_frame(WS, WS, SESSION_B, &cid, BODY).unwrap();
    let raw_node = keys::seal_with_ad(
        &NODE_KEY,
        &keys::snapshot_node_ad_fields(WS, WS, SESSION_A, &cid),
        frame.as_slice(),
    )
    .unwrap();
    let sealed = format!("xsn1:{}", crate::crypto::hex_encode(raw_node));
    plant(&node_db, &path, &cid, SESSION_B, &sealed).await;
    assert!(matches!(
        load(&node_db, SESSION_B, &cid).await,
        Err(SnapshotError::Tampered)
    ));

    // Node: header and destination differ in workspace and project.
    let cid = Uuid::new_v4().to_string();
    let frame = encode_frame("ws-b", "ws-b", SESSION_B, &cid, BODY).unwrap();
    let raw_node = keys::seal_with_ad(
        &NODE_KEY,
        &keys::snapshot_node_ad_fields(WS, WS, SESSION_B, &cid),
        frame.as_slice(),
    )
    .unwrap();
    let sealed = format!("xsn1:{}", crate::crypto::hex_encode(raw_node));
    let path_b = sb.path().join("ws-b.db");
    let db_b = open_node_db(&path_b, "ws-b").await;
    plant(&db_b, &path_b, &cid, SESSION_B, &sealed).await;
    assert!(matches!(
        load(&db_b, SESSION_B, &cid).await,
        Err(SnapshotError::Tampered)
    ));

    // Dek: real key and correct id, header names session A, stored as B.
    let cid = Uuid::new_v4().to_string();
    let frame = encode_frame(WS, WS, SESSION_A, &cid, BODY).unwrap();
    let dek = at_rest::default_space_key().unwrap();
    let stored = dek
        .seal_record_bytes(DEFAULT_SPACE_ID, "conv.snapshot.v1", &cid, frame.as_slice())
        .unwrap();
    let dek_db = open_db(&path, WS).await;
    plant(&dek_db, &path, &cid, SESSION_B, &stored).await;
    assert!(matches!(
        load(&dek_db, SESSION_B, &cid).await,
        Err(SnapshotError::Tampered)
    ));
}

#[tokio::test]
#[serial_test::serial]
async fn policy_matrix() {
    let sb = Sandbox::new(Some(node_key_hex()));
    let path = sb.path().join("conv.db");

    // Node-sealed snapshot under a Dek policy: WeakerThanPolicy.
    let node_db = open_node_db(&path, WS).await;
    insert_thread(&node_db, SESSION_A, Some(WS)).await;
    store(&node_db, SESSION_A, "policy alpha").await;
    let node_saved = save(&node_db, SESSION_A).await;
    assert_eq!(node_saved.tier, SnapshotTier::Node);
    assert!(blob(&path, &node_saved.checkpoint_id).starts_with("xsn1:"));
    sb.create_keystore();
    let dek_db = open_db(&path, WS).await;
    assert!(matches!(
        load(&dek_db, SESSION_A, &node_saved.checkpoint_id).await,
        Err(SnapshotError::WeakerThanPolicy)
    ));

    // Dek-sealed snapshot under a Node policy: TierMismatch.
    let dek_saved = save(&dek_db, SESSION_A).await;
    assert_eq!(dek_saved.tier, SnapshotTier::Dek);
    assert!(blob(&path, &dek_saved.checkpoint_id).starts_with("xr1:"));
    assert!(matches!(
        load(&node_db, SESSION_A, &dek_saved.checkpoint_id).await,
        Err(SnapshotError::TierMismatch)
    ));

    // No key anywhere: NoSealKey on save and on load, with no schema or row written.
    let sb_none = Sandbox::new(None);
    let bare = sb_none.path().join("bare.db");
    let bare_db = open_db(&bare, WS).await;
    assert!(matches!(
        bare_db
            .create_checkpoint_with_snapshot(&scope(WS, SESSION_A), None)
            .await,
        Err(SnapshotError::NoSealKey)
    ));
    assert!(matches!(
        load(&bare_db, SESSION_A, CID).await,
        Err(SnapshotError::NoSealKey)
    ));
    assert_eq!(
        raw(&bare)
            .query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );

    // Key path that is a directory: KeyUnavailable.
    let sb_dir = Sandbox::new(None);
    std::fs::create_dir_all(at_rest::record_key_path().parent().unwrap()).unwrap();
    std::fs::create_dir(at_rest::record_key_path()).unwrap();
    let dir_db = open_db(&sb_dir.path().join("dir.db"), WS).await;
    assert!(matches!(
        dir_db
            .create_checkpoint_with_snapshot(&scope(WS, SESSION_A), None)
            .await,
        Err(SnapshotError::KeyUnavailable)
    ));
}

#[tokio::test]
#[serial_test::serial]
async fn snapshot_bounds() {
    let sb = Sandbox::new(Some(node_key_hex()));
    let path = sb.path().join("conv.db");
    let db = open_node_db(&path, WS).await;

    // Largest body that fits: 65536 bytes exactly.
    insert_thread(&db, "fits", Some(WS)).await;
    store(&db, "fits", &"a".repeat(MAX_BODY - envelope_len())).await;
    let fits = save(&db, "fits").await;
    let loaded = load(&db, "fits", &fits.checkpoint_id).await.unwrap();
    assert_eq!(loaded.body.len(), MAX_BODY);

    // One byte more is refused, and nothing is written.
    insert_thread(&db, "huge", Some(WS)).await;
    store(&db, "huge", &"a".repeat(MAX_BODY - envelope_len() + 1)).await;
    let before = (
        rows(&path, "checkpoints"),
        rows(&path, "checkpoint_snapshots"),
    );
    assert!(matches!(
        db.create_checkpoint_with_snapshot(&scope(WS, "huge"), None)
            .await,
        Err(SnapshotError::TooLarge)
    ));
    assert_eq!(
        (
            rows(&path, "checkpoints"),
            rows(&path, "checkpoint_snapshots")
        ),
        before
    );

    // Node length bound: 132023 bytes reaches the AEAD and fails it; 132024 is oversized.
    let cid = fits.checkpoint_id.clone();
    set_blob(&path, &cid, &format!("xsn1:{}", "0".repeat(132_023 - 5)));
    assert!(matches!(
        load(&db, "fits", &cid).await,
        Err(SnapshotError::Tampered)
    ));
    set_blob(&path, &cid, &format!("xsn1:{}", "0".repeat(132_024 - 5)));
    SNAPSHOT_VALUE_FETCHES.store(0, Ordering::SeqCst);
    assert!(matches!(
        load(&db, "fits", &cid).await,
        Err(SnapshotError::Oversized)
    ));
    // The oversized value was never fetched, so never decoded or decrypted.
    assert_eq!(SNAPSHOT_VALUE_FETCHES.load(Ordering::SeqCst), 0);

    // A value with an embedded NUL and 200 KB behind it is oversized: the length
    // is in bytes, not up to the first NUL.
    set_blob(
        &path,
        &cid,
        &format!("xsn1:\u{0}{}", "a".repeat(200 * 1024)),
    );
    SNAPSHOT_VALUE_FETCHES.store(0, Ordering::SeqCst);
    assert!(matches!(
        load(&db, "fits", &cid).await,
        Err(SnapshotError::Oversized)
    ));
    // Refused by the SQL bound, so the value never reaches Rust.
    assert_eq!(SNAPSHOT_VALUE_FETCHES.load(Ordering::SeqCst), 0);

    // Uppercase hex of an authentic blob is refused before it is decoded: the
    // bytes would authenticate, so only the case check can refuse it.
    insert_thread(&db, "upper", Some(WS)).await;
    store(&db, "upper", "upper case").await;
    let upper = save(&db, "upper").await;
    let authentic = blob(&path, &upper.checkpoint_id);
    let hex = authentic.strip_prefix("xsn1:").unwrap();
    assert!(hex.bytes().any(|b| b.is_ascii_lowercase()));
    set_blob(
        &path,
        &upper.checkpoint_id,
        &format!("xsn1:{}", hex.to_ascii_uppercase()),
    );
    assert!(matches!(
        load(&db, "upper", &upper.checkpoint_id).await,
        Err(SnapshotError::Tampered)
    ));
}

#[tokio::test]
#[serial_test::serial]
async fn snapshot_bounds_dek() {
    let sb = Sandbox::new(Some(node_key_hex()));
    sb.create_keystore();
    let path = sb.path().join("conv.db");
    let db = open_db(&path, WS).await;
    insert_thread(&db, SESSION_A, Some(WS)).await;
    store(&db, SESSION_A, "dek bounds").await;
    let saved = save(&db, SESSION_A).await;
    // Dek: 132022 bytes reaches the AEAD and fails it; 132023 is oversized.
    set_blob(
        &path,
        &saved.checkpoint_id,
        &format!("xr1:{}", "0".repeat(132_022 - 4)),
    );
    assert!(matches!(
        load(&db, SESSION_A, &saved.checkpoint_id).await,
        Err(SnapshotError::Tampered)
    ));
    set_blob(
        &path,
        &saved.checkpoint_id,
        &format!("xr1:{}", "0".repeat(132_023 - 4)),
    );
    SNAPSHOT_VALUE_FETCHES.store(0, Ordering::SeqCst);
    assert!(matches!(
        load(&db, SESSION_A, &saved.checkpoint_id).await,
        Err(SnapshotError::Oversized)
    ));
    // 132023 bytes is inside the shared SQL bound, so the value is fetched once
    // and refused by length before any decode or decrypt.
    assert_eq!(SNAPSHOT_VALUE_FETCHES.load(Ordering::SeqCst), 1);
}

#[tokio::test]
#[serial_test::serial]
async fn capture_atomic_and_member() {
    let sb = Sandbox::new(Some(node_key_hex()));
    let path = sb.path().join("conv.db");
    let db = open_node_db(&path, WS).await;
    insert_thread(&db, SESSION_A, Some(WS)).await;
    store(&db, SESSION_A, "alpha").await;
    let own = scope(WS, SESSION_A);

    // The capture transaction holds the write lock: a second writer is refused
    // while it is open. Needs a WAL file, so the lock behaviour is real.
    let mode: String = raw(&path)
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    let outcome: Arc<Mutex<Option<ErrorCode>>> = Arc::new(Mutex::new(None));
    let (hook_path, hook_outcome) = (path.clone(), Arc::clone(&outcome));
    install_capture_hook(&db.full_project_id, move || {
        let conn = Connection::open(&hook_path).unwrap();
        conn.busy_timeout(Duration::from_millis(50)).unwrap();
        let result = conn.execute(
            "INSERT INTO deductions (id, deduction, created_at)
             VALUES ('hook', 'hook', '2020-01-01T00:00:00Z')",
            [],
        );
        let code = match result {
            Err(rusqlite::Error::SqliteFailure(e, _)) => Some(e.code),
            _ => None,
        };
        *hook_outcome.lock().unwrap() = code;
    });
    let first = db
        .create_checkpoint_with_snapshot(&own, None)
        .await
        .unwrap();
    assert_eq!(
        *outcome.lock().unwrap(),
        Some(ErrorCode::DatabaseBusy),
        "a second writer must be refused during the capture transaction"
    );

    // Membership: NULL project, another project, no thread, orphan messages and
    // an empty thread write nothing.
    insert_thread(&db, "null-project", None).await;
    raw(&path)
        .execute(
            "INSERT INTO conversation_messages (id, thread_id, role, content, created_at)
             VALUES ('null-msg', 'null-project', 'user', 'x', '2020-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    let db_b = open_node_db(&path, "ws-b").await;
    insert_thread(&db_b, "other-project", Some("ws-b")).await;
    store(&db_b, "other-project", "x").await;
    raw(&path)
        .execute_batch(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO conversation_messages (id, thread_id, role, content, created_at)
             VALUES ('orphan-msg', 'orphan', 'user', 'x', '2020-01-01T00:00:00Z');",
        )
        .unwrap();
    insert_thread(&db, "empty", Some(WS)).await;
    let before = (
        rows(&path, "checkpoints"),
        rows(&path, "checkpoint_snapshots"),
    );
    for session in ["null-project", "other-project", "absent", "orphan"] {
        let err = db
            .create_checkpoint_with_snapshot(&scope(WS, session), None)
            .await
            .unwrap_err();
        assert!(
            matches!(err, SnapshotError::NotMember),
            "{session}: {err:?}"
        );
    }
    let err = db
        .create_checkpoint_with_snapshot(&scope(WS, "empty"), None)
        .await
        .unwrap_err();
    assert!(matches!(err, SnapshotError::Empty), "{err:?}");
    assert_eq!(
        (
            rows(&path, "checkpoints"),
            rows(&path, "checkpoint_snapshots")
        ),
        before
    );

    // A failing snapshot insert rolls back the checkpoint row as well.
    let count = || {
        (
            rows(&path, "checkpoints"),
            rows(&path, "checkpoint_snapshots"),
        )
    };
    let before = count();
    raw(&path)
        .execute_batch(
            "CREATE TRIGGER snap_refuse BEFORE INSERT ON checkpoint_snapshots
             BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();
    let err = db
        .create_checkpoint_with_snapshot(&own, None)
        .await
        .unwrap_err();
    assert!(matches!(err, SnapshotError::Db(_)), "{err:?}");
    assert_eq!(count(), before);
    raw(&path)
        .execute_batch("DROP TRIGGER snap_refuse")
        .unwrap();

    // A checkpoint-id collision fails and never overwrites. Ids are generated,
    // so a trigger stands in for the collision.
    raw(&path)
        .execute_batch(
            "CREATE TRIGGER snap_collide AFTER INSERT ON checkpoints
             BEGIN INSERT INTO checkpoint_snapshots (checkpoint_id, context_sealed)
             VALUES (NEW.id, 'x'); END;",
        )
        .unwrap();
    let before = count();
    let err = db
        .create_checkpoint_with_snapshot(&own, None)
        .await
        .unwrap_err();
    assert!(matches!(err, SnapshotError::Db(_)), "{err:?}");
    assert_eq!(count(), before);
    raw(&path)
        .execute_batch("DROP TRIGGER snap_collide")
        .unwrap();
    // The earlier snapshot is untouched.
    let loaded = load(&db, SESSION_A, &first.checkpoint_id).await.unwrap();
    assert!(loaded.body.contains("alpha"));
}

#[tokio::test]
#[serial_test::serial]
async fn snapshot_disabled_db() {
    let db = ConversationsDb::open_disabled(WS).unwrap();
    let own = scope(WS, SESSION_A);
    assert!(matches!(
        db.create_checkpoint_with_snapshot(&own, None).await,
        Err(SnapshotError::Disabled)
    ));
    assert!(matches!(
        db.load_checkpoint_snapshot(&own, CID).await,
        Err(SnapshotError::Disabled)
    ));
    // No pool was registered for the disabled handle.
    assert!(ConnectionManager::global().pool_class("").is_none());
}

#[tokio::test]
#[serial_test::serial]
async fn snapshot_persists_across_reopen() {
    let sb = Sandbox::new(Some(node_key_hex()));
    let path = sb.path().join("conv.db");
    let db = open_node_db(&path, WS).await;
    insert_thread(&db, SESSION_A, Some(WS)).await;
    let first = store(&db, SESSION_A, "canary-one alpha").await;
    insert_thread(&db, SESSION_B, Some(WS)).await;
    store(&db, SESSION_B, "canary-b beta").await;
    let saved = save(&db, SESSION_A).await;
    assert_eq!(saved.tier, SnapshotTier::Node);

    // Close the handle and its pool, then reopen the same file.
    let pool = ConversationsDb::connection_id_for_path(&path);
    drop(db);
    ConnectionManager::global().disconnect(&pool);
    // Reopening does not carry the explicit key: re-apply it.
    let db = open_node_db(&path, WS).await;
    store(&db, SESSION_A, "canary-two gamma").await;
    assert!(db.delete_thread(SESSION_A).await.unwrap());

    let loaded = load(&db, SESSION_A, &saved.checkpoint_id).await.unwrap();
    let expected = format!(
        r#"[{{"id":"{}","role":"user","content":"canary-one alpha"}}]"#,
        first.id
    );
    assert_eq!(loaded.body, expected);
    assert!(!loaded.body.contains("canary-two"));
    assert_eq!(loaded.tier, SnapshotTier::Node);
    // Another session's scope cannot read this checkpoint.
    assert!(matches!(
        load(&db, SESSION_B, &saved.checkpoint_id).await,
        Err(SnapshotError::Missing)
    ));
    // No plaintext reached the file.
    let file = raw_bytes(&path);
    for marker in ["canary-one", "canary-two", "canary-b", "alpha"] {
        assert!(
            !file.windows(marker.len()).any(|w| w == marker.as_bytes()),
            "{marker} found in the database file"
        );
    }
}

#[tokio::test]
#[serial_test::serial]
async fn snapshot_capture_migrates_legacy_rows_first() {
    let sb = Sandbox::new(Some(node_key_hex()));
    let path = sb.path().join("conv.db");
    let db = open_node_db(&path, WS).await;
    insert_thread(&db, SESSION_A, Some(WS)).await;
    // A clear row written before the seal floor existed. No other operation runs first.
    raw(&path)
        .execute(
            "INSERT INTO conversation_messages (id, thread_id, role, content, created_at)
             VALUES ('legacy-1', ?1, 'user', 'legacy canary', ?2)",
            params![SESSION_A, Utc::now().to_rfc3339()],
        )
        .unwrap();

    // The capture opens node-tier messages, so it migrates the row before reading it.
    let saved = save(&db, SESSION_A).await;
    assert_eq!(saved.tier, SnapshotTier::Node);
    let loaded = load(&db, SESSION_A, &saved.checkpoint_id).await.unwrap();
    assert!(loaded.body.contains("legacy canary"));
    let stored: String = raw(&path)
        .query_row(
            "SELECT content FROM conversation_messages WHERE id = 'legacy-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(stored.starts_with("xcn2:"), "legacy row was not migrated");
}
