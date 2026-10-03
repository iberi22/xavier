//! End-to-end tests for default-workspace private-row encryption.
//!
//! Every test works on a tempdir store with `XAVIER_DATA_DIR` and
//! `XAVIER_RECORD_KEY` pointing at test values; nothing touches a real data
//! directory, keyring or master key. Tests are serialised because they set
//! process-wide environment variables.

use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use xavier::memory::sqlite_vec_store::at_rest::{self, DecryptSource, JobOptions};
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::memory::store::{HybridSearchMode, MemoryRecord, MemoryStore};

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const TEST_KEY_HEX: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";

struct Env {
    _guard: tokio::sync::MutexGuard<'static, ()>,
    tmp: TempDir,
}

impl Env {
    async fn new() -> Self {
        let guard = ENV_LOCK.lock().await;
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("data")).unwrap();
        std::env::set_var("XAVIER_DATA_DIR", tmp.path().join("data"));
        std::env::set_var("XAVIER_RECORD_KEY", TEST_KEY_HEX);
        std::env::remove_var("XAVIER_DEFAULT_CLEARANCE");
        Self { _guard: guard, tmp }
    }
    fn db(&self) -> PathBuf {
        self.tmp.path().join("data").join("vec-store.sqlite3")
    }
    fn backup(&self, name: &str) -> PathBuf {
        self.tmp.path().join(name)
    }
    async fn store(&self) -> VecSqliteMemoryStore {
        VecSqliteMemoryStore::new(VecSqliteStoreConfig {
            path: self.db(),
            embedding_dimensions: 3,
        })
        .await
        .unwrap()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        std::env::remove_var("XAVIER_DATA_DIR");
        std::env::remove_var("XAVIER_RECORD_KEY");
    }
}

type Snap = BTreeMap<
    String,
    (
        String,
        String,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
    ),
>;

fn snapshot(db: &Path) -> Snap {
    let conn = Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare("SELECT id, content, metadata, encrypted_dek, content_iv, metadata_iv FROM memory_records")
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?),
            ))
        })
        .unwrap();
    rows.map(|r| r.unwrap()).collect()
}

/// Logical (decrypted) view of every row: id -> (content, metadata json).
fn logical(db: &Path) -> BTreeMap<String, (String, serde_json::Value)> {
    let dek = at_rest::default_space_key().ok();
    snapshot(db)
        .into_iter()
        .map(|(id, (c, m, dekcol, civ, miv))| {
            let mut rec = MemoryRecord {
                id: id.clone(),
                content: c,
                metadata: serde_json::from_str(&m).unwrap_or_default(),
                encrypted_dek: dekcol,
                content_iv: civ,
                metadata_iv: miv,
                ..Default::default()
            };
            match &dek {
                Some(h)
                    if rec
                        .encrypted_dek
                        .as_deref()
                        .is_some_and(at_rest::is_default_space_row) =>
                {
                    let (c, mj) = at_rest::open_default_space(
                        h,
                        &id,
                        &rec.content,
                        &serde_json::to_string(&rec.metadata).unwrap(),
                    )
                    .unwrap();
                    (id, (c, serde_json::from_str(&mj).unwrap()))
                }
                _ => {
                    at_rest::decrypt_record_in_place(&mut rec).unwrap();
                    (id, (rec.content, rec.metadata))
                }
            }
        })
        .collect()
}

fn file_hash(db: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for suffix in ["", "-wal"] {
        let mut p = db.as_os_str().to_owned();
        p.push(suffix);
        if let Ok(b) = std::fs::read(PathBuf::from(p)) {
            h.update(&b);
        }
    }
    xavier::crypto::hex_encode(h.finalize())
}

fn contains_bytes(db: &Path, needle: &str) -> bool {
    for suffix in ["", "-wal"] {
        let mut p = db.as_os_str().to_owned();
        p.push(suffix);
        if let Ok(b) = std::fs::read(PathBuf::from(p)) {
            if b.windows(needle.len()).any(|w| w == needle.as_bytes()) {
                return true;
            }
        }
    }
    false
}

const LEVELS: [Option<&str>; 7] = [
    Some("INTERNAL"),
    Some("UNCLASSIFIED"),
    Some("CONFIDENTIAL"),
    Some("SECRET"),
    None,
    Some("public"),
    Some("RESTRICTED"),
];

fn is_public_level(l: Option<&str>) -> bool {
    matches!(l, Some("UNCLASSIFIED") | Some("public"))
}

