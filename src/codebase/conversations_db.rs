//! Private conversations database.
//!
//! Stored at `~/.xavier/conversations/{project_id}.db`, this DB holds
//! AI conversation threads, messages, deductions, agent beliefs, and
//! session checkpoints. It is never committed to a repository.

use std::path::{Path, PathBuf};

use crate::codebase::connection_manager::ConnectionManager;
use crate::codebase::validate_project_id;
use crate::espacio::keys::{self, KeyHandle, DEFAULT_SPACE_ID, RECORD_PREFIX};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use uuid::Uuid;
use zeroize::Zeroizing;

const CONVERSATIONS_DIR: &str = "conversations";

// Hot read queries. The query sites and the index tests share these strings,
// so the planner checks run against exactly the SQL that ships.
const SQL_THREAD_MESSAGES: &str = "SELECT id, thread_id, role, content, tool_calls, openui_lang, xui_json, metadata, created_at, tokens
                 FROM conversation_messages
                 WHERE thread_id = ?1
                 ORDER BY created_at ASC";
const SQL_SESSION_EXISTS: &str = "SELECT EXISTS(SELECT 1 FROM conversation_threads WHERE id = ?1)
                     OR EXISTS(SELECT 1 FROM conversation_messages WHERE thread_id = ?1)
                     OR EXISTS(SELECT 1 FROM checkpoints WHERE session_id = ?1 AND project_id = ?2)";
const SQL_DEDUCTIONS_BY_PROJECT: &str = "SELECT id, project_id, source_thread, deduction, confidence, created_at, last_accessed, category
                 FROM deductions WHERE project_id = ?1 ORDER BY created_at DESC LIMIT ?2";
const SQL_CHECKPOINTS_ACTIVE: &str = "SELECT id, project_id, workspace_id, session_id, task_id, name, state, created_at, expires_at, kind
                 FROM checkpoints WHERE project_id = ?1 AND state = 'active' ORDER BY created_at DESC LIMIT ?2";

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A conversation thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub id: String,
    pub project_id: Option<String>,
    pub title: Option<String>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_preview: Option<String>,
    pub model: Option<String>,
    pub source: Option<String>,
}

pub use crate::domain::cycle_breaks::w30_10::Message;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadSummary {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_preview: String,
    pub message_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadDetail {
    pub thread: ThreadSummary,
    pub messages: Vec<Message>,
}

impl From<&Thread> for ThreadSummary {
    fn from(t: &Thread) -> Self {
        Self {
            id: t.id.clone(),
            title: t.title.clone().unwrap_or_else(|| "Untitled".to_string()),
            created_at: t.started_at,
            updated_at: t.updated_at,
            last_preview: t.last_preview.clone().unwrap_or_default(),
            message_count: 0, // Should be populated by caller
        }
    }
}

/// A deduction derived from conversations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deduction {
    pub id: String,
    pub project_id: Option<String>,
    pub source_thread: Option<String>,
    pub deduction: String,
    pub confidence: f64,
    pub created_at: DateTime<Utc>,
    pub last_accessed: Option<DateTime<Utc>>,
    pub category: Option<String>,
}

/// An agent's belief about something.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Belief {
    pub id: String,
    pub project_id: Option<String>,
    pub subject: String,
    pub relation: Option<String>,
    pub object: Option<String>,
    pub proposition: Option<String>,
    pub confidence: f64,
    pub weight: f64,
    pub source: Option<String>,
    pub provenance_id: Option<String>,
    pub contradicts_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A session checkpoint for resuming interrupted work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub name: Option<String>,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub kind: Option<String>,
}

// ---------------------------------------------------------------------------
// ConversationsDb
// ---------------------------------------------------------------------------

/// Manages the private per-project conversations database.
pub struct ConversationsDb {
    project_id: String,
    full_project_id: String,
    schema_initialized: OnceCell<()>,
    /// Encrypting spaces get no persistent conversations database: threads,
    /// messages, beliefs and checkpoints would sit in clear. Every operation
    /// fails and no file or pool is ever created.
    disabled: bool,
    /// Explicit node key (tests / injected roots); `None` resolves the node's key.
    node_key: Option<[u8; 32]>,
    /// Set once the snapshot table exists. Created lazily, only by the snapshot calls.
    snapshot_schema_initialized: OnceCell<()>,
}

impl ConversationsDb {
    /// Open (or create) the conversations database for `project_id`.
    ///
    /// The database is stored at `~/.xavier/conversations/{project_id}.db`.
    /// The directory is created if it doesn't exist.
    pub async fn open(project_id: &str) -> Result<Self> {
        validate_project_id(project_id)?;
        let full_project_id = format!("conv_{}", project_id);
        ConnectionManager::global().connect(&full_project_id, ".")?;

        Ok(Self {
            project_id: project_id.to_string(),
            full_project_id,
            schema_initialized: OnceCell::new(),
            disabled: false,
            node_key: None,
            snapshot_schema_initialized: OnceCell::new(),
        })
    }

    /// Pool id of the conversations database stored at `db_path`. Keyed by
    /// path, so two spaces with the same id under different roots never share
    /// a pool.
    pub fn connection_id_for_path(db_path: &Path) -> String {
        use sha2::{Digest, Sha256};
        let digest =
            crate::crypto::hex_encode(Sha256::digest(db_path.to_string_lossy().as_bytes()));
        format!("conv_at_{}", &digest[..16])
    }

    /// Open (or create) the conversations database at an explicit file path
    /// (a space keeps it inside its own directory). Never reads or writes
    /// `~/.xavier`.
    pub async fn open_at(project_id: &str, db_path: &Path) -> Result<Self> {
        validate_project_id(project_id)?;
        let full_project_id = Self::connection_id_for_path(db_path);
        ConnectionManager::global().connect_with_path(&full_project_id, db_path.to_path_buf())?;

        Ok(Self {
            project_id: project_id.to_string(),
            full_project_id,
            schema_initialized: OnceCell::new(),
            disabled: false,
            node_key: None,
            snapshot_schema_initialized: OnceCell::new(),
        })
    }

    /// A conversations handle that stores nothing (encrypting spaces).
    pub fn open_disabled(project_id: &str) -> Result<Self> {
        validate_project_id(project_id)?;
        Ok(Self {
            project_id: project_id.to_string(),
            full_project_id: String::new(),
            schema_initialized: OnceCell::new(),
            disabled: true,
            node_key: None,
            snapshot_schema_initialized: OnceCell::new(),
        })
    }

    /// Whether this handle refuses all operations (encrypting space).
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Open an in-memory conversations database (for testing).
    pub async fn open_in_memory(project_id: &str) -> Result<Self> {
        validate_project_id(project_id)?;
        let full_project_id = format!("conv_test_{}", project_id);
        ConnectionManager::global().connect(&full_project_id, ".")?;

        Ok(Self {
            project_id: project_id.to_string(),
            full_project_id,
            schema_initialized: OnceCell::new(),
            disabled: false,
            node_key: None,
            snapshot_schema_initialized: OnceCell::new(),
        })
    }

