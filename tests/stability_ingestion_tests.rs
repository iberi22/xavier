//! Stability S1.10: cross-importer ingestion integration tests.
//!
//! The production embedding storm came from all four session importers
//! running TOGETHER every 10 minutes against one store. These tests prove
//! that a second identical import pass performs zero re-encodes — per
//! importer and with all four sharing a single store.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;
use xavier::embedding::{Embedder, EmbeddingError};
use xavier::memory::antigravity_importer::AntigravityImporter;
use xavier::memory::codex_importer::CodexImporter;
use xavier::memory::hermes_importer::HermesImporter;
use xavier::memory::opencode_importer::OpenCodeImporter;
use xavier::memory::store::InMemoryMemoryStore;

struct CountingEmbedder {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl Embedder for CountingEmbedder {
    async fn encode(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(vec![0.5; 8])
    }

    fn dimension(&self) -> usize {
        8
    }
}

fn counted() -> Arc<CountingEmbedder> {
    Arc::new(CountingEmbedder {
        calls: AtomicUsize::new(0),
    })
}

// ── fixtures (recipes copied from each importer's own unit tests) ──

fn write_hermes_db(dir: &std::path::Path) -> anyhow::Result<()> {
    let db_path = dir.join("session1.db");
    let conn = rusqlite::Connection::open(&db_path)?;
    conn.execute(
        "CREATE TABLE messages (id TEXT PRIMARY KEY, role TEXT, content TEXT)",
        [],
    )?;
    conn.execute(
        "INSERT INTO messages (id, role, content) VALUES ('msg1', 'user', 'Hello Hermes')",
        [],
    )?;
    conn.execute(
        "INSERT INTO messages (id, role, content) VALUES ('msg2', 'assistant', 'Hello User')",
        [],
    )?;
    Ok(())
}

async fn write_antigravity_dir(dir: &std::path::Path) -> anyhow::Result<()> {
    let logs_dir = dir
        .join("sess_test_123")
        .join(".system_generated")
        .join("logs");
    tokio::fs::create_dir_all(&logs_dir).await?;
    let lines = vec![
        serde_json::json!({"step_index": 0, "type": "USER_INPUT", "content": "Hello Antigravity!"}),
        serde_json::json!({"step_index": 1, "type": "PLANNER_RESPONSE", "content": "I am ready."}),
    ];
    let mut content = String::new();
    for l in lines {
        content.push_str(&l.to_string());
        content.push('\n');
    }
    tokio::fs::write(logs_dir.join("transcript.jsonl"), content).await?;
    Ok(())
}

async fn write_codex_dir(dir: &std::path::Path) -> anyhow::Result<()> {
    let content = serde_json::json!({
        "session_id": "codex-123",
        "created_at": "2026-08-15T12:00:00Z",
        "topic": "Refactoring module",
        "messages": [
            { "role": "user", "content": "Refactor codebase" },
            { "role": "assistant", "content": "Refactored successfully" }
        ]
    });
    tokio::fs::write(dir.join("sess_001.json"), serde_json::to_string(&content)?).await?;
    Ok(())
}

fn write_opencode_db(dir: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let db_file = dir.join("opencode.db");
    let conn = rusqlite::Connection::open(&db_file)?;
    conn.execute_batch(
        "CREATE TABLE session (
            id TEXT PRIMARY KEY, title TEXT, agent TEXT, model TEXT, time_created INTEGER
        );
        CREATE TABLE message (
            id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT
        );
        CREATE TABLE part (
            id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, data TEXT
        );
        INSERT INTO session VALUES ('ses_001', 'Fixing login bug', 'coder', '{\"id\":\"qwen-coder\"}', 1780000000);
        INSERT INTO message VALUES ('msg_001', 'ses_001', 1780000001, '{\"role\":\"user\"}');
        INSERT INTO message VALUES ('msg_002', 'ses_001', 1780000002, '{\"role\":\"assistant\"}');
        ",
    )?;
    let part1 = serde_json::json!({"type": "text", "text": "Please fix the auth loop"}).to_string();
    let part2 = serde_json::json!({"type": "text", "text": "Auth loop fixed!"}).to_string();
    conn.execute(
        "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params!["prt_001", "msg_001", "ses_001", 1780000001, part1],
    )?;
    conn.execute(
        "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params!["prt_002", "msg_002", "ses_001", 1780000002, part2],
    )?;
    Ok(db_file)
}

// ── per-importer double-import tests ──