/// Raw plaintext rows (legacy shape: no encrypted_dek) plus their FTS entries.
fn seed_plain(db: &Path, n: usize) {
    let conn = Connection::open(db).unwrap();
    for i in 0..n {
        let level = LEVELS[i % LEVELS.len()];
        let meta = match level {
            Some(l) => format!(r#"{{"clearance":"{l}","n":{i}}}"#),
            None => format!(r#"{{"n":{i}}}"#),
        };
        let content = format!("row number {i} zqcanary{i}x body text");
        let path = format!("notes/seed/{i}.md");
        conn.execute(
            "INSERT INTO memory_records (id, workspace_id, path, content, metadata, created_at, updated_at) VALUES (?1,'ws',?2,?3,?4,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
            params![format!("seed-{i}"), path, content, meta],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO memory_fts(id, path, content, code_tokens) VALUES (?1, ?2, ?3, ?3)",
            params![format!("seed-{i}"), path, content],
        )
        .unwrap();
    }
}

fn opts(env: &Env, backup: &str) -> JobOptions {
    let mut o = JobOptions::new(env.db());
    o.backup_path = Some(env.backup(backup));
    o
}

fn rec(id: &str, path: &str, content: &str, clearance: Option<&str>) -> MemoryRecord {
    let metadata = match clearance {
        Some(c) => serde_json::json!({ "clearance": c, "topic": "t" }),
        None => serde_json::json!({ "topic": "t" }),
    };
    MemoryRecord {
        id: id.to_string(),
        workspace_id: "ws".to_string(),
        path: path.to_string(),
        content: content.to_string(),
        metadata,
        embedding: vec![1.0, 0.0, 0.0],
        ..Default::default()
    }
}

fn apply(o: &JobOptions) -> anyhow::Result<at_rest::JobReport> {
    at_rest::apply_encrypt(o, &mut |_code| {})
}

// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn classification_internal_is_private_explicit_public_is_not() {
    let _env = Env::new().await;
    use serde_json::json;
    assert!(at_rest::is_private_record(
        &json!({"clearance":"INTERNAL"}),
        "a"
    ));
    assert!(
        at_rest::is_private_record(&json!({}), "a"),
        "absent clearance => default INTERNAL => private"
    );
    assert!(
        at_rest::is_private_record(&json!({"clearance":"TPOSECRET"}), "a"),
        "unknown => private"
    );
    assert!(
        at_rest::is_private_record(&json!({"clearance":4}), "a"),
        "non-string => private"
    );
    assert!(
        at_rest::is_private_record(&json!("not an object"), "a"),
        "garbled => private"
    );
    assert!(at_rest::is_private_record(&serde_json::Value::Null, "a"));
    assert!(at_rest::is_private_record(
        &json!({"clearance":"SECRET"}),
        "a"
    ));
    assert!(!at_rest::is_private_record(
        &json!({"clearance":"UNCLASSIFIED"}),
        "a"
    ));
    assert!(!at_rest::is_private_record(
        &json!({"clearance":"public"}),
        "a"
    ));
    assert!(at_rest::clearance_level_for(&json!({"clearance":"INTERNAL"}), "a") >= 1);
    assert_eq!(
        at_rest::clearance_level_for(&json!({"clearance":"UNCLASSIFIED"}), "a"),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dry_run_changes_nothing_and_counts() {
    let env = Env::new().await;
    let store = env.store().await;
    drop(store);
    seed_plain(&env.db(), 70);
    let before = file_hash(&env.db());
    let rep = at_rest::dry_run_report(&env.db()).unwrap();
    assert_eq!(rep.total, 70);
    let want_public = (0..70)
        .filter(|i| is_public_level(LEVELS[i % LEVELS.len()]))
        .count() as u64;
    assert_eq!(rep.plaintext_public, want_public);
    assert_eq!(rep.plaintext_private, 70 - want_public);
    assert_eq!(rep.already_xdk2, 0);
    assert!(!rep.keystore_exists);
    assert_eq!(rep.plaintext_by_clearance.get("<absent>"), Some(&10));
    assert_eq!(
        file_hash(&env.db()),
        before,
        "dry run must not change the database file"
    );
    assert!(
        !at_rest::default_keystore_exists(),
        "dry run must not create a key"
    );
    assert!(!env.tmp.path().join("data/spaces").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn roundtrip_500_mixed_rows_and_canary_absent() {
    let env = Env::new().await;
    drop(env.store().await);
    seed_plain(&env.db(), 500);
    let before = snapshot(&env.db());
    let logical_before = logical(&env.db());

    let mut recovery = Vec::new();
    let rep = at_rest::apply_encrypt(&opts(&env, "bk1.sqlite3"), &mut |c| {
        recovery.push(c.to_string())
    })
    .unwrap();
    assert!(rep.complete && rep.keystore_created);
    assert_eq!(
        recovery.len(),
        1,
        "recovery code is handed out exactly once"
    );
    assert!(recovery[0].starts_with("XRC1-"));
    let public_n = (0..500)
        .filter(|i| is_public_level(LEVELS[i % LEVELS.len()]))
        .count() as u64;
    assert_eq!(rep.changed, 500 - public_n);
    assert_eq!(rep.skipped_public, public_n);
    assert_eq!(rep.verified, rep.changed);
    assert_eq!(rep.failed, 0);

    let after = snapshot(&env.db());
    for (id, (c, m, dek, civ, miv)) in &after {
        let i: usize = id.trim_start_matches("seed-").parse().unwrap();
        let level = LEVELS[i % LEVELS.len()];
        let o = &before[id];
        if is_public_level(level) {
            assert_eq!(
                (c, m, dek, civ, miv),
                (&o.0, &o.1, &o.2, &o.3, &o.4),
                "explicit public row untouched: {id}"
            );
            assert!(!c.starts_with("xr1:"));
        } else {
            assert_eq!(
                dek.as_deref(),
                Some(&b"XDK2"[..]),
                "{level:?} row must be XDK2: {id}"
            );
            assert!(c.starts_with("xr1:"));
            assert!(!c.contains("zqcanary"));
            assert!(!m.contains("clearance") && !m.contains("INTERNAL"));
        }
    }
    // Plaintext canaries of private rows are gone from the raw file; public ones stay.
    assert!(
        !contains_bytes(&env.db(), "zqcanary0x"),
        "row 0 is INTERNAL => must not be in the file"
    );
    assert!(!contains_bytes(&env.db(), "zqcanary2x"), "CONFIDENTIAL");
    assert!(
        contains_bytes(&env.db(), "zqcanary1x"),
        "UNCLASSIFIED row stays plaintext"
    );
    // FTS: private terms gone, public terms still searchable.
    let conn = Connection::open(env.db()).unwrap();
    let hits = |term: &str| -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM memory_fts WHERE memory_fts MATCH ?1",
            params![term],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(hits("zqcanary0x"), 0);
    assert_eq!(hits("zqcanary1x"), 1);
    drop(conn);
    // Logical content is unchanged.
    assert_eq!(logical(&env.db()), logical_before);

    // Round trip back by key restores the exact original columns.
    let mut o = opts(&env, "bk2.sqlite3");
    o.backup_path = Some(env.backup("bk2.sqlite3"));
    let rep = at_rest::apply_decrypt(&o, DecryptSource::Key(None)).unwrap();
    assert!(rep.complete);
    assert_eq!(rep.failed, 0);
    let restored = snapshot(&env.db());
    for (id, (c, m, dek, _, _)) in &restored {
        let o = &before[id];
        assert_eq!(
            (c, m, dek),
            (&o.0, &o.1, &o.2),
            "byte-identical after decrypt: {id}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn internal_rows_are_encrypted_explicit_public_rows_are_not() {
    let env = Env::new().await;
    drop(env.store().await);
    {
        let conn = Connection::open(env.db()).unwrap();
        for (id, meta) in [
            ("r-internal", r#"{"clearance":"INTERNAL"}"#),
            ("r-public", r#"{"clearance":"UNCLASSIFIED"}"#),
            ("r-garbled", "not json at all"),
            ("r-nometa", "{}"),
        ] {
            conn.execute(
                "INSERT INTO memory_records (id, workspace_id, path, content, metadata, created_at, updated_at) VALUES (?1,'ws','p',?2,?3,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
                params![id, format!("content of {id}"), meta],
            )
            .unwrap();
        }
    }
    let rep = apply(&opts(&env, "bk.sqlite3")).unwrap();
    assert_eq!(rep.changed, 3);
    let s = snapshot(&env.db());
    let is_xdk2 = |id: &str| {
        s[id]
            .2
            .as_deref()
            .is_some_and(at_rest::is_default_space_row)
    };
    assert!(is_xdk2("r-internal"), "INTERNAL is private");
    assert!(is_xdk2("r-garbled"), "garbled metadata fails closed");
    assert!(is_xdk2("r-nometa"), "missing clearance fails closed");
    assert!(!is_xdk2("r-public"));
    assert_eq!(s["r-public"].0, "content of r-public");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kill_between_batches_then_resume_matches_uninterrupted_run() {
    let env = Env::new().await;
    drop(env.store().await);
    seed_plain(&env.db(), 130);
    let want = logical(&env.db());

    let mut o = opts(&env, "bk.sqlite3");
    o.batch = 20;
    o.hooks.abort_after_batches = Some(2);
    let err = apply(&o).unwrap_err().to_string();
    assert!(err.contains("injected"), "{err}");
    let mid = snapshot(&env.db());
    let encrypted_mid = mid.values().filter(|v| v.2.is_some()).count();
    assert!(
        encrypted_mid > 0 && encrypted_mid < 130 - 18,
        "partial progress after kill: {encrypted_mid}"
    );

    // Resume without a new backup (the recorded one is re-verified).
    let mut r = JobOptions::new(env.db());
    r.batch = 20;
    r.resume = true;
    let rep = apply(&r).unwrap();
    assert!(rep.complete);
    assert_eq!(
        logical(&env.db()),
        want,
        "final logical state equals an uninterrupted run"
    );
    let again = apply(&opts(&env, "bk-again.sqlite3")).unwrap();
    assert_eq!(again.changed, 0);

    // Kill INSIDE a batch: the transaction rolls back and the cursor stays consistent.
    let env2_db = env.tmp.path().join("data").join("second.sqlite3");
    drop(
        VecSqliteMemoryStore::new(VecSqliteStoreConfig {
            path: env2_db.clone(),
            embedding_dimensions: 3,
        })
        .await
        .unwrap(),
    );
    seed_plain(&env2_db, 50);
    let mut o2 = JobOptions::new(&env2_db);
    o2.backup_path = Some(env.backup("bk2.sqlite3"));
    o2.batch = 20;
    o2.hooks.abort_in_batch = Some((1, 5));
    assert!(apply(&o2).is_err());
    let conn = Connection::open(&env2_db).unwrap();
    let cursor: i64 = conn
        .query_row("SELECT last_rowid FROM memory_encrypt_job", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(cursor, 20, "cursor stays at the last committed batch");
    let enc: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_records WHERE encrypted_dek IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        enc <= 20,
        "rows of the aborted batch rolled back, got {enc}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn corrupted_backup_aborts_before_any_write() {
    let env = Env::new().await;
    drop(env.store().await);
    seed_plain(&env.db(), 40);
    let before = snapshot(&env.db());
    let mut o = opts(&env, "bk.sqlite3");
    o.hooks.corrupt_backup = true;
    let err = apply(&o).unwrap_err().to_string();
    assert!(err.contains("backup verification failed"), "{err}");
    assert_eq!(
        snapshot(&env.db()),
        before,
        "no row may change when the backup does not verify"
    );
    assert!(
        !at_rest::default_keystore_exists(),
        "no key before a verified backup"
    );
    let conn = Connection::open(env.db()).unwrap();
    let jobs: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_encrypt_job", [], |r| r.get(0))
        .unwrap();
    assert_eq!(jobs, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn xdk2_row_with_locked_or_missing_keystore_never_returns_ciphertext() {
    let env = Env::new().await;
    drop(env.store().await);
    seed_plain(&env.db(), 7);
    apply(&opts(&env, "bk.sqlite3")).unwrap();
    let s = snapshot(&env.db());
    let (id, (c, m, dek, civ, miv)) = s.iter().find(|(_, v)| v.2.is_some()).unwrap();
    let make = || MemoryRecord {
        id: id.clone(),
        content: c.clone(),
        metadata: serde_json::from_str(m).unwrap(),
        encrypted_dek: dek.clone(),
        content_iv: civ.clone(),
        metadata_iv: miv.clone(),
        ..Default::default()
    };
    let mut ok = make();
    at_rest::decrypt_record_in_place(&mut ok).unwrap();
    assert!(ok.content.contains("zqcanary"));

    // (a) wrong node key => keystore cannot be unlocked => fail closed.
    std::env::set_var("XAVIER_RECORD_KEY", "00".repeat(32));
    at_rest::lock_default_space(); // fresh process: nothing cached
    let mut locked = make();
    assert!(at_rest::decrypt_record_in_place(&mut locked).is_err());
    assert!(!locked.content.starts_with("xr1:") && !locked.content.contains("zqcanary"));
    assert_eq!(locked.content, at_rest::LOCKED_PLACEHOLDER);
    assert!(!locked.metadata.to_string().contains("xr1:"));
    let mut discarded = make();
    let _ = at_rest::decrypt_with_resolved_key(&mut discarded, None);
    assert_eq!(
        discarded.content,
        at_rest::LOCKED_PLACEHOLDER,
        "`let _ =` callers still get no ciphertext"
    );
    std::env::set_var("XAVIER_RECORD_KEY", TEST_KEY_HEX);
    at_rest::lock_default_space();

    // (b) keystore missing.
    std::fs::remove_dir_all(env.tmp.path().join("data/spaces")).unwrap();
    let mut missing = make();
    let err = at_rest::decrypt_record_in_place(&mut missing).unwrap_err();
    assert!(err.to_string().contains("keystore"), "{err}");
    assert_eq!(missing.content, at_rest::LOCKED_PLACEHOLDER);
    assert!(!missing.content.starts_with("xr1:"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn store_write_path_search_and_legacy_xrk1_migration() {
    let env = Env::new().await;
    let store = env.store().await;
    // Before a keystore exists: private and public writes keep the XRK1 envelope.
    store
        .put(rec(
            "legacy-1",
            "docs/legacy",
            "legacy private zebra content",
            Some("INTERNAL"),
        ))
        .await
        .unwrap();
    let s = snapshot(&env.db());
    assert!(
        s["legacy-1"]
            .2
            .as_deref()
            .is_some_and(at_rest::is_wrapped_v2),
        "XRK1 before the keystore exists"
    );

    let (_h, _code) = at_rest::create_default_keystore().unwrap();
    store
        .put(rec(
            "new-priv",
            "docs/priv",
            "fresh private giraffe content",
            Some("INTERNAL"),
        ))
        .await
        .unwrap();
    store
        .put(rec(
            "new-nometa",
            "docs/nometa",
            "no clearance giraffe",
            None,
        ))
        .await
        .unwrap();
    store
        .put(rec(
            "new-pub",
            "docs/pub",
            "explicit public giraffe content",
            Some("UNCLASSIFIED"),
        ))
        .await
        .unwrap();
    let s = snapshot(&env.db());
    assert_eq!(
        s["new-priv"].2.as_deref(),
        Some(&b"XDK2"[..]),
        "new private write is XDK2"
    );
    assert_eq!(s["new-nometa"].2.as_deref(), Some(&b"XDK2"[..]));
    assert!(s["new-priv"].0.starts_with("xr1:") && !s["new-priv"].0.contains("giraffe"));
    assert!(
        s["new-pub"]
            .2
            .as_deref()
            .is_some_and(at_rest::is_wrapped_v2),
        "public writes unchanged (XRK1)"
    );
    let level: i64 = Connection::open(env.db())
        .unwrap()
        .query_row(
            "SELECT clearance_level FROM memory_records WHERE id='new-priv'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(level >= 1);
    let level: i64 = Connection::open(env.db())
        .unwrap()
        .query_row(
            "SELECT clearance_level FROM memory_records WHERE id='new-pub'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(level, 0);

    // Reads, FTS/vector and content queries still work on XDK2 rows.
    let got = store.get("ws", "docs/priv").await.unwrap().expect("row");
    assert_eq!(got.content, "fresh private giraffe content");
    let hits = store
        .hybrid_search("ws", "giraffe", HybridSearchMode::Both, None, 10)
        .await
        .unwrap();
    let ids: Vec<_> = hits.iter().map(|h| h.record.id.clone()).collect();
    assert!(
        ids.contains(&"new-priv".to_string()),
        "private row found through search: {ids:?}"
    );
    assert!(hits
        .iter()
        .find(|h| h.record.id == "new-priv")
        .unwrap()
        .record
        .content
        .contains("giraffe"));

    // Legacy XRK1 row migrates to XDK2 and still reads back.
    drop(store);
    let rep = apply(&opts(&env, "bk.sqlite3")).unwrap();
    assert!(rep.complete);
    assert_eq!(rep.failed, 0);
    let s = snapshot(&env.db());
    assert_eq!(
        s["legacy-1"].2.as_deref(),
        Some(&b"XDK2"[..]),
        "XRK1 row -> XDK2"
    );
    assert!(
        s["new-pub"]
            .2
            .as_deref()
            .is_some_and(at_rest::is_wrapped_v2),
        "public XRK1 left alone"
    );
    let store = env.store().await;
    let got = store.get("ws", "docs/legacy").await.unwrap().expect("row");
    assert_eq!(got.content, "legacy private zebra content");
    let hits = store
        .hybrid_search("ws", "zebra", HybridSearchMode::Both, None, 10)
        .await
        .unwrap();
    assert!(hits.iter().any(|h| h.record.id == "legacy-1"));
    assert!(!contains_bytes(&env.db(), "zebra"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn decrypt_from_backup_restores_byte_identical_rows() {
    let env = Env::new().await;
    drop(env.store().await);
    seed_plain(&env.db(), 60);
    let before = snapshot(&env.db());
    let rep = apply(&opts(&env, "plain-backup.sqlite3")).unwrap();
    assert!(rep.changed > 0);
    let backup = rep.backup_path.clone().unwrap();

    let rep = at_rest::apply_decrypt(
        &opts(&env, "pre-restore.sqlite3"),
        DecryptSource::FromBackup(backup),
    )
    .unwrap();
    assert!(rep.complete);
    assert_eq!(rep.failed, 0);
    assert_eq!(
        snapshot(&env.db()),
        before,
        "restore from backup is byte-identical"
    );
    // FTS searchable again for restored rows.
    let conn = Connection::open(env.db()).unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_fts WHERE memory_fts MATCH 'zqcanary0x'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_apply_is_idempotent() {
    let env = Env::new().await;
    drop(env.store().await);
    seed_plain(&env.db(), 45);
    let first = apply(&opts(&env, "bk1.sqlite3")).unwrap();
    assert!(first.changed > 0);
    let snap = snapshot(&env.db());
    let second = apply(&opts(&env, "bk2.sqlite3")).unwrap();
    assert_eq!(second.changed, 0, "second --apply migrates 0 rows");
    assert_eq!(snapshot(&env.db()), snap, "and rewrites nothing");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_guard_refuses_when_database_is_locked() {
    let env = Env::new().await;
    drop(env.store().await);
    seed_plain(&env.db(), 5);
    let holder = Connection::open(env.db()).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE").unwrap();
    let err = apply(&opts(&env, "bk.sqlite3")).unwrap_err();
    assert!(format!("{err:#}").contains("locked"), "{err:#}");
    holder.execute_batch("ROLLBACK").unwrap();
    assert!(apply(&opts(&env, "bk.sqlite3")).is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backup_is_mandatory_and_never_overwrites() {
    let env = Env::new().await;
    drop(env.store().await);
    seed_plain(&env.db(), 5);
    let mut o = JobOptions::new(env.db());
    assert!(apply(&o).unwrap_err().to_string().contains("--backup-path"));
    std::fs::write(env.backup("exists.sqlite3"), b"x").unwrap();
    o.backup_path = Some(env.backup("exists.sqlite3"));
    assert!(apply(&o)
        .unwrap_err()
        .to_string()
        .contains("already exists"));
    assert!(snapshot(&env.db()).values().all(|v| v.2.is_none()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn semantic_dedup_on_xdk2_rows_merges_and_never_merges_a_locked_row() {
    let env = Env::new().await;
    let store = env.store().await;
    store
        .set_dedup_settings(xavier::settings::types::DedupSettings {
            enabled: true,
            threshold: 0.90,
            scope: xavier::settings::types::DedupScope::PathExact,
            max_revisions: 5,
        })
        .await;
    at_rest::create_default_keystore().unwrap();
    let dedup = |mut r: MemoryRecord| {
        r.metadata["dedup"] = serde_json::json!(true);
        r
    };
    store
        .put(dedup(rec("dd-1", "dd/path", "base text", Some("INTERNAL"))))
        .await
        .unwrap();
    store
        .put(dedup(rec(
            "dd-2",
            "dd/path",
            "base text and more",
            Some("INTERNAL"),
        )))
        .await
        .unwrap();
    let s = snapshot(&env.db());
    assert_eq!(
        s.len(),
        1,
        "superset write merged into the existing XDK2 row"
    );
    assert!(s.contains_key("dd-1"));
    assert_eq!(s.values().next().unwrap().2.as_deref(), Some(&b"XDK2"[..]));
    let got = store.get("ws", "dd/path").await.unwrap().unwrap();
    assert_eq!(got.content, "base text and more");

    // Key locked: the existing row is a placeholder and must not be merged over.
    std::env::set_var("XAVIER_RECORD_KEY", "00".repeat(32));
    at_rest::lock_default_space();
    let _ = store
        .put(dedup(rec(
            "dd-3",
            "dd/path",
            "base text and more and even more",
            Some("INTERNAL"),
        )))
        .await;
    std::env::set_var("XAVIER_RECORD_KEY", TEST_KEY_HEX);
    at_rest::lock_default_space();
    let s = snapshot(&env.db());
    let orig = &s["dd-1"];
    assert_eq!(
        orig.2.as_deref(),
        Some(&b"XDK2"[..]),
        "original row still XDK2, not overwritten"
    );
    let dek = at_rest::default_space_key().unwrap();
    let (content, _) = at_rest::open_default_space(&dek, "dd-1", &orig.0, &orig.1).unwrap();
    assert_eq!(content, "base text and more", "original content intact");
}

// ---------------------------------------------------------------------------
// ENCPRIVFIX regression tests (B1..B5 and minors)
// ---------------------------------------------------------------------------

const REV_CANARY_A: &str = "REVCANARY-ALPHA-7731";
const REV_CANARY_B: &str = "REVCANARY-BRAVO-9942";

fn doc_record(id: &str, path: &str, content: &str, clearance: &str) -> MemoryRecord {
    let doc = xavier::memory::qmd_memory::MemoryDocument {
        id: Some(id.to_string()),
        path: path.to_string(),
        content: content.to_string(),
        metadata: serde_json::json!({ "clearance": clearance, "topic": "rev" }),
        embedding: vec![1.0, 0.0, 0.0],
        ..Default::default()
    };
    MemoryRecord::from_document("ws", &doc, true, None)
}

fn raw_revisions(db: &Path, id: &str) -> Option<String> {
    Connection::open(db)
        .unwrap()
        .query_row(
            "SELECT revisions FROM memory_records WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
}

/// B1: `from_document` seeds `revisions` with the plaintext content; an update
/// pushes another copy. Neither may reach the file, for XDK2 and XRK1 rows.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn b1_revisions_are_sealed_for_xdk2_and_xrk1_rows() {
    let env = Env::new().await;
    let store = env.store().await;

    // XRK1 first (no keystore yet).
    store
        .put(doc_record(
            "rv-old",
            "docs/rv-old",
            &format!("first {REV_CANARY_A}"),
            "INTERNAL",
        ))
        .await
        .unwrap();
    store
        .update(doc_record(
            "rv-old",
            "docs/rv-old",
            &format!("second {REV_CANARY_B}"),
            "INTERNAL",
        ))
        .await
        .unwrap();
    assert!(
        !contains_bytes(&env.db(), REV_CANARY_A) && !contains_bytes(&env.db(), REV_CANARY_B),
        "XRK1 row: revisions must not hold plaintext"
    );
    let got = store.get("ws", "docs/rv-old").await.unwrap().unwrap();
    assert_eq!(
        got.revisions.len(),
        2,
        "revisions readable through the store"
    );
    assert!(got.revisions[0].content.contains(REV_CANARY_A));
    assert!(got.content.contains(REV_CANARY_B));

    // XDK2 rows written after the keystore exists.
    at_rest::create_default_keystore().unwrap();
    store
        .put(doc_record(
            "rv-new",
            "docs/rv-new",
            "newrow first PLAINREV-1",
            "INTERNAL",
        ))
        .await
        .unwrap();
    store
        .update(doc_record(
            "rv-new",
            "docs/rv-new",
            "newrow second PLAINREV-2",
            "INTERNAL",
        ))
        .await
        .unwrap();
    assert!(!contains_bytes(&env.db(), "PLAINREV-1"));
    assert!(!contains_bytes(&env.db(), "PLAINREV-2"));
    let got = store.get("ws", "docs/rv-new").await.unwrap().unwrap();
    assert_eq!(got.revisions.len(), 2);
    assert!(got.revisions[0].content.contains("PLAINREV-1"));
    drop(store);

    // The legacy XRK1 row migrates with its revisions to XDK2.
    let rep = apply(&opts(&env, "bk-rev.sqlite3")).unwrap();
    assert!(rep.complete && !rep.incomplete, "{rep:?}");
    assert!(!contains_bytes(&env.db(), REV_CANARY_A));
    let sentinel = raw_revisions(&env.db(), "rv-old").unwrap();
    assert!(sentinel.contains("sealed_revisions") && !sentinel.contains(REV_CANARY_A));
    let store = env.store().await;
    let got = store.get("ws", "docs/rv-old").await.unwrap().unwrap();
    assert_eq!(got.revisions.len(), 2);
    assert!(got.revisions[0].content.contains(REV_CANARY_A));
    drop(store);

    // Locked: no revision content is served.
    let raw = Connection::open(env.db())
        .unwrap()
        .query_row(
            "SELECT content, metadata, encrypted_dek, revisions FROM memory_records WHERE id='rv-new'",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<Vec<u8>>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .unwrap();
    std::env::set_var("XAVIER_RECORD_KEY", "00".repeat(32));
    at_rest::lock_default_space();
    let mut locked = MemoryRecord {
        id: "rv-new".into(),
        content: raw.0,
        metadata: serde_json::from_str(&raw.1).unwrap(),
        encrypted_dek: raw.2,
        revisions: serde_json::from_str(&raw.3.unwrap()).unwrap(),
        ..Default::default()
    };
    assert!(
        !locked.revisions.is_empty(),
        "sentinel present before decrypt"
    );
    assert!(at_rest::decrypt_record_in_place(&mut locked).is_err());
    assert!(
        locked.revisions.is_empty(),
        "locked read returns no revisions"
    );
    assert_eq!(locked.content, at_rest::LOCKED_PLACEHOLDER);
}

/// B1 (pre-fix rows): an XDK2 row whose revisions column still holds plaintext
/// is sealed by `--apply`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn b1_apply_seals_plaintext_revisions_of_existing_xdk2_rows() {
    let env = Env::new().await;
    drop(env.store().await);
    seed_plain(&env.db(), 3);
    apply(&opts(&env, "bk1.sqlite3")).unwrap();
    {
        let conn = Connection::open(env.db()).unwrap();
        conn.execute(
            "UPDATE memory_records SET revisions = ?1 WHERE id = 'seed-0'",
            params![format!(
                r#"[{{"revision":1,"recorded_at":"2026-01-01T00:00:00Z","path":"p","content":"{REV_CANARY_A}","metadata":{{}}}}]"#
            )],
        )
        .unwrap();
    }
    assert!(contains_bytes(&env.db(), REV_CANARY_A));
    let dry = at_rest::dry_run_report(&env.db()).unwrap();
    assert_eq!(dry.xdk2_plain_revisions, 1);
    let rep = apply(&opts(&env, "bk2.sqlite3")).unwrap();
    assert_eq!(rep.changed, 1);
    assert!(!rep.incomplete);
    assert!(!contains_bytes(&env.db(), REV_CANARY_A));
}

/// B2: a row that fails (undecryptable XRK1) must not let the job finish as done.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn b2_failed_row_makes_the_job_incomplete() {
    let env = Env::new().await;
    drop(env.store().await);
    seed_plain(&env.db(), 12);
    {
        let conn = Connection::open(env.db()).unwrap();
        let mut junk = b"XRK1".to_vec();
        junk.extend_from_slice(&[7u8; 60]);
        conn.execute(
            "INSERT INTO memory_records (id, workspace_id, path, content, metadata, encrypted_dek, content_iv, metadata_iv, created_at, updated_at) VALUES ('bad-1','ws','p','deadbeef','{\"encrypted\":\"00\"}',?1,?2,?2,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
            params![junk, vec![1u8; 12]],
        )
        .unwrap();
    }
    let rep = apply(&opts(&env, "bk.sqlite3")).unwrap();
    assert!(rep.incomplete, "a failed row => incomplete");
    assert!(rep.failed >= 1 && rep.pending_private >= 1, "{rep:?}");
    let phase: String = Connection::open(env.db())
        .unwrap()
        .query_row(
            "SELECT phase FROM memory_encrypt_job ORDER BY job_id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(phase, "incomplete");
    // Still resumable, still incomplete while the row is bad.
    let mut r = JobOptions::new(env.db());
    r.resume = true;
    let again = apply(&r).unwrap();
    assert!(again.incomplete);
    // The good rows were sealed anyway.
    let s = snapshot(&env.db());
    assert!(s["seed-0"]
        .2
        .as_deref()
        .is_some_and(at_rest::is_default_space_row));
}

/// B2: rows that end up below the cursor (rowid renumbering, e.g. by VACUUM)
/// are still migrated by the final sweep.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn b2_renumbered_rowids_are_still_migrated() {
    let env = Env::new().await;
    drop(env.store().await);
    seed_plain(&env.db(), 130);
    let mut o = opts(&env, "bk.sqlite3");
    o.batch = 20;
    o.hooks.abort_after_batches = Some(2);
    assert!(apply(&o).is_err());
    {
        // Pending rows now sit BELOW the cursor.
        let conn = Connection::open(env.db()).unwrap();
        // The cursor row (rowid 40) keeps its rowid, so the anchor still matches:
        // only the final sweep can notice.
        conn.execute_batch(
            "UPDATE memory_records SET rowid = rowid + 10000 WHERE rowid < 40;
             UPDATE memory_records SET rowid = rowid - 40 WHERE rowid BETWEEN 41 AND 79;",
        )
        .unwrap();
    }
    let mut r = JobOptions::new(env.db());
    r.batch = 20;
    r.resume = true;
    let rep = apply(&r).unwrap();
    assert!(rep.complete && !rep.incomplete, "{rep:?}");
    let pending = Connection::open(env.db())
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM memory_records WHERE (encrypted_dek IS NULL OR length(encrypted_dek)=0) AND json_extract(metadata,'$.clearance') IS NOT 'UNCLASSIFIED' AND json_extract(metadata,'$.clearance') IS NOT 'public'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(pending, 0, "no private row left in plaintext");
    assert!(!contains_bytes(&env.db(), "zqcanary0x"));
}

/// B3: with the keystore present but locked, a private write errors and
/// nothing (plaintext or XRK1) is written.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn b3_locked_keystore_private_write_fails_closed() {
    let env = Env::new().await;
    let store = env.store().await;
    at_rest::create_default_keystore().unwrap();
    store
        .put(rec(
            "ok-1",
            "docs/ok",
            "stored while unlocked",
            Some("INTERNAL"),
        ))
        .await
        .unwrap();
    let before = snapshot(&env.db());

    std::env::set_var("XAVIER_RECORD_KEY", "00".repeat(32));
    at_rest::lock_default_space();
    let err = store
        .put(rec(
            "w-1",
            "docs/locked-write",
            "LOCKEDWRITE-CANARY-5521",
            Some("INTERNAL"),
        ))
        .await;
    assert!(
        err.is_err(),
        "private write with a locked keystore must fail"
    );
    let err2 = store
        .put(rec(
            "w-2",
            "docs/locked-nometa",
            "LOCKEDWRITE-CANARY-5522",
            None,
        ))
        .await;
    assert!(err2.is_err(), "missing clearance is private too");
    std::env::set_var("XAVIER_RECORD_KEY", TEST_KEY_HEX);
    at_rest::lock_default_space();
    assert_eq!(snapshot(&env.db()), before, "nothing was written");
    assert!(!contains_bytes(&env.db(), "LOCKEDWRITE-CANARY"));
    // Unlocked again: the same write succeeds as XDK2.
    store
        .put(rec(
            "w-1",
            "docs/locked-write",
            "now fine",
            Some("INTERNAL"),
        ))
        .await
        .unwrap();
    assert_eq!(snapshot(&env.db())["w-1"].2.as_deref(), Some(&b"XDK2"[..]));
}

/// B4: the plaintext backup is refused inside cloud-synced directories, is
/// 0600, and the report carries the loud notice.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn b4_backup_refuses_synced_dirs_and_is_private() {
    let env = Env::new().await;
    drop(env.store().await);
    seed_plain(&env.db(), 6);
    let synced = env.tmp.path().join("Dropbox").join("backups");
    std::fs::create_dir_all(&synced).unwrap();
    let mut o = JobOptions::new(env.db());
    o.backup_path = Some(synced.join("bk.sqlite3"));
    let err = format!("{:#}", apply(&o).unwrap_err());
    assert!(
        err.contains("PLAINTEXT backup") && err.contains("allow-synced-backup"),
        "{err}"
    );
    assert!(!synced.join("bk.sqlite3").exists());
    assert!(snapshot(&env.db()).values().all(|v| v.2.is_none()));
    for name in [
        "Google Drive",
        "OneDrive",
        "iCloudDrive",
        "Nextcloud",
        "GoogleDrive",
        "gdrive",
    ] {
        let d = env.tmp.path().join(name);
        std::fs::create_dir_all(&d).unwrap();
        o.backup_path = Some(d.join("x.sqlite3"));
        assert!(apply(&o).is_err(), "{name} must be refused");
    }
    o.allow_synced_backup = true;
    o.backup_path = Some(synced.join("bk.sqlite3"));
    let rep = apply(&o).unwrap();
    assert!(rep.notices.iter().any(|n| n.contains("PLAINTEXT")));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(rep.backup_path.unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}

/// B5: derived graph rows, timeline details and unsalted chain hashes of
/// migrated rows are scrubbed; new private writes extract nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn b5_residue_is_scrubbed_and_new_writes_leave_none() {
    use sha2::{Digest, Sha256};
    let env = Env::new().await;
    let store = env.store().await;
    drop(store);
    let content = "plain residue body @zebraresidue #topicresidue";
    let old_hash = xavier::crypto::hex_encode(Sha256::digest(content.as_bytes()));
    {
        let conn = Connection::open(env.db()).unwrap();
        conn.execute(
            "INSERT INTO memory_records (id, workspace_id, path, content, metadata, created_at, updated_at) VALUES ('res-1','ws','notes/res',?1,'{\"clearance\":\"INTERNAL\"}','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
            params![content],
        )
        .unwrap();
        conn.execute("INSERT INTO entities (id, name, entity_type, properties, workspace_id) VALUES ('mem:ws:res-1','notes/res','memory','{}','ws')", []).unwrap();
        conn.execute("INSERT INTO entities (id, name, entity_type, properties, workspace_id) VALUES ('ent-z','@zebraresidue','mention','{}','ws')", []).unwrap();
        conn.execute("INSERT INTO memory_entities (id, workspace_id, memory_id, entity_id, relation_type) VALUES ('me-1','ws','res-1','ent-z','mentions')", []).unwrap();
        conn.execute("INSERT INTO relations (id, source_id, target_id, relation_type, properties, provenance_id, workspace_id) VALUES ('rel-1','mem:ws:res-1','ent-z','mentions','{}','res-1','ws')", []).unwrap();
        conn.execute(
            "INSERT INTO memory_chain (id, prev_hash, content_hash) VALUES ('ch-1', NULL, ?1)",
            params![old_hash],
        )
        .unwrap();
        conn.execute("INSERT INTO timeline_events (id, workspace_id, memory_id, sequence, timestamp, operation, summary, details, agent_id, prev_hash, curr_hash) VALUES ('ev-1','ws','res-1',1,'2026-01-01T00:00:00Z','create','create notes/res','{\"clearance\":\"INTERNAL\",\"leak\":\"TIMELINE-LEAK-4411\"}','a',NULL,'abc')", []).unwrap();
    }
    assert!(contains_bytes(&env.db(), "zebraresidue"));
    let rep = apply(&opts(&env, "bk.sqlite3")).unwrap();
    assert!(rep.complete && !rep.incomplete, "{rep:?}");
    let conn = Connection::open(env.db()).unwrap();
    let n = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
    assert_eq!(
        n("SELECT COUNT(*) FROM memory_entities WHERE memory_id='res-1'"),
        0
    );
    assert_eq!(
        n("SELECT COUNT(*) FROM relations WHERE provenance_id='res-1'"),
        0
    );
    assert_eq!(
        n("SELECT COUNT(*) FROM entities WHERE name='@zebraresidue'"),
        0,
        "orphan entity removed"
    );
    let chain: String = conn
        .query_row(
            "SELECT content_hash FROM memory_chain WHERE id='ch-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(chain, old_hash, "unsalted hash replaced by a keyed MAC");
    let details: String = conn
        .query_row(
            "SELECT details FROM timeline_events WHERE id='ev-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!details.contains("TIMELINE-LEAK"));
    drop(conn);
    assert!(
        rep.scrub.graph_links >= 2 && rep.scrub.chain_hashes >= 1 && rep.scrub.timeline_events == 1
    );
    assert!(!contains_bytes(&env.db(), "zebraresidue"));
    assert!(!contains_bytes(&env.db(), "TIMELINE-LEAK-4411"));

    // New private writes: no extracted entities, keyed chain hash.
    let store = env.store().await;
    store
        .put(rec(
            "res-2",
            "notes/res2",
            "fresh body @mentionfresh #topicfresh",
            Some("INTERNAL"),
        ))
        .await
        .unwrap();
    drop(store);
    assert!(!contains_bytes(&env.db(), "mentionfresh"));
    let conn = Connection::open(env.db()).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM entities WHERE name LIKE '%mentionfresh%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    let stored: String = conn
        .query_row(
            "SELECT content FROM memory_records WHERE id='res-2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let plain_sha = xavier::crypto::hex_encode(Sha256::digest(stored.as_bytes()));
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM memory_chain WHERE content_hash = ?1",
            params![plain_sha],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0,
        "chain hash of a private row is keyed"
    );
}

/// Minor 7: duplicate `clearance` keys are ambiguous and therefore private.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minor7_duplicate_clearance_keys_are_private() {
    let env = Env::new().await;
    drop(env.store().await);
    {
        let conn = Connection::open(env.db()).unwrap();
        conn.execute(
            "INSERT INTO memory_records (id, workspace_id, path, content, metadata, created_at, updated_at) VALUES ('dup-1','ws','p','dupbody',?1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
            params![r#"{"clearance":"INTERNAL","clearance":"PUBLIC"}"#],
        )
        .unwrap();
    }
    let dry = at_rest::dry_run_report(&env.db()).unwrap();
    assert_eq!(dry.plaintext_private, 1);
    assert_eq!(dry.plaintext_public, 0);
    let rep = apply(&opts(&env, "bk.sqlite3")).unwrap();
    assert_eq!(rep.changed, 1);
}

/// Minor 6: restore from backup refuses rows changed since the backup and a
/// tampered backup; `--force` overrides the row check.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minor6_restore_checks_manifest_and_changed_rows() {
    let env = Env::new().await;
    drop(env.store().await);
    seed_plain(&env.db(), 14);
    let rep = apply(&opts(&env, "bk.sqlite3")).unwrap();
    let backup = rep.backup_path.clone().unwrap();
    {
        let conn = Connection::open(env.db()).unwrap();
        conn.execute(
            "UPDATE memory_records SET updated_at = '2031-01-01T00:00:00Z', revision = revision + 1 WHERE id = 'seed-0'",
            [],
        )
        .unwrap();
    }
    let r = at_rest::apply_decrypt(
        &opts(&env, "pre1.sqlite3"),
        DecryptSource::FromBackup(backup.clone()),
    )
    .unwrap();
    assert!(r.complete);
    assert_eq!(r.failed, 1, "the edited row is refused");
    let s = snapshot(&env.db());
    assert!(s["seed-0"]
        .2
        .as_deref()
        .is_some_and(at_rest::is_default_space_row));
    // Resume with force restores it.
    let mut o = opts(&env, "pre2.sqlite3");
    o.force = true;
    let r = at_rest::apply_decrypt(&o, DecryptSource::FromBackup(backup.clone())).unwrap();
    assert_eq!(r.failed, 0);
    assert!(snapshot(&env.db())["seed-0"].2.is_none());

    // Tampered backup: manifest SHA mismatch.
    let mut bytes = std::fs::read(&backup).unwrap();
    let n = bytes.len();
    bytes[n - 1] ^= 0xFF;
    std::fs::write(&backup, bytes).unwrap();
    let err = at_rest::apply_decrypt(
        &opts(&env, "pre3.sqlite3"),
        DecryptSource::FromBackup(backup),
    )
    .unwrap_err();
    assert!(format!("{err:#}").contains("SHA-256"), "{err:#}");
}

/// Minor 1 + 4: the key cache follows the keystore bytes (not mtime/len), and
/// the KEK source is recorded in the keystore.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minor1_4_key_cache_fingerprint_and_kek_source() {
    let env = Env::new().await;
    let (h1, _code) = at_rest::create_default_keystore().unwrap();
    let ks_path = at_rest::default_keystore_path();
    let text = std::fs::read_to_string(&ks_path).unwrap();
    assert!(text.contains("\"kek_source\": \"env\""), "{text}");
    let mac1 = h1.verify_mac("t", b"x");
    assert_eq!(
        at_rest::default_space_key().unwrap().verify_mac("t", b"x"),
        mac1
    );

    // A second keystore of the same size, restored with the old mtime.
    let other = tempfile::tempdir().unwrap();
    std::env::set_var("XAVIER_DATA_DIR", other.path());
    let (h2, _c2) = at_rest::create_default_keystore().unwrap();
    let other_bytes = std::fs::read(at_rest::default_keystore_path()).unwrap();
    std::env::set_var("XAVIER_DATA_DIR", env.tmp.path().join("data"));
    assert_eq!(other_bytes.len(), std::fs::read(&ks_path).unwrap().len());
    let old_mtime = std::fs::metadata(&ks_path).unwrap().modified().unwrap();
    std::fs::write(&ks_path, &other_bytes).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&ks_path)
        .unwrap()
        .set_modified(old_mtime)
        .unwrap();
    let mac2 = h2.verify_mac("t", b"x");
    assert_ne!(mac1, mac2);
    assert_eq!(
        at_rest::default_space_key().unwrap().verify_mac("t", b"x"),
        mac2,
        "stale cached key must not survive a replaced keystore"
    );
}