    /// Use an explicit node key instead of resolving the node's key.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn with_node_key(mut self, key: [u8; 32]) -> Self {
        self.node_key = Some(key);
        self
    }

    /// Pick the sealing key: explicit key, then the default-space key (fail
    /// closed when a keystore exists), then the node record key, else none.
    fn sealer(&self) -> Result<Sealer> {
        use crate::memory::sqlite_vec_store::at_rest;
        if let Some(k) = self.node_key {
            return Ok(Sealer::Node(k));
        }
        if at_rest::default_keystore_exists() {
            return Ok(Sealer::Dek(at_rest::default_space_key().map_err(|e| {
                e.context("conversations: default-space key unavailable (nothing was stored)")
            })?));
        }
        Ok(at_rest::peek_record_key_with_source()
            .0
            .map_or(Sealer::None, Sealer::Node))
    }

    fn refuse_if_disabled(&self) -> Result<()> {
        if self.disabled {
            anyhow::bail!("conversations are disabled for encrypted spaces");
        }
        Ok(())
    }

    /// Ensure the schema is created (lazy initialization).
    async fn ensure_schema(&self) -> Result<()> {
        self.refuse_if_disabled()?;
        self.schema_initialized
            .get_or_try_init(|| async { self.create_schema().await })
            .await?;
        Ok(())
    }

    /// Create all tables for the conversations database.
    pub async fn create_schema(&self) -> Result<()> {
        self.refuse_if_disabled()?;
        ConnectionManager::global()
            .with_conn(&self.full_project_id, |conn| {
                conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS conversation_threads (
                    id TEXT PRIMARY KEY,
                    project_id TEXT,
                    title TEXT,
                    started_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    last_preview TEXT,
                    model TEXT,
                    source TEXT
                );

                CREATE TABLE IF NOT EXISTS conversation_messages (
                    id TEXT PRIMARY KEY,
                    thread_id TEXT NOT NULL,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL,
                    tool_calls TEXT,
                    openui_lang TEXT,
                    xui_json TEXT,
                    metadata TEXT,
                    created_at TEXT NOT NULL,
                    tokens INTEGER,
                    FOREIGN KEY (thread_id) REFERENCES conversation_threads(id)
                );

                CREATE TABLE IF NOT EXISTS deductions (
                    id TEXT PRIMARY KEY,
                    project_id TEXT,
                    source_thread TEXT,
                    deduction TEXT NOT NULL,
                    confidence REAL DEFAULT 0.0,
                    created_at TEXT NOT NULL,
                    last_accessed TEXT,
                    category TEXT
                );

                CREATE TABLE IF NOT EXISTS beliefs (
                    id TEXT PRIMARY KEY,
                    project_id TEXT,
                    subject TEXT NOT NULL,
                    relation TEXT,
                    object TEXT,
                    proposition TEXT,
                    confidence REAL DEFAULT 0.0,
                    weight REAL DEFAULT 0.0,
                    source TEXT,
                    provenance_id TEXT,
                    contradicts_id TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS checkpoints (
                    id TEXT PRIMARY KEY,
                    project_id TEXT,
                    workspace_id TEXT,
                    session_id TEXT,
                    task_id TEXT,
                    name TEXT,
                    state TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    expires_at TEXT,
                    kind TEXT DEFAULT 'work'
                );

                CREATE INDEX IF NOT EXISTS idx_conversation_messages_thread_created
                    ON conversation_messages (thread_id, created_at);
                CREATE INDEX IF NOT EXISTS idx_checkpoints_project_state_created
                    ON checkpoints (project_id, state, created_at);
                CREATE INDEX IF NOT EXISTS idx_checkpoints_session_project
                    ON checkpoints (session_id, project_id);
                CREATE INDEX IF NOT EXISTS idx_deductions_project_created
                    ON deductions (project_id, created_at);",
                )
                .context("failed to create conversations schema")?;
                Ok(())
            })
            .await
    }

    /// Return the project ID.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    // ------------------------------------------------------------------
    // Thread CRUD
    // ------------------------------------------------------------------

    /// Create a new conversation thread.
    pub async fn create_thread(
        &self,
        title: Option<&str>,
        model: Option<&str>,
        source: Option<&str>,
    ) -> Result<Thread> {
        self.ensure_schema().await?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let pid = self.project_id.clone();
        let sealer = self.sealer()?;
        let title_c = sealer.seal_opt("conv.thread.title", &id, title)?;
        let model_c = model.map(|s| s.to_string());
        let source_c = source.map(|s| s.to_string());

        let id_for_return = id.clone();
        let title_for_return = title.map(|s| s.to_string());
        let model_for_return = model_c.clone();
        let source_for_return = source_c.clone();
        let pid_for_return = pid.clone();

        ConnectionManager::global().with_conn(&self.full_project_id, move |conn| {
            conn.execute(
                "INSERT INTO conversation_threads (id, project_id, title, started_at, updated_at, model, source)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, pid, title_c, now.clone(), now, model_c, source_c],
            ).context("failed to create thread")?;
            Ok(())
        }).await?;

        Ok(Thread {
            id: id_for_return,
            project_id: Some(pid_for_return),
            title: title_for_return,
            started_at: Utc::now(),
            updated_at: Utc::now(),
            last_preview: None,
            model: model_for_return,
            source: source_for_return,
        })
    }

    /// Get a thread by ID.
    pub async fn get_thread(&self, thread_id: &str) -> Result<Option<Thread>> {
        self.ensure_schema().await?;
        let sealer = self.sealer()?;
        let tid = thread_id.to_string();
        ConnectionManager::global()
            .with_conn(&self.full_project_id, move |conn| {
                let mut stmt = conn.prepare(
                "SELECT id, project_id, title, started_at, updated_at, last_preview, model, source
                 FROM conversation_threads WHERE id = ?1",
            )?;

                let mut rows = stmt.query(params![tid])?;

                if let Some(row) = rows.next()? {
                    let id: String = row.get(0)?;
                    Ok(Some(Thread {
                        project_id: row.get(1)?,
                        title: sealer.open_opt("conv.thread.title", &id, row.get(2)?)?,
                        started_at: parse_datetime(&row.get::<_, String>(3)?),
                        updated_at: parse_datetime(&row.get::<_, String>(4)?),
                        last_preview: sealer.open_opt("conv.thread.preview", &id, row.get(5)?)?,
                        id,
                        model: row.get(6)?,
                        source: row.get(7)?,
                    }))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    /// Delete a project's thread and its messages atomically.
    /// Returns false when the thread does not belong to this project or is absent.
    pub async fn delete_thread(&self, thread_id: &str) -> Result<bool> {
        self.ensure_schema().await?;
        let tid = thread_id.to_string();
        let pid = self.project_id.clone();
        ConnectionManager::global()
            .with_conn(&self.full_project_id, move |conn| {
                let tx = conn.unchecked_transaction()?;
                tx.execute(
                    "DELETE FROM conversation_messages WHERE thread_id = ?1
                     AND EXISTS (SELECT 1 FROM conversation_threads
                                 WHERE id = ?1 AND project_id = ?2)",
                    params![tid, pid],
                )
                .context("failed to delete thread messages")?;
                let deleted = tx.execute(
                    "DELETE FROM conversation_threads WHERE id = ?1 AND project_id = ?2",
                    params![tid, pid],
                )?;
                tx.commit().context("failed to commit thread deletion")?;
                Ok(deleted > 0)
            })
            .await
    }

    /// List all threads for a project.
    pub async fn list_threads(&self, limit: usize) -> Result<Vec<Thread>> {
        self.ensure_schema().await?;
        let sealer = self.sealer()?;
        let pid = self.project_id.clone();
        ConnectionManager::global()
            .with_conn(&self.full_project_id, move |conn| {
                let mut stmt = conn.prepare(
                "SELECT id, project_id, title, started_at, updated_at, last_preview, model, source
                 FROM conversation_threads
                 WHERE project_id = ?1
                 ORDER BY updated_at DESC
                 LIMIT ?2",
            )?;

                let mut rows = stmt.query(params![pid, limit as i64])?;

                let mut results = Vec::new();
                while let Some(row) = rows.next()? {
                    let id: String = row.get(0)?;
                    results.push(Thread {
                        project_id: row.get(1)?,
                        title: sealer.open_opt("conv.thread.title", &id, row.get(2)?)?,
                        started_at: parse_datetime(&row.get::<_, String>(3)?),
                        updated_at: parse_datetime(&row.get::<_, String>(4)?),
                        last_preview: sealer.open_opt("conv.thread.preview", &id, row.get(5)?)?,
                        id,
                        model: row.get(6)?,
                        source: row.get(7)?,
                    });
                }
                Ok(results)
            })
            .await
    }

    // ------------------------------------------------------------------
    // Message CRUD
    // ------------------------------------------------------------------

    /// Store a message in a thread.
    #[allow(clippy::too_many_arguments)]
    pub async fn store_message(
        &self,
        thread_id: &str,
        role: &str,
        content: &str,
        tool_calls: Option<&str>,
        openui_lang: Option<&str>,
        xui_json: Option<&str>,
        metadata: Option<&str>,
        tokens: Option<i64>,
    ) -> Result<Message> {
        self.ensure_schema().await?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        let thread_id_c = thread_id.to_string();
        let role_c = role.to_string();
        let sealer = self.sealer()?;
        let content_c = sealer.seal("conv.msg.content", &id, content)?;
        let tool_calls_c = sealer.seal_opt("conv.msg.tool_calls", &id, tool_calls)?;
        let openui_lang_c = sealer.seal_opt("conv.msg.openui_lang", &id, openui_lang)?;
        let xui_json_c = sealer.seal_opt("conv.msg.xui_json", &id, xui_json)?;
        let metadata_c = sealer.seal_opt("conv.msg.metadata", &id, metadata)?;

        let id_for_return = id.clone();
        let now_c = now.clone();
        let thread_id_for_update = thread_id.to_string();

        ConnectionManager::global().with_conn(&self.full_project_id, move |conn| {
            conn.execute(
                "INSERT INTO conversation_messages (id, thread_id, role, content, tool_calls, openui_lang, xui_json, metadata, created_at, tokens)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![id, thread_id_c, role_c, content_c, tool_calls_c, openui_lang_c, xui_json_c, metadata_c, now_c.clone(), tokens],
            ).context("failed to store message")?;

            // Update thread's updated_at timestamp
            let _ = conn.execute(
                "UPDATE conversation_threads SET updated_at = ?1 WHERE id = ?2",
                params![now_c, thread_id_for_update.clone()],
            );
            Ok(())
        }).await?;

        // After successfully inserting the message, let's load all messages of this thread,
        // generate the extractive summary, and save it to the thread's `last_preview`!
        if let Ok(messages) = self.get_thread_messages(thread_id).await {
            let summary =
                crate::domain::cycle_breaks::w30_10::summarize_session_extractive(&messages);
            let _ = self.update_last_preview(thread_id, &summary).await;
        }

        Ok(Message {
            id: id_for_return,
            thread_id: thread_id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: tool_calls.map(|s| s.to_string()),
            openui_lang: openui_lang.map(|s| s.to_string()),
            xui_json: xui_json.map(|s| s.to_string()),
            metadata: metadata.map(|s| s.to_string()),
            created_at: Utc::now(),
            tokens,
        })
    }

    /// Update the last_preview of a thread.
    pub async fn update_last_preview(&self, thread_id: &str, preview: &str) -> Result<()> {
        self.ensure_schema().await?;
        let thread_id_c = thread_id.to_string();
        let preview_c = self
            .sealer()?
            .seal("conv.thread.preview", thread_id, preview)?;
        let now_c = Utc::now().to_rfc3339();

        ConnectionManager::global()
            .with_conn(&self.full_project_id, move |conn| {
                let _ = conn.execute(
                "UPDATE conversation_threads SET updated_at = ?1, last_preview = ?2 WHERE id = ?3",
                params![now_c, preview_c, thread_id_c],
            );
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Get all messages for a thread, ordered by creation time.
    pub async fn get_thread_messages(&self, thread_id: &str) -> Result<Vec<Message>> {
        self.ensure_schema().await?;
        let tid = thread_id.to_string();
        let sealer = self.sealer()?;
        ConnectionManager::global()
            .with_conn(&self.full_project_id, move |conn| {
                let mut stmt = conn.prepare(SQL_THREAD_MESSAGES)?;

                let mut rows = stmt.query(params![tid])?;

                let mut results = Vec::new();
                while let Some(row) = rows.next()? {
                    let id: String = row.get(0)?;
                    results.push(Message {
                        thread_id: row.get(1)?,
                        role: row.get(2)?,
                        content: sealer.open("conv.msg.content", &id, row.get(3)?)?,
                        tool_calls: sealer.open_opt("conv.msg.tool_calls", &id, row.get(4)?)?,
                        openui_lang: sealer.open_opt("conv.msg.openui_lang", &id, row.get(5)?)?,
                        xui_json: sealer.open_opt("conv.msg.xui_json", &id, row.get(6)?)?,
                        metadata: sealer.open_opt("conv.msg.metadata", &id, row.get(7)?)?,
                        id,
                        created_at: parse_datetime(&row.get::<_, String>(8)?),
                        tokens: row.get(9)?,
                    });
                }
                Ok(results)
            })
            .await
    }

    /// Whether a thread, its messages, or a checkpoint exists for this session id.
    pub async fn session_exists(&self, session_id: &str) -> Result<bool> {
        self.ensure_schema().await?;
        let sid = session_id.to_string();
        let pid = self.project_id.clone();
        ConnectionManager::global()
            .with_conn(&self.full_project_id, move |conn| {
                let exists: bool =
                    conn.query_row(SQL_SESSION_EXISTS, params![sid, pid], |row| row.get(0))?;
                Ok(exists)
            })
            .await
    }

    // ------------------------------------------------------------------
    // Deduction CRUD
    // ------------------------------------------------------------------

    /// Record a deduction.
    pub async fn record_deduction(
        &self,
        deduction_text: &str,
        confidence: f64,
        category: Option<&str>,
        source_thread: Option<&str>,
    ) -> Result<Deduction> {
        self.ensure_schema().await?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        let pid = self.project_id.clone();
        let deduction_c = self
            .sealer()?
            .seal("conv.deduction.text", &id, deduction_text)?;
        let category_c = category.map(|s| s.to_string());
        let source_thread_c = source_thread.map(|s| s.to_string());

        let id_for_return = id.clone();
        let now_c = now.clone();

        ConnectionManager::global().with_conn(&self.full_project_id, move |conn| {
            conn.execute(
                "INSERT INTO deductions (id, project_id, source_thread, deduction, confidence, created_at, last_accessed, category)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![id, pid, source_thread_c, deduction_c, confidence, now_c.clone(), now_c, category_c],
            ).context("failed to record deduction")?;
            Ok(())
        }).await?;

        Ok(Deduction {
            id: id_for_return,
            project_id: Some(self.project_id.clone()),
            source_thread: source_thread.map(|s| s.to_string()),
            deduction: deduction_text.to_string(),
            confidence,
            created_at: Utc::now(),
            last_accessed: Some(Utc::now()),
            category: category.map(|s| s.to_string()),
        })
    }

    /// List deductions for this project.
    pub async fn list_deductions(&self, limit: usize) -> Result<Vec<Deduction>> {
        self.ensure_schema().await?;
        let sealer = self.sealer()?;
        let pid = self.project_id.clone();
        ConnectionManager::global()
            .with_conn(&self.full_project_id, move |conn| {
                let mut stmt = conn.prepare(SQL_DEDUCTIONS_BY_PROJECT)?;

                let mut rows = stmt.query(params![pid, limit as i64])?;

                let mut results = Vec::new();
                while let Some(row) = rows.next()? {
                    let id: String = row.get(0)?;
                    results.push(Deduction {
                        project_id: row.get(1)?,
                        source_thread: row.get(2)?,
                        deduction: sealer.open("conv.deduction.text", &id, row.get(3)?)?,
                        id,
                        confidence: row.get(4)?,
                        created_at: parse_datetime(&row.get::<_, String>(5)?),
                        last_accessed: row.get::<_, Option<String>>(6)?.map(|s| parse_datetime(&s)),
                        category: row.get(7)?,
                    });
                }
                Ok(results)
            })
            .await
    }

    // ------------------------------------------------------------------
    // Belief CRUD
    // ------------------------------------------------------------------

    /// Upsert a belief (create or update by subject+relation+object or subject+proposition).
    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_belief(
        &self,
        subject: &str,
        relation: Option<&str>,
        object: Option<&str>,
        proposition: Option<&str>,
        confidence: f64,
        weight: f64,
        source: Option<&str>,
        provenance_id: Option<&str>,
        contradicts_id: Option<&str>,
    ) -> Result<Belief> {
        self.ensure_schema().await?;
        let now = Utc::now().to_rfc3339();

        let pid = self.project_id.clone();
        let sealer = self.sealer()?;
        let subject_c = subject.to_string();
        let relation_c = relation.map(|s| s.to_string());
        let object_c = object.map(|s| s.to_string());
        let proposition_c = proposition.map(|s| s.to_string());
        let source_c = source.map(|s| s.to_string());
        let provenance_id_c = provenance_id.map(|s| s.to_string());
        let contradicts_id_c = contradicts_id.map(|s| s.to_string());
        let now_c = now.clone();

        let id = ConnectionManager::global().with_conn(&self.full_project_id, move |conn| {
            // Sealed columns cannot be matched in SQL: compare after opening.
            let mut existing_id: Option<String> = None;
            {
                let mut stmt = conn.prepare(
                    "SELECT id, subject, relation, object, proposition FROM beliefs WHERE project_id = ?1",
                )?;
                let mut rows = stmt.query(params![pid.clone()])?;
                while let Some(row) = rows.next()? {
                    let rid: String = row.get(0)?;
                    let subj = sealer.open("conv.belief.subject", &rid, row.get(1)?)?;
                    if subj != subject_c {
                        continue;
                    }
                    let matched = if let (Some(rel), Some(obj)) = (&relation_c, &object_c) {
                        sealer.open_opt("conv.belief.relation", &rid, row.get(2)?)?.as_deref() == Some(rel.as_str())
                            && sealer.open_opt("conv.belief.object", &rid, row.get(3)?)?.as_deref() == Some(obj.as_str())
                    } else if let Some(prop) = &proposition_c {
                        sealer.open_opt("conv.belief.proposition", &rid, row.get(4)?)?.as_deref() == Some(prop.as_str())
                    } else {
                        true
                    };
                    if matched {
                        existing_id = Some(rid);
                        break;
                    }
                }
            }

            match existing_id {
                Some(id) => {
                    let subject_sealed = sealer.seal("conv.belief.subject", &id, &subject_c)?;
                    let relation_c = sealer.seal_opt("conv.belief.relation", &id, relation_c.as_deref())?;
                    let object_c = sealer.seal_opt("conv.belief.object", &id, object_c.as_deref())?;
                    let proposition_c = sealer.seal_opt("conv.belief.proposition", &id, proposition_c.as_deref())?;
                    conn.execute(
                        "UPDATE beliefs SET relation = ?1, object = ?2, proposition = ?3, confidence = ?4, \
                         weight = ?5, source = ?6, provenance_id = ?7, contradicts_id = ?8, updated_at = ?9, subject = ?11 WHERE id = ?10",
                        params![
                            relation_c, object_c, proposition_c, confidence,
                            weight, source_c, provenance_id_c, contradicts_id_c,
                            now_c.clone(), id, subject_sealed
                        ],
                    ).context("failed to update belief")?;
                    Ok(id)
                }
                None => {
                    let new_id = Uuid::new_v4().to_string();
                    let subject_c = sealer.seal("conv.belief.subject", &new_id, &subject_c)?;
                    let relation_c = sealer.seal_opt("conv.belief.relation", &new_id, relation_c.as_deref())?;
                    let object_c = sealer.seal_opt("conv.belief.object", &new_id, object_c.as_deref())?;
                    let proposition_c = sealer.seal_opt("conv.belief.proposition", &new_id, proposition_c.as_deref())?;
                    conn.execute(
                        "INSERT INTO beliefs (id, project_id, subject, relation, object, proposition, \
                         confidence, weight, source, provenance_id, contradicts_id, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                        params![
                            new_id.clone(), pid, subject_c, relation_c, object_c, proposition_c,
                            confidence, weight, source_c, provenance_id_c, contradicts_id_c, now_c.clone(), now_c
                        ],
                    ).context("failed to insert belief")?;
                    Ok(new_id)
                }
            }
        }).await?;

        self.get_belief(&id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("failed to retrieve upserted belief"))
    }

    /// Get a belief by ID.
    pub async fn get_belief(&self, id: &str) -> Result<Option<Belief>> {
        self.ensure_schema().await?;
        let bid = id.to_string();
        let sealer = self.sealer()?;
        ConnectionManager::global().with_conn(&self.full_project_id, move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, subject, relation, object, proposition, confidence, weight, source, provenance_id, contradicts_id, created_at, updated_at
                 FROM beliefs WHERE id = ?1",
            )?;

            let mut rows = stmt.query(params![bid])?;

            if let Some(row) = rows.next()? {
                let rid: String = row.get(0)?;
                Ok(Some(Belief {
                    project_id: row.get(1)?,
                    subject: sealer.open("conv.belief.subject", &rid, row.get(2)?)?,
                    relation: sealer.open_opt("conv.belief.relation", &rid, row.get(3)?)?,
                    object: sealer.open_opt("conv.belief.object", &rid, row.get(4)?)?,
                    proposition: sealer.open_opt("conv.belief.proposition", &rid, row.get(5)?)?,
                    id: rid,
                    confidence: row.get(6)?,
                    weight: row.get(7)?,
                    source: row.get(8)?,
                    provenance_id: row.get(9)?,
                    contradicts_id: row.get(10)?,
                    created_at: parse_datetime(&row.get::<_, String>(11)?),
                    updated_at: parse_datetime(&row.get::<_, String>(12)?),
                }))
            } else {
                Ok(None)
            }
        }).await
    }

    /// List all beliefs for this project.
    pub async fn list_beliefs(&self, limit: usize) -> Result<Vec<Belief>> {
        self.ensure_schema().await?;
        let sealer = self.sealer()?;
        let pid = self.project_id.clone();
        ConnectionManager::global().with_conn(&self.full_project_id, move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, subject, relation, object, proposition, confidence, weight, source, provenance_id, contradicts_id, created_at, updated_at
                 FROM beliefs WHERE project_id = ?1 ORDER BY updated_at DESC LIMIT ?2",
            )?;

            let mut rows = stmt.query(params![pid, limit as i64])?;

            let mut results = Vec::new();
            while let Some(row) = rows.next()? {
                let rid: String = row.get(0)?;
                results.push(Belief {
                    project_id: row.get(1)?,
                    subject: sealer.open("conv.belief.subject", &rid, row.get(2)?)?,
                    relation: sealer.open_opt("conv.belief.relation", &rid, row.get(3)?)?,
                    object: sealer.open_opt("conv.belief.object", &rid, row.get(4)?)?,
                    proposition: sealer.open_opt("conv.belief.proposition", &rid, row.get(5)?)?,
                    id: rid,
                    confidence: row.get(6)?, weight: row.get(7)?,
                    source: row.get(8)?, provenance_id: row.get(9)?,
                    contradicts_id: row.get(10)?,
                    created_at: parse_datetime(&row.get::<_, String>(11)?),
                    updated_at: parse_datetime(&row.get::<_, String>(12)?),
                });
            }
            Ok(results)
        }).await
    }

    // ------------------------------------------------------------------
    // Checkpoint CRUD
    // ------------------------------------------------------------------

    /// Create a checkpoint.
    pub async fn create_checkpoint(
        &self,
        workspace_id: Option<&str>,
        session_id: Option<&str>,
        task_id: Option<&str>,
        name: Option<&str>,
        kind: Option<&str>,
    ) -> Result<Checkpoint> {
        self.ensure_schema().await?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        let pid = self.project_id.clone();
        let workspace_id_c = workspace_id.map(|s| s.to_string());
        let session_id_c = session_id.map(|s| s.to_string());
        let task_id_c = task_id.map(|s| s.to_string());
        let name_c = name.map(|s| s.to_string());
        let kind_c = kind.map(|s| s.to_string());

        let id_for_return = id.clone();
        let now_c = now.clone();

        ConnectionManager::global().with_conn(&self.full_project_id, move |conn| {
            conn.execute(
                "INSERT INTO checkpoints (id, project_id, workspace_id, session_id, task_id, name, state, created_at, kind)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![id, pid, workspace_id_c, session_id_c, task_id_c, name_c, "active", now_c, kind_c],
            ).context("failed to create checkpoint")?;
            Ok(())
        }).await?;

        Ok(Checkpoint {
            id: id_for_return,
            project_id: Some(self.project_id.clone()),
            workspace_id: workspace_id.map(|s| s.to_string()),
            session_id: session_id.map(|s| s.to_string()),
            task_id: task_id.map(|s| s.to_string()),
            name: name.map(|s| s.to_string()),
            state: "active".to_string(),
            created_at: Utc::now(),
            expires_at: None,
            kind: kind.map(|s| s.to_string()),
        })
    }

    /// List active checkpoints.
    pub async fn list_checkpoints(&self, limit: usize) -> Result<Vec<Checkpoint>> {
        self.ensure_schema().await?;
        let pid = self.project_id.clone();
        ConnectionManager::global()
            .with_conn(&self.full_project_id, move |conn| {
                let mut stmt = conn.prepare(SQL_CHECKPOINTS_ACTIVE)?;

                let mut rows = stmt.query(params![pid, limit as i64])?;

                let mut results = Vec::new();
                while let Some(row) = rows.next()? {
                    results.push(Checkpoint {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        workspace_id: row.get(2)?,
                        session_id: row.get(3)?,
                        task_id: row.get(4)?,
                        name: row.get(5)?,
                        state: row.get(6)?,
                        created_at: parse_datetime(&row.get::<_, String>(7)?),
                        expires_at: row.get::<_, Option<String>>(8)?.map(|s| parse_datetime(&s)),
                        kind: row.get(9)?,
                    });
                }
                Ok(results)
            })
            .await
    }

    // ------------------------------------------------------------------
    // Path helpers (port of original code)
    // ------------------------------------------------------------------

    /// Build the database path for a given project_id, with path traversal validation.
    pub fn conversations_db_path(project_id: &str) -> PathBuf {
        validate_project_id(project_id).expect("Invalid project_id");

        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let mut path = home;
        path.push(".xavier");
        path.push(CONVERSATIONS_DIR);

        let mut db_file = path.clone();
        db_file.push(format!("{}.db", project_id));

        // Verify that the resolved path is within the expected directory
        if let Ok(canonical_base) = std::fs::canonicalize(&path) {
            let resolved = canonical_base.join(format!("{}.db", project_id));
            assert!(
                resolved.starts_with(&canonical_base),
                "Path escape detected"
            );
        }

        db_file
    }

    /// Migrate legacy JSON session files to the database.
    pub async fn migrate_legacy_sessions(&self, sessions_dir: &Path) -> Result<usize> {
        self.ensure_schema().await?;
        if !sessions_dir.exists() {
            return Ok(0);
        }

        let mut entries = tokio::fs::read_dir(sessions_dir).await?;
        let mut count = 0;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            let content = tokio::fs::read_to_string(&path).await?;
            let thread_data: serde_json::Value = serde_json::from_str(&content)?;

            let id = thread_data["id"].as_str().unwrap_or_default().to_string();
            if id.is_empty() {
                continue;
            }

            // Check if thread already exists
            if self.get_thread(&id).await?.is_some() {
                continue;
            }

            let title = thread_data["title"].as_str().map(|s| s.to_string());
            let started_at = thread_data["created_at"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let updated_at = thread_data["updated_at"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let last_preview = thread_data["last_preview"].as_str().map(|s| s.to_string());

            let id_c = id.clone();
            let pid = self.project_id.clone();
            let sealer = self.sealer()?;
            let title_c = sealer.seal_opt("conv.thread.title", &id, title.as_deref())?;
            let started_at_c = started_at.clone();
            let updated_at_c = updated_at.clone();
            let last_preview_c =
                sealer.seal_opt("conv.thread.preview", &id, last_preview.as_deref())?;
            let thread_data_clone = thread_data.clone();

            ConnectionManager::global().with_conn(&self.full_project_id, move |conn| {
                conn.execute(
                    "INSERT INTO conversation_threads (id, project_id, title, started_at, updated_at, last_preview)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![id_c.clone(), pid, title_c, started_at_c, updated_at_c, last_preview_c],
                )?;

                if let Some(messages) = thread_data_clone["messages"].as_array() {
                    for msg in messages {
                        let msg_id = msg["id"].as_str().unwrap_or_default().to_string();
                        let role = msg["role"].as_str().unwrap_or("user");
                        let mcontent = msg["plain_text"].as_str().unwrap_or("");
                        let created_at = msg["created_at"].as_str().unwrap_or_default();
                        let xui_json = msg["xui_json"].as_str();
                        let openui_lang = msg["openui_lang"].as_str();
                        let metadata = msg["metadata"].to_string();
                        let mcontent = sealer.seal("conv.msg.content", &msg_id, mcontent)?;
                        let xui_json = sealer.seal_opt("conv.msg.xui_json", &msg_id, xui_json)?;
                        let openui_lang = sealer.seal_opt("conv.msg.openui_lang", &msg_id, openui_lang)?;
                        let metadata = sealer.seal("conv.msg.metadata", &msg_id, &metadata)?;

                        conn.execute(
                            "INSERT INTO conversation_messages (id, thread_id, role, content, xui_json, openui_lang, metadata, created_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                            params![msg_id, id_c.clone(), role, mcontent, xui_json, openui_lang, metadata, created_at],
                        )?;
                    }
                }
                Ok(())
            }).await?;

            count += 1;
        }

        Ok(count)
    }

    /// Open an in-memory conversations database (for testing).
    #[cfg(any(test, feature = "test-utils"))]
    pub fn new_test() -> Self {
        Self {
            project_id: "test".to_string(),
            full_project_id: "conv_test_default".to_string(),
            schema_initialized: OnceCell::new(),
            disabled: false,
            node_key: None,
            snapshot_schema_initialized: OnceCell::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Encryption at rest (legacy clear rows stay readable)
// ---------------------------------------------------------------------------

const NODE_SEAL_PREFIX: &str = "xcn1:";

/// Key material used to seal text columns.
#[derive(Clone)]
enum Sealer {
    /// Default-space data key (`xr1:` envelope, AD binds column kind + row id).
    Dek(crate::espacio::keys::KeyHandle),
    /// Node record key (`xcn1:` envelope), used before a keystore exists.
    Node([u8; 32]),
    /// No key available: rows are written as before (clear).
    None,
}

fn is_node_sealed(v: &str) -> bool {
    v.strip_prefix(NODE_SEAL_PREFIX).is_some_and(|b| {
        b.len() >= 56 && b.len() % 2 == 0 && b.bytes().all(|c| c.is_ascii_hexdigit())
    })
}

impl Sealer {
    fn seal(&self, kind: &str, id: &str, plain: &str) -> Result<String> {
        match self {
            Sealer::Dek(k) => {
                k.seal_record(crate::espacio::keys::DEFAULT_SPACE_ID, kind, id, plain)
            }
            Sealer::Node(key) => {
                let nonce = crate::crypto::encryption::NonceBytes::generate();
                let ct = crate::crypto::encryption::aes_encrypt(plain.as_bytes(), key, &nonce)
                    .map_err(|e| anyhow::anyhow!("conversation seal failed: {e}"))?;
                Ok(format!(
                    "{NODE_SEAL_PREFIX}{}",
                    crate::crypto::hex_encode(ct)
                ))
            }
            Sealer::None => Ok(plain.to_string()),
        }
    }

    fn seal_opt(&self, kind: &str, id: &str, plain: Option<&str>) -> Result<Option<String>> {
        plain.map(|p| self.seal(kind, id, p)).transpose()
    }

    /// Open a stored value. Values without a seal envelope are legacy clear rows.
    fn open(&self, kind: &str, id: &str, stored: String) -> Result<String> {
        use crate::memory::sqlite_vec_store::at_rest;
        if crate::espacio::keys::looks_encrypted(&stored) {
            let dek = match self {
                Sealer::Dek(k) => k.clone(),
                _ => at_rest::default_space_key()?,
            };
            return dek.open_record(crate::espacio::keys::DEFAULT_SPACE_ID, kind, id, &stored);
        }
        if is_node_sealed(&stored) {
            let key = match self {
                Sealer::Node(k) => *k,
                _ => at_rest::peek_record_key_with_source()
                    .0
                    .ok_or_else(|| anyhow::anyhow!("sealed conversation row but no node key"))?,
            };
            let raw = crate::crypto::hex_decode(&stored[NODE_SEAL_PREFIX.len()..])
                .map_err(|e| anyhow::anyhow!("conversation row is corrupt: {e}"))?;
            let pt = crate::crypto::encryption::aes_decrypt(&raw, &key)
                .map_err(|e| anyhow::anyhow!("conversation row failed authentication: {e}"))?;
            return String::from_utf8(pt).map_err(|_| anyhow::anyhow!("row is not valid UTF-8"));
        }
        Ok(stored)
    }

    fn open_opt(&self, kind: &str, id: &str, stored: Option<String>) -> Result<Option<String>> {
        stored.map(|s| self.open(kind, id, s)).transpose()
    }
}

// ---------------------------------------------------------------------------
// Sealed context snapshots
// ---------------------------------------------------------------------------

/// Session ids: 1-128 chars of ASCII alnum . _ - : / with no `..`.
pub(crate) fn validate_session_id(session_id: &str) -> Result<(), String> {
    let ok = !session_id.is_empty()
        && session_id.len() <= 128
        && !session_id.contains("..")
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '/'));
    if ok {
        Ok(())
    } else {
        Err("session_id must be 1-128 chars of ASCII alphanumerics or . _ - : / and not contain '..'"
            .to_string())
    }
}

/// Largest transcript body a snapshot stores. Larger transcripts are refused, never truncated.
const MAX_BODY: usize = 65_536;
/// Longest workspace, project and session id a snapshot accepts.
const MAX_ID: usize = 128;
/// Length of a canonical checkpoint id (hyphenated lowercase UUID).
const CID_LEN: usize = 36;
/// Frame header: magic, version, three ids, checkpoint id, body length.
const MAX_HEADER: usize = 4 + 1 + 3 * (4 + MAX_ID) + (4 + CID_LEN) + 4;
const MAX_INNER: usize = MAX_HEADER + MAX_BODY;
/// AEAD overhead: 12-byte nonce plus 16-byte tag.
const AEAD_OVERHEAD: usize = 12 + 16;
const MAX_RAW: usize = AEAD_OVERHEAD + MAX_INNER;
/// Longest stored value per tier, prefix included.
const MAX_STORED_NODE: usize = "xsn1:".len() + 2 * MAX_RAW;
const MAX_STORED_DEK: usize = RECORD_PREFIX.len() + 2 * MAX_RAW;
/// Shortest raw blob that can authenticate.
const MIN_RAW: usize = AEAD_OVERHEAD;
const _: () = assert!(MAX_STORED_NODE == 132_023 && MAX_STORED_DEK == 132_022);

const SNAPSHOT_MAGIC: &[u8; 4] = b"XSNP";
const SNAPSHOT_FRAME_VERSION: u8 = 1;
/// Stored prefix of a node-keyed snapshot.
const NODE_SNAPSHOT_PREFIX: &str = "xsn1:";
/// Record kind of a default-space snapshot (its AD kind).
const SNAPSHOT_DEK_KIND: &str = "conv.snapshot.v1";

const SQL_SNAPSHOT_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS checkpoint_snapshots (
    checkpoint_id TEXT PRIMARY KEY NOT NULL,
    context_sealed TEXT NOT NULL CHECK(typeof(context_sealed) = 'text')
);";
const SQL_SNAPSHOT_MEMBER: &str =
    "SELECT 1 FROM conversation_threads WHERE id = ?1 AND project_id = ?2";
const SQL_SNAPSHOT_MESSAGES: &str = "SELECT id, role, content FROM conversation_messages
    WHERE thread_id = ?1 ORDER BY created_at ASC, id ASC";
/// The value column is NULL when it is not text or exceeds the bound (?5, in
/// bytes), so an oversized value is never fetched.
const SQL_SNAPSHOT_LOAD: &str = "SELECT typeof(s.context_sealed),
       CASE WHEN typeof(s.context_sealed) = 'text'
             AND length(CAST(s.context_sealed AS BLOB)) <= ?5 THEN s.context_sealed END
FROM checkpoints c JOIN checkpoint_snapshots s ON s.checkpoint_id = c.id
WHERE c.id = ?1 AND c.project_id = ?2 AND c.workspace_id = ?3 AND c.session_id = ?4
  AND c.state = 'active'";

/// Key tier a snapshot is sealed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotTier {
    Node,
    Dek,
}

impl SnapshotTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Dek => "dek",
        }
    }
}