#[tokio::test]
async fn hermes_double_import_no_reencode() -> anyhow::Result<()> {
    let dir = tempdir()?;
    write_hermes_db(dir.path())?;
    let store = InMemoryMemoryStore::new();
    let embedder = counted();
    let importer = HermesImporter::with_dir(dir.path()).with_embedder(embedder.clone());
    let first = importer.import_all(&store).await?;
    assert!(!first.is_empty());
    let after_first = embedder.calls.load(Ordering::SeqCst);
    let second = importer.import_all(&store).await?;
    assert_eq!(second.len(), first.len());
    assert_eq!(embedder.calls.load(Ordering::SeqCst), after_first);
    Ok(())
}

#[tokio::test]
async fn antigravity_double_import_no_reencode() -> anyhow::Result<()> {
    let dir = tempdir()?;
    write_antigravity_dir(dir.path()).await?;
    let store = InMemoryMemoryStore::new();
    let embedder = counted();
    let importer = AntigravityImporter::with_dir_and_embedder(dir.path(), embedder.clone());
    let first = importer.import_all(&store).await?;
    assert!(!first.is_empty());
    let after_first = embedder.calls.load(Ordering::SeqCst);
    let second = importer.import_all(&store).await?;
    assert_eq!(second.len(), first.len());
    assert_eq!(embedder.calls.load(Ordering::SeqCst), after_first);
    Ok(())
}

#[tokio::test]
async fn codex_double_import_no_reencode() -> anyhow::Result<()> {
    let dir = tempdir()?;
    write_codex_dir(dir.path()).await?;
    let store = InMemoryMemoryStore::new();
    let embedder = counted();
    let importer = CodexImporter::with_dir_and_embedder(dir.path(), embedder.clone());
    let first = importer.import_all(&store).await?;
    assert!(!first.is_empty());
    let after_first = embedder.calls.load(Ordering::SeqCst);
    let second = importer.import_all(&store).await?;
    assert_eq!(second.len(), first.len());
    assert_eq!(embedder.calls.load(Ordering::SeqCst), after_first);
    Ok(())
}

#[tokio::test]
async fn opencode_double_import_no_reencode() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let db_file = write_opencode_db(dir.path())?;
    let store = InMemoryMemoryStore::new();
    let embedder = counted();
    let importer = OpenCodeImporter::with_path(&db_file).with_embedder(embedder.clone());
    let first = importer.import_all(&store).await?;
    assert!(!first.is_empty());
    let after_first = embedder.calls.load(Ordering::SeqCst);
    let second = importer.import_all(&store).await?;
    assert_eq!(second.len(), first.len());
    assert_eq!(embedder.calls.load(Ordering::SeqCst), after_first);
    Ok(())
}

// ── shared-store test: the real production shape ──

#[tokio::test]
async fn all_importers_shared_store_second_pass_stable() -> anyhow::Result<()> {
    let hermes_dir = tempdir()?;
    write_hermes_db(hermes_dir.path())?;
    let ag_dir = tempdir()?;
    write_antigravity_dir(ag_dir.path()).await?;
    let codex_dir = tempdir()?;
    write_codex_dir(codex_dir.path()).await?;
    let oc_dir = tempdir()?;
    let oc_db = write_opencode_db(oc_dir.path())?;

    let store = InMemoryMemoryStore::new();
    let embedder = counted();

    // First cycle warms the store; count records per importer run.
    let h1 = HermesImporter::with_dir(hermes_dir.path())
        .with_embedder(embedder.clone())
        .import_all(&store)
        .await?
        .len();
    let a1 = AntigravityImporter::with_dir_and_embedder(ag_dir.path(), embedder.clone())
        .import_all(&store)
        .await?
        .len();
    let c1 = CodexImporter::with_dir_and_embedder(codex_dir.path(), embedder.clone())
        .import_all(&store)
        .await?
        .len();
    let o1 = OpenCodeImporter::with_path(&oc_db)
        .with_embedder(embedder.clone())
        .import_all(&store)
        .await?
        .len();
    assert!(h1 > 0 && a1 > 0 && c1 > 0 && o1 > 0);
    let after_first = embedder.calls.load(Ordering::SeqCst);

    // Second full cycle: same counts, zero new encodes.
    let h2 = HermesImporter::with_dir(hermes_dir.path())
        .with_embedder(embedder.clone())
        .import_all(&store)
        .await?
        .len();
    let a2 = AntigravityImporter::with_dir_and_embedder(ag_dir.path(), embedder.clone())
        .import_all(&store)
        .await?
        .len();
    let c2 = CodexImporter::with_dir_and_embedder(codex_dir.path(), embedder.clone())
        .import_all(&store)
        .await?
        .len();
    let o2 = OpenCodeImporter::with_path(&oc_db)
        .with_embedder(embedder.clone())
        .import_all(&store)
        .await?
        .len();
    assert_eq!((h2, a2, c2, o2), (h1, a1, c1, o1));
    assert_eq!(embedder.calls.load(Ordering::SeqCst), after_first);
    Ok(())
}