/// Sealing policy, resolved once per operation. There is no unsealed arm.
enum SnapshotPolicy {
    Node([u8; 32]),
    Dek(KeyHandle),
}

impl SnapshotPolicy {
    fn tier(&self) -> SnapshotTier {
        match self {
            Self::Node(_) => SnapshotTier::Node,
            Self::Dek(_) => SnapshotTier::Dek,
        }
    }
}

/// Ids a snapshot is bound to. Only [`SnapshotScope::new`] builds one, so the
/// ids are validated and agree with each other.
#[derive(Debug, Clone)]
pub struct SnapshotScope {
    workspace_id: String,
    project_id: String,
    session_id: String,
}

impl SnapshotScope {
    /// Validate the ids, then require `workspace == config == db project`.
    pub fn new(
        workspace_id: &str,
        config_id: &str,
        db_project: &str,
        session_id: &str,
    ) -> Result<Self, SnapshotError> {
        if !(is_snapshot_project_id(workspace_id)
            && is_snapshot_project_id(config_id)
            && is_snapshot_project_id(db_project)
            && validate_session_id(session_id).is_ok())
        {
            return Err(SnapshotError::InvalidScope);
        }
        if workspace_id != config_id || config_id != db_project {
            return Err(SnapshotError::ScopeMismatch);
        }
        Ok(Self {
            workspace_id: workspace_id.to_string(),
            project_id: db_project.to_string(),
            session_id: session_id.to_string(),
        })
    }
}

/// Workspace and project id rule: the project-id charset, at most [`MAX_ID`] bytes.
fn is_snapshot_project_id(id: &str) -> bool {
    id.len() <= MAX_ID && validate_project_id(id).is_ok()
}

/// Canonical checkpoint id: a lowercase hyphenated UUID, byte for byte.
fn is_canonical_cid(cid: &str) -> bool {
    Uuid::parse_str(cid).is_ok_and(|u| u.to_string() == cid)
}

/// Lowercase hex only, even length. `keys` accepts uppercase, so snapshots check first.
fn is_lower_hex(s: &str) -> bool {
    s.len().is_multiple_of(2) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("snapshot scope is invalid")]
    InvalidScope,
    #[error("snapshot scope does not match the database")]
    ScopeMismatch,
    #[error("context snapshots are disabled")]
    Disabled,
    #[error("session has no conversation thread in this workspace")]
    NotMember,
    #[error("session has no messages")]
    Empty,
    #[error("transcript exceeds the snapshot size limit")]
    TooLarge,
    #[error("a stored message could not be read")]
    SourceUnreadable,
    #[error("no sealing key is configured")]
    NoSealKey,
    #[error("sealing key unavailable")]
    KeyUnavailable,
    #[error("no such snapshot")]
    Missing,
    #[error("stored snapshot exceeds the size limit")]
    Oversized,
    #[error("snapshot failed verification")]
    Tampered,
    #[error("snapshot is sealed under a weaker tier than the policy")]
    WeakerThanPolicy,
    #[error("snapshot is stored in clear under a sealing policy")]
    ClearUnderSealPolicy,
    #[error("snapshot is sealed under a different key tier")]
    TierMismatch,
    #[error("context store error: {0}")]
    Db(String),
}

/// Result of a successful capture.
#[derive(Debug, Clone)]
pub struct SavedSnapshot {
    pub checkpoint_id: String,
    pub tier: SnapshotTier,
}

/// A restored transcript body (canonical JSON of the messages).
#[derive(Debug, Clone)]
pub struct LoadedSnapshot {
    pub body: String,
    pub tier: SnapshotTier,
}

/// One message as it is stored in a snapshot body. Field order is the wire order.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotMessage {
    id: String,
    role: String,
    content: String,
}

fn db_err(e: anyhow::Error) -> SnapshotError {
    SnapshotError::Db(format!("{e:#}"))
}

/// Frame: `"XSNP" ‖ version ‖ (u32 len ‖ id) x4 ‖ u32 body len ‖ body`.
fn encode_frame(
    ws: &str,
    project: &str,
    session: &str,
    cid: &str,
    body: &[u8],
) -> Result<Zeroizing<Vec<u8>>, SnapshotError> {
    if body.len() > MAX_BODY {
        return Err(SnapshotError::TooLarge);
    }
    let mut frame = Vec::with_capacity(MAX_INNER);
    frame.extend_from_slice(SNAPSHOT_MAGIC);
    frame.push(SNAPSHOT_FRAME_VERSION);
    for id in [ws, project, session, cid] {
        frame.extend_from_slice(&(id.len() as u32).to_be_bytes());
        frame.extend_from_slice(id.as_bytes());
    }
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(body);
    Ok(Zeroizing::new(frame))
}

/// Take `n` bytes from the front of `rest`, or refuse if fewer remain.
fn take_bytes<'a>(rest: &mut &'a [u8], n: usize) -> Result<&'a [u8], SnapshotError> {
    if n > rest.len() {
        return Err(SnapshotError::Tampered);
    }
    let (head, tail) = rest.split_at(n);
    *rest = tail;
    Ok(head)
}

fn take_u32(rest: &mut &[u8]) -> Result<usize, SnapshotError> {
    let b = take_bytes(rest, 4)?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

/// Read one header id: it must pass its rule and equal the expected id byte for byte.
fn read_header_id(
    rest: &mut &[u8],
    expected: &str,
    valid: fn(&str) -> bool,
) -> Result<(), SnapshotError> {
    let len = take_u32(rest)?;
    let text = std::str::from_utf8(take_bytes(rest, len)?).map_err(|_| SnapshotError::Tampered)?;
    if valid(text) && text == expected {
        Ok(())
    } else {
        Err(SnapshotError::Tampered)
    }
}

/// Strict parser for [`encode_frame`]. Returns the body as text. Anything else
/// is `Tampered`: a bad header, an id that is not the expected one, lengths
/// past the end, trailing bytes, a body that is not UTF-8 or not a message array.
fn parse_frame(
    frame: &[u8],
    ws: &str,
    project: &str,
    session: &str,
    cid: &str,
) -> Result<String, SnapshotError> {
    let mut rest = frame;
    if take_bytes(&mut rest, 4)? != SNAPSHOT_MAGIC.as_slice() {
        return Err(SnapshotError::Tampered);
    }
    if take_bytes(&mut rest, 1)?[0] != SNAPSHOT_FRAME_VERSION {
        return Err(SnapshotError::Tampered);
    }
    read_header_id(&mut rest, ws, is_snapshot_project_id)?;
    read_header_id(&mut rest, project, is_snapshot_project_id)?;
    read_header_id(&mut rest, session, |s| validate_session_id(s).is_ok())?;
    read_header_id(&mut rest, cid, is_canonical_cid)?;
    let body_len = take_u32(&mut rest)?;
    if body_len > MAX_BODY {
        return Err(SnapshotError::Tampered);
    }
    let body = take_bytes(&mut rest, body_len)?;
    if !rest.is_empty() {
        return Err(SnapshotError::Tampered);
    }
    let text = std::str::from_utf8(body).map_err(|_| SnapshotError::Tampered)?;
    serde_json::from_str::<Vec<SnapshotMessage>>(text).map_err(|_| SnapshotError::Tampered)?;
    Ok(text.to_string())
}

/// Seal a frame under the policy's tier. Node: `xsn1:` over the node AD.
/// Default space: `xr1:` over the record AD, bound to the checkpoint id.
fn seal_snapshot(
    policy: &SnapshotPolicy,
    scope: &SnapshotScope,
    cid: &str,
    frame: &[u8],
) -> Result<String, SnapshotError> {
    match policy {
        SnapshotPolicy::Node(key) => {
            let fields = keys::snapshot_node_ad_fields(
                &scope.workspace_id,
                &scope.project_id,
                &scope.session_id,
                cid,
            );
            let raw = keys::seal_with_ad(key, &fields, frame).map_err(db_err)?;
            Ok(format!(
                "{NODE_SNAPSHOT_PREFIX}{}",
                crate::crypto::hex_encode(raw)
            ))
        }
        SnapshotPolicy::Dek(handle) => handle
            .seal_record_bytes(DEFAULT_SPACE_ID, SNAPSHOT_DEK_KIND, cid, frame)
            .map_err(db_err),
    }
}

/// Open and verify a stored snapshot under the policy's tier only. Never
/// falls back to the other tier, and never uses `Sealer::open`.
fn open_snapshot(
    policy: &SnapshotPolicy,
    scope: &SnapshotScope,
    cid: &str,
    stored: &str,
) -> Result<String, SnapshotError> {
    let (ws, pj, sid) = (
        scope.workspace_id.as_str(),
        scope.project_id.as_str(),
        scope.session_id.as_str(),
    );
    match policy {
        SnapshotPolicy::Dek(handle) => {
            if stored.starts_with(NODE_SNAPSHOT_PREFIX) {
                return Err(SnapshotError::WeakerThanPolicy);
            }
            let Some(hex) = stored.strip_prefix(RECORD_PREFIX) else {
                return Err(SnapshotError::ClearUnderSealPolicy);
            };
            if stored.len() > MAX_STORED_DEK {
                return Err(SnapshotError::Oversized);
            }
            if !is_lower_hex(hex) || hex.len() / 2 < MIN_RAW {
                return Err(SnapshotError::Tampered);
            }
            let frame = handle
                .open_record_bytes(DEFAULT_SPACE_ID, SNAPSHOT_DEK_KIND, cid, stored)
                .map_err(|_| SnapshotError::Tampered)?;
            parse_frame(&frame, ws, pj, sid, cid)
        }
        SnapshotPolicy::Node(key) => {
            if stored.starts_with(RECORD_PREFIX) {
                return Err(SnapshotError::TierMismatch);
            }
            let Some(hex) = stored.strip_prefix(NODE_SNAPSHOT_PREFIX) else {
                return Err(SnapshotError::ClearUnderSealPolicy);
            };
            if stored.len() > MAX_STORED_NODE {
                return Err(SnapshotError::Oversized);
            }
            if !is_lower_hex(hex) || hex.len() / 2 < MIN_RAW {
                return Err(SnapshotError::Tampered);
            }
            let raw = crate::crypto::hex_decode(hex).map_err(|_| SnapshotError::Tampered)?;
            let fields = keys::snapshot_node_ad_fields(ws, pj, sid, cid);
            let frame =
                keys::open_with_ad(key, &fields, &raw).map_err(|_| SnapshotError::Tampered)?;
            parse_frame(&frame, ws, pj, sid, cid)
        }
    }
}

/// Snapshot bodies: read the session's messages in order, opened with the
/// policy's tier. `Err` is a snapshot-level refusal; the outer `Result` is
/// the database. Early exit once the running total passes the body bound.
fn read_snapshot_messages(
    tx: &rusqlite::Transaction<'_>,
    policy: &SnapshotPolicy,
    scope: &SnapshotScope,
) -> Result<Result<Vec<SnapshotMessage>, SnapshotError>> {
    let sealer = match policy {
        SnapshotPolicy::Node(key) => Sealer::Node(*key),
        SnapshotPolicy::Dek(handle) => Sealer::Dek(handle.clone()),
    };
    let mut stmt = tx.prepare(SQL_SNAPSHOT_MESSAGES)?;
    let mut rows = stmt.query(params![scope.session_id])?;
    let mut messages = Vec::new();
    let mut total = 0usize;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let role: String = row.get(1)?;
        let stored: String = row.get(2)?;
        let Ok(content) = sealer.open("conv.msg.content", &id, stored) else {
            return Ok(Err(SnapshotError::SourceUnreadable));
        };
        total += id.len() + role.len() + content.len();
        if total > MAX_BODY {
            return Ok(Err(SnapshotError::TooLarge));
        }
        messages.push(SnapshotMessage { id, role, content });
    }
    Ok(Ok(messages))
}

/// The capture transaction. Nothing is written unless every step passes.
/// Runs inside `with_conn`; the outer `Result` is the database.
fn capture_snapshot_in_tx(
    conn: &Connection,
    pool: &str,
    policy: &SnapshotPolicy,
    scope: &SnapshotScope,
    cid: &str,
    name: Option<&str>,
) -> Result<Result<(), SnapshotError>> {
    // Immediate: the write lock is taken here, so a concurrent writer waits.
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let member: Option<i64> = tx
        .query_row(
            SQL_SNAPSHOT_MEMBER,
            params![scope.session_id, scope.project_id],
            |r| r.get(0),
        )
        .optional()?;
    if member.is_none() {
        return Ok(Err(SnapshotError::NotMember));
    }
    let messages = match read_snapshot_messages(&tx, policy, scope)? {
        Ok(messages) => messages,
        Err(e) => return Ok(Err(e)),
    };
    if messages.is_empty() {
        return Ok(Err(SnapshotError::Empty));
    }
    let body = serde_json::to_vec(&messages)?;
    let frame = match encode_frame(
        &scope.workspace_id,
        &scope.project_id,
        &scope.session_id,
        cid,
        &body,
    ) {
        Ok(frame) => frame,
        Err(e) => return Ok(Err(e)),
    };
    let sealed = match seal_snapshot(policy, scope, cid, &frame) {
        Ok(sealed) => sealed,
        Err(e) => return Ok(Err(e)),
    };
    run_capture_hook(pool);
    tx.execute(
        "INSERT INTO checkpoints (id, project_id, workspace_id, session_id, task_id, name, state, created_at, kind)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, 'active', ?6, 'context_save')",
        params![
            cid,
            scope.project_id,
            scope.workspace_id,
            scope.session_id,
            name,
            Utc::now().to_rfc3339()
        ],
    )?;
    // Plain INSERT: a duplicate id fails and rolls back, never overwrites.
    tx.execute(
        "INSERT INTO checkpoint_snapshots (checkpoint_id, context_sealed) VALUES (?1, ?2)",
        params![cid, sealed],
    )?;
    tx.commit()?;
    Ok(Ok(()))
}

#[cfg(test)]
type CaptureHook = Box<dyn FnOnce() + Send>;

/// Test hooks run inside the capture transaction, keyed by pool id.
#[cfg(test)]
static CAPTURE_HOOKS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, CaptureHook>>,
> = std::sync::LazyLock::new(Default::default);

/// Count of stored snapshot values fetched into Rust (tests check that
/// oversized values are never fetched).
#[cfg(test)]
static SNAPSHOT_VALUE_FETCHES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Runs (and removes) the hook for `pool_id`. The map lock is released before
/// the hook runs, so a panicking hook cannot poison it.
#[cfg(test)]
fn run_capture_hook(pool_id: &str) {
    let hook = CAPTURE_HOOKS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(pool_id);
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(not(test))]
fn run_capture_hook(_pool_id: &str) {}

#[cfg(test)]
fn note_value_fetch() {
    SNAPSHOT_VALUE_FETCHES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(not(test))]
fn note_value_fetch() {}

impl ConversationsDb {
    /// Resolve the sealing policy. Never goes through `sealer()`, which maps a
    /// missing key to clear writes. Order: explicit key, then the default-space
    /// keystore (when one exists), then the node record key.
    fn snapshot_policy(&self) -> Result<SnapshotPolicy, SnapshotError> {
        use crate::memory::sqlite_vec_store::at_rest;
        if let Some(key) = self.node_key {
            return Ok(SnapshotPolicy::Node(key));
        }
        if at_rest::default_keystore_exists() {
            return at_rest::default_space_key()
                .map(SnapshotPolicy::Dek)
                .map_err(|_| SnapshotError::KeyUnavailable);
        }
        match at_rest::peek_record_key_with_source() {
            (Some(key), _) => Ok(SnapshotPolicy::Node(key)),
            (None, at_rest::KeySource::Unavailable(_)) => Err(SnapshotError::KeyUnavailable),
            (None, _) => Err(SnapshotError::NoSealKey),
        }
    }

    /// Create the snapshot table on first use. Never part of `create_schema`,
    /// so a database that never takes a snapshot gets no new table.
    async fn ensure_snapshot_schema(&self) -> Result<(), SnapshotError> {
        self.snapshot_schema_initialized
            .get_or_try_init(|| async {
                ConnectionManager::global()
                    .with_conn(&self.full_project_id, |conn| {
                        conn.execute_batch(SQL_SNAPSHOT_SCHEMA)
                            .context("failed to create snapshot schema")?;
                        Ok(())
                    })
                    .await
            })
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// Checkpoint `scope`'s session with a sealed transcript snapshot. All or nothing.
    pub async fn create_checkpoint_with_snapshot(
        &self,
        scope: &SnapshotScope,
        name: Option<&str>,
    ) -> Result<SavedSnapshot, SnapshotError> {
        if self.disabled {
            return Err(SnapshotError::Disabled);
        }
        if scope.project_id != self.project_id {
            return Err(SnapshotError::ScopeMismatch);
        }
        let policy = self.snapshot_policy()?;
        let tier = policy.tier();
        let cid = Uuid::new_v4().to_string();
        self.ensure_schema().await.map_err(db_err)?;
        self.ensure_snapshot_schema().await?;

        let (scope, name) = (scope.clone(), name.map(str::to_string));
        let (pool, cid_c) = (self.full_project_id.clone(), cid.clone());
        ConnectionManager::global()
            .with_conn(&self.full_project_id, move |conn| {
                capture_snapshot_in_tx(conn, &pool, &policy, &scope, &cid_c, name.as_deref())
            })
            .await
            .map_err(db_err)??;
        Ok(SavedSnapshot {
            checkpoint_id: cid,
            tier,
        })
    }

    /// Restore a checkpoint's sealed transcript. The row only selects the
    /// candidate; authority comes from the scope and the AD. No live thread is needed.
    pub async fn load_checkpoint_snapshot(
        &self,
        scope: &SnapshotScope,
        checkpoint_id: &str,
    ) -> Result<LoadedSnapshot, SnapshotError> {
        if self.disabled {
            return Err(SnapshotError::Disabled);
        }
        if scope.project_id != self.project_id {
            return Err(SnapshotError::ScopeMismatch);
        }
        if !is_canonical_cid(checkpoint_id) {
            return Err(SnapshotError::InvalidScope);
        }
        let policy = self.snapshot_policy()?;
        self.ensure_schema().await.map_err(db_err)?;
        self.ensure_snapshot_schema().await?;

        let (ws, pj, sid) = (
            scope.workspace_id.clone(),
            scope.project_id.clone(),
            scope.session_id.clone(),
        );
        let cid = checkpoint_id.to_string();
        let row = ConnectionManager::global()
            .with_conn(&self.full_project_id, move |conn| {
                let row = conn
                    .query_row(
                        SQL_SNAPSHOT_LOAD,
                        params![cid, pj, ws, sid, MAX_STORED_NODE as i64],
                        |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
                    )
                    .optional()?;
                if matches!(row, Some((_, Some(_)))) {
                    note_value_fetch();
                }
                Ok(row)
            })
            .await
            .map_err(db_err)?;

        let Some((type_name, value)) = row else {
            return Err(SnapshotError::Missing);
        };
        if type_name != "text" {
            return Err(SnapshotError::Tampered);
        }
        let stored = value.ok_or(SnapshotError::Oversized)?;
        let body = open_snapshot(&policy, scope, checkpoint_id, &stored)?;
        Ok(LoadedSnapshot {
            body,
            tier: policy.tier(),
        })
    }
}

/// Parse an RFC 3339 datetime string.
fn parse_datetime(s: &str) -> DateTime<Utc> {
    s.parse::<DateTime<Utc>>().unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_open_with_malicious_project_id() {
        let mal_ids = vec!["../etc/passwd", "my/project", "project\\name", "~", " ", ""];
        for id in mal_ids {
            let res = ConversationsDb::open(id).await;
            assert!(
                res.is_err(),
                "project_id '{}' should have been rejected",
                id
            );
        }
    }

    #[tokio::test]
    async fn test_open_in_memory_with_malicious_project_id() {
        let mal_ids = vec!["../etc/passwd", "my/project", "project\\name", "~", " ", ""];
        for id in mal_ids {
            let res = ConversationsDb::open_in_memory(id).await;
            assert!(
                res.is_err(),
                "project_id '{}' should have been rejected",
                id
            );
        }
    }

    #[test]
    fn test_conversations_db_path_with_malicious_project_id() {
        let mal_ids = vec!["../etc/passwd", "my/project", "project\\name", "~", " ", ""];
        for id in mal_ids {
            let res = std::panic::catch_unwind(|| ConversationsDb::conversations_db_path(id));
            assert!(
                res.is_err(),
                "conversations_db_path should have panicked for project_id '{}'",
                id
            );
        }
    }

    #[tokio::test]
    async fn test_open_valid_project_id() {
        let db = ConversationsDb::open_in_memory("valid-project_123")
            .await
            .unwrap();
        assert_eq!(db.project_id(), "valid-project_123");
    }

    #[tokio::test]
    async fn test_thread_and_message_crud() {
        let db = ConversationsDb::open_in_memory("test-crud").await.unwrap();
        db.create_schema().await.unwrap();

        // Create thread
        let thread = db
            .create_thread(Some("Test Title"), Some("gpt-4"), Some("unit-test"))
            .await
            .unwrap();
        assert_eq!(thread.title, Some("Test Title".to_string()));

        // Get thread
        let thread_get = db.get_thread(&thread.id).await.unwrap().unwrap();
        assert_eq!(thread_get.id, thread.id);

        // List threads
        let threads = db.list_threads(10).await.unwrap();
        assert!(!threads.is_empty());

        // Store message
        let msg = db
            .store_message(
                &thread.id,
                "user",
                "Hello world",
                None,
                None,
                None,
                Some("{\"key\": \"val\"}"),
                Some(10),
            )
            .await
            .unwrap();
        assert_eq!(msg.content, "Hello world");
        assert_eq!(msg.role, "user");

        // Get messages
        let messages = db.get_thread_messages(&thread.id).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "Hello world");

        // Test updated_at and last_preview on thread
        let thread_updated = db.get_thread(&thread.id).await.unwrap().unwrap();
        assert!(thread_updated.last_preview.is_some());
        let last_preview_val = thread_updated.last_preview.unwrap();
        assert!(last_preview_val.contains("### Extractive Session Summary"));
        assert!(last_preview_val.contains("Hello world"));
    }

    #[tokio::test]
    async fn test_delete_thread_existing() {
        let db = ConversationsDb::open_in_memory("test-delete-existing")
            .await
            .unwrap();
        let thread = db.create_thread(None, None, None).await.unwrap();
        let retained = db.create_thread(None, None, None).await.unwrap();
        for thread_id in [&thread.id, &retained.id] {
            db.store_message(thread_id, "user", "Hello", None, None, None, None, None)
                .await
                .unwrap();
        }

        assert!(db.delete_thread(&thread.id).await.unwrap());
        assert!(db.get_thread(&thread.id).await.unwrap().is_none());
        assert!(db.get_thread_messages(&thread.id).await.unwrap().is_empty());
        assert!(db.get_thread(&retained.id).await.unwrap().is_some());
        assert_eq!(db.get_thread_messages(&retained.id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_delete_thread_unknown() {
        let db = ConversationsDb::open_in_memory("test-delete-unknown")
            .await
            .unwrap();
        assert!(!db.delete_thread("unknown-thread").await.unwrap());
    }

    #[tokio::test]
    async fn test_delete_thread_respects_project_scope() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let owner = ConversationsDb::open_at("delete-owner", &path)
            .await
            .unwrap();
        let other = ConversationsDb::open_at("delete-other", &path)
            .await
            .unwrap();
        let thread = owner.create_thread(None, None, None).await.unwrap();
        owner
            .store_message(&thread.id, "user", "Private", None, None, None, None, None)
            .await
            .unwrap();

        assert!(!other.delete_thread(&thread.id).await.unwrap());
        assert!(owner.get_thread(&thread.id).await.unwrap().is_some());
        assert_eq!(
            owner.get_thread_messages(&thread.id).await.unwrap().len(),
            1
        );
        assert!(owner.delete_thread(&thread.id).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete_thread_rolls_back_messages_on_failure() {
        let db = ConversationsDb::open_in_memory("test-delete-rollback")
            .await
            .unwrap();
        let thread = db.create_thread(None, None, None).await.unwrap();
        db.store_message(&thread.id, "user", "Keep me", None, None, None, None, None)
            .await
            .unwrap();
        ConnectionManager::global()
            .with_conn(&db.full_project_id, |conn| {
                conn.execute_batch(
                    "CREATE TRIGGER reject_thread_delete BEFORE DELETE ON conversation_threads
                     BEGIN SELECT RAISE(ABORT, 'deletion rejected'); END;",
                )?;
                Ok(())
            })
            .await
            .unwrap();

        assert!(db.delete_thread(&thread.id).await.is_err());
        assert!(db.get_thread(&thread.id).await.unwrap().is_some());
        assert_eq!(db.get_thread_messages(&thread.id).await.unwrap().len(), 1);
    }

    const CANARY: &str = "zq-canary-7f3a91";

    fn raw_bytes(db_path: &Path) -> Vec<u8> {
        let mut all = std::fs::read(db_path).unwrap_or_default();
        for ext in ["-wal", "-shm"] {
            let mut p = db_path.as_os_str().to_owned();
            p.push(ext);
            all.extend(std::fs::read(std::path::PathBuf::from(p)).unwrap_or_default());
        }
        all
    }

    #[tokio::test]
    async fn test_conversations_db_encrypted_at_rest_adversary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = ConversationsDb::open_at("enc-adversary", &path)
            .await
            .unwrap()
            .with_node_key([0x5A; 32]);
        let thread = db
            .create_thread(Some(&format!("title {CANARY}")), None, None)
            .await
            .unwrap();
        db.store_message(
            &thread.id,
            "user",
            &format!("secret message {CANARY}"),
            Some(&format!("tool {CANARY}")),
            None,
            None,
            Some(&format!("{{\"k\":\"{CANARY}\"}}")),
            None,
        )
        .await
        .unwrap();
        db.record_deduction(&format!("deduction {CANARY}"), 0.5, None, None)
            .await
            .unwrap();
        let belief = db
            .upsert_belief(
                &format!("subject {CANARY}"),
                Some("rel"),
                Some(&format!("obj {CANARY}")),
                Some(&format!("prop {CANARY}")),
                0.9,
                1.0,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        // Upsert of the same belief matches the sealed row (no duplicate).
        let again = db
            .upsert_belief(
                &format!("subject {CANARY}"),
                Some("rel"),
                Some(&format!("obj {CANARY}")),
                None,
                0.8,
                1.0,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(again.id, belief.id);
        assert_eq!(db.list_beliefs(10).await.unwrap().len(), 1);

        // Reads return plaintext.
        let msgs = db.get_thread_messages(&thread.id).await.unwrap();
        assert_eq!(msgs[0].content, format!("secret message {CANARY}"));
        assert_eq!(
            db.get_thread(&thread.id).await.unwrap().unwrap().title,
            Some(format!("title {CANARY}"))
        );
        assert_eq!(
            db.list_deductions(5).await.unwrap()[0].deduction,
            format!("deduction {CANARY}")
        );

        // ADVERSARY: the raw sqlite file must not contain the marker.
        let raw = raw_bytes(&path);
        assert!(!raw.is_empty());
        assert!(
            raw.windows(CANARY.len()).all(|w| w != CANARY.as_bytes()),
            "ADVERSARY FAIL: plaintext marker found in {}",
            path.display()
        );
    }

    #[tokio::test]
    async fn test_conversations_db_reads_legacy_clear_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = ConversationsDb::open_at("enc-legacy", &path)
            .await
            .unwrap()
            .with_node_key([0x5A; 32]);
        db.create_schema().await.unwrap();
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute(
                "INSERT INTO conversation_threads (id, project_id, title, started_at, updated_at)
                 VALUES ('t1', 'enc-legacy', 'legacy title', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO conversation_messages (id, thread_id, role, content, created_at)
                 VALUES ('m1', 't1', 'user', 'legacy clear message', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO beliefs (id, project_id, subject, proposition, created_at, updated_at)
                 VALUES ('b1', 'enc-legacy', 'legacy subject', 'legacy prop', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        }
        let msgs = db.get_thread_messages("t1").await.unwrap();
        assert_eq!(msgs[0].content, "legacy clear message");
        assert_eq!(
            db.get_thread("t1").await.unwrap().unwrap().title.as_deref(),
            Some("legacy title")
        );
        let b = db.get_belief("b1").await.unwrap().unwrap();
        assert_eq!(b.subject, "legacy subject");
        assert_eq!(b.proposition.as_deref(), Some("legacy prop"));

        // Updating a legacy belief matches it and re-seals it.
        let upd = db
            .upsert_belief(
                "legacy subject",
                None,
                None,
                Some("legacy prop"),
                0.7,
                1.0,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(upd.id, "b1");
        let conn = rusqlite::Connection::open(&path).unwrap();
        let stored: String = conn
            .query_row("SELECT subject FROM beliefs WHERE id = 'b1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(stored.starts_with("xcn1:"), "legacy row was not re-sealed");
    }

    /// `EXPLAIN QUERY PLAN` detail lines for `sql` on the live schema.
    async fn query_plan(db: &ConversationsDb, sql: &'static str) -> Vec<String> {
        ConnectionManager::global()
            .with_conn(&db.full_project_id, move |conn| {
                let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
                // The plan does not depend on bound values; bind NULLs to fill ?1..?n.
                let nulls = vec![rusqlite::types::Value::Null; stmt.parameter_count()];
                let details = stmt
                    .query_map(rusqlite::params_from_iter(nulls), |row| {
                        row.get::<_, String>(3)
                    })?
                    .collect::<rusqlite::Result<Vec<String>>>()?;
                Ok(details)
            })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn conversation_queries_use_indexes() {
        let db = ConversationsDb::open_in_memory("idx-plan").await.unwrap();
        db.create_schema().await.unwrap();

        // (label, shared SQL, tables that must be searched by index, ORDER BY query?)
        let cases: [(&str, &'static str, &[&str], bool); 4] = [
            (
                "thread messages",
                SQL_THREAD_MESSAGES,
                &["conversation_messages"],
                true,
            ),
            (
                "active checkpoints",
                SQL_CHECKPOINTS_ACTIVE,
                &["checkpoints"],
                true,
            ),
            (
                "session exists",
                SQL_SESSION_EXISTS,
                &[
                    "conversation_threads",
                    "conversation_messages",
                    "checkpoints",
                ],
                false,
            ),
            (
                "deductions",
                SQL_DEDUCTIONS_BY_PROJECT,
                &["deductions"],
                true,
            ),
        ];
        for (label, sql, tables, ordered) in cases {
            let plan = query_plan(&db, sql).await;
            for table in tables {
                let scan = format!("SCAN {table}");
                assert!(
                    !plan.iter().any(|line| line.starts_with(&scan)),
                    "{label}: full table scan of {table}; plan = {plan:?}"
                );
                assert!(
                    plan.iter().any(|line| {
                        line.contains(table)
                            && (line.contains("USING INDEX")
                                || line.contains("USING COVERING INDEX"))
                    }),
                    "{label}: {table} is not searched by an index; plan = {plan:?}"
                );
            }
            if ordered {
                assert!(
                    !plan.iter().any(|line| line.contains("TEMP B-TREE")),
                    "{label}: ORDER BY needs a temp sort; plan = {plan:?}"
                );
            }
        }
    }

    #[tokio::test]
    async fn index_migration_is_idempotent_on_existing_db() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        {
            // A database written before the indexes existed: tables and rows only.
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE conversation_threads (
                    id TEXT PRIMARY KEY, project_id TEXT, title TEXT,
                    started_at TEXT NOT NULL, updated_at TEXT NOT NULL,
                    last_preview TEXT, model TEXT, source TEXT);
                 CREATE TABLE conversation_messages (
                    id TEXT PRIMARY KEY, thread_id TEXT NOT NULL, role TEXT NOT NULL,
                    content TEXT NOT NULL, tool_calls TEXT, openui_lang TEXT, xui_json TEXT,
                    metadata TEXT, created_at TEXT NOT NULL, tokens INTEGER,
                    FOREIGN KEY (thread_id) REFERENCES conversation_threads(id));
                 CREATE TABLE deductions (
                    id TEXT PRIMARY KEY, project_id TEXT, source_thread TEXT,
                    deduction TEXT NOT NULL, confidence REAL DEFAULT 0.0,
                    created_at TEXT NOT NULL, last_accessed TEXT, category TEXT);
                 CREATE TABLE checkpoints (
                    id TEXT PRIMARY KEY, project_id TEXT, workspace_id TEXT,
                    session_id TEXT, task_id TEXT, name TEXT, state TEXT NOT NULL,
                    created_at TEXT NOT NULL, expires_at TEXT, kind TEXT DEFAULT 'work');
                 INSERT INTO conversation_threads (id, project_id, title, started_at, updated_at)
                    VALUES ('t1', 'idx-migrate', 'old', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO conversation_messages (id, thread_id, role, content, created_at)
                    VALUES ('m1', 't1', 'user', 'old one', '2026-01-01T00:00:00Z'),
                           ('m2', 't1', 'assistant', 'old two', '2026-01-01T00:00:01Z');
                 INSERT INTO deductions (id, project_id, deduction, created_at)
                    VALUES ('d1', 'idx-migrate', 'old deduction', '2026-01-01T00:00:00Z');
                 INSERT INTO checkpoints (id, project_id, session_id, name, state, created_at)
                    VALUES ('c1', 'idx-migrate', 's1', 'old checkpoint', 'active', '2026-01-01T00:00:00Z');",
            )
            .unwrap();
        }

        // First open migrates the file; a second create_schema and a reopen must be no-ops.
        let db = ConversationsDb::open_at("idx-migrate", &path)
            .await
            .unwrap();
        db.create_schema().await.unwrap();
        db.create_schema().await.unwrap();
        drop(db);
        let db = ConversationsDb::open_at("idx-migrate", &path)
            .await
            .unwrap();
        db.create_schema().await.unwrap();
        drop(db);

        let conn = rusqlite::Connection::open(&path).unwrap();
        for name in [
            "idx_conversation_messages_thread_created",
            "idx_checkpoints_project_state_created",
            "idx_checkpoints_session_project",
            "idx_deductions_project_created",
        ] {
            let found: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
                    params![name],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(found, 1, "index {name} missing after migration");
        }

        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT COUNT(*) FROM conversation_threads"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM conversation_messages"), 2);
        assert_eq!(count("SELECT COUNT(*) FROM deductions"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM checkpoints"), 1);
        let content: String = conn
            .query_row(
                "SELECT content FROM conversation_messages WHERE id = 'm2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(content, "old two");
    }
}

#[cfg(test)]
mod snapshot_tests;
