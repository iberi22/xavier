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
    /// SHA-256 digests of the node keys whose seal floor this handle has verified.
    floor_verified: std::sync::Mutex<Vec<[u8; 32]>>,
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
            floor_verified: std::sync::Mutex::new(Vec::new()),
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
            floor_verified: std::sync::Mutex::new(Vec::new()),
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
            floor_verified: std::sync::Mutex::new(Vec::new()),
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
            floor_verified: std::sync::Mutex::new(Vec::new()),
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
        match at_rest::peek_record_key_with_source() {
            (Some(k), _) => Ok(Sealer::Node(k)),
            (None, at_rest::KeySource::Unavailable(reason)) => {
                let key_path = at_rest::record_key_path();
                let env_var = at_rest::RECORD_KEY_ENV;
                Ok(Sealer::Unavailable(format!(
                    "at-rest encryption key is unavailable: {reason}. Key is expected at '{}' or via environment variable '{env_var}'",
                    key_path.display()
                )))
            }
            (None, _) => Ok(Sealer::None),
        }
    }

    /// The sealer for one operation. Node-tier operations verify the seal floor
    /// first; a handle verifies each node key once.
    async fn ready_sealer(&self) -> Result<Sealer> {
        use sha2::{Digest, Sha256};
        let sealer = self.sealer()?;
        if let Sealer::Node(key) = &sealer {
            let fingerprint: [u8; 32] = Sha256::digest(key).into();
            let verified = self
                .floor_verified
                .lock()
                .is_ok_and(|v| v.contains(&fingerprint));
            if !verified {
                let key = *key;
                let project = self.project_id.clone();
                ConnectionManager::global()
                    .with_conn(&self.full_project_id, move |conn| {
                        ensure_seal_floor(conn, &key, &project)
                    })
                    .await?;
                if let Ok(mut v) = self.floor_verified.lock() {
                    v.push(fingerprint);
                }
            }
        }
        Ok(sealer)
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

                CREATE TABLE IF NOT EXISTS conv_seal_state (
                    k TEXT PRIMARY KEY,
                    v TEXT NOT NULL
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
        let sealer = self.ready_sealer().await?;
        let title_c = sealer.seal_opt(
            "conv.thread.title",
            FieldCtx {
                project: &self.project_id,
                parent: "",
                row: &id,
            },
            title,
        )?;
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
        let sealer = self.ready_sealer().await?;
        let pid = self.project_id.clone();
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
                        title: sealer.open_opt(
                            "conv.thread.title",
                            FieldCtx {
                                project: &pid,
                                parent: "",
                                row: &id,
                            },
                            row.get(2)?,
                        )?,
                        started_at: parse_datetime(&row.get::<_, String>(3)?),
                        updated_at: parse_datetime(&row.get::<_, String>(4)?),
                        last_preview: sealer.open_opt(
                            "conv.thread.preview",
                            FieldCtx {
                                project: &pid,
                                parent: "",
                                row: &id,
                            },
                            row.get(5)?,
                        )?,
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
        let sealer = self.ready_sealer().await?;
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
                        title: sealer.open_opt(
                            "conv.thread.title",
                            FieldCtx {
                                project: &pid,
                                parent: "",
                                row: &id,
                            },
                            row.get(2)?,
                        )?,
                        started_at: parse_datetime(&row.get::<_, String>(3)?),
                        updated_at: parse_datetime(&row.get::<_, String>(4)?),
                        last_preview: sealer.open_opt(
                            "conv.thread.preview",
                            FieldCtx {
                                project: &pid,
                                parent: "",
                                row: &id,
                            },
                            row.get(5)?,
                        )?,
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
        let sealer = self.ready_sealer().await?;
        let ctx = FieldCtx {
            project: &self.project_id,
            parent: thread_id,
            row: &id,
        };
        let content_c = sealer.seal("conv.msg.content", ctx, content)?;
        let tool_calls_c = sealer.seal_opt("conv.msg.tool_calls", ctx, tool_calls)?;
        let openui_lang_c = sealer.seal_opt("conv.msg.openui_lang", ctx, openui_lang)?;
        let xui_json_c = sealer.seal_opt("conv.msg.xui_json", ctx, xui_json)?;
        let metadata_c = sealer.seal_opt("conv.msg.metadata", ctx, metadata)?;

        let id_for_return = id.clone();
        let now_c = now.clone();
        let thread_id_for_update = thread_id.to_string();
        let pid = self.project_id.clone();

        ConnectionManager::global().with_conn(&self.full_project_id, move |conn| {
            let thread_belongs: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM conversation_threads WHERE id = ?1 AND project_id = ?2)",
                    params![thread_id_c, pid],
                    |r| r.get(0),
                )
                .context("failed to check thread project")?;
            if !thread_belongs {
                anyhow::bail!("thread {thread_id_c} does not belong to project {pid}");
            }

            conn.execute(
                "INSERT INTO conversation_messages (id, thread_id, role, content, tool_calls, openui_lang, xui_json, metadata, created_at, tokens)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![id, thread_id_c, role_c, content_c, tool_calls_c, openui_lang_c, xui_json_c, metadata_c, now_c.clone(), tokens],
            ).context("failed to store message")?;

            // Update thread's updated_at timestamp
            let _ = conn.execute(
                "UPDATE conversation_threads SET updated_at = ?1 WHERE id = ?2 AND project_id = ?3",
                params![now_c, thread_id_for_update, pid],
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
        let sealer = self.ready_sealer().await?;
        let preview_c = sealer.seal(
            "conv.thread.preview",
            FieldCtx {
                project: &self.project_id,
                parent: "",
                row: thread_id,
            },
            preview,
        )?;
        let now_c = Utc::now().to_rfc3339();
        let pid = self.project_id.clone();

        ConnectionManager::global()
            .with_conn(&self.full_project_id, move |conn| {
                let updated = conn.execute(
                    "UPDATE conversation_threads SET updated_at = ?1, last_preview = ?2 WHERE id = ?3 AND project_id = ?4",
                    params![now_c, preview_c, thread_id_c, pid],
                ).context("failed to update thread preview")?;
                if updated == 0 {
                    anyhow::bail!("thread {thread_id_c} does not belong to project {pid}");
                }
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Get all messages for a thread, ordered by creation time.
    pub async fn get_thread_messages(&self, thread_id: &str) -> Result<Vec<Message>> {
        self.ensure_schema().await?;
        let tid = thread_id.to_string();
        let sealer = self.ready_sealer().await?;
        let pid = self.project_id.clone();
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
                        content: sealer.open(
                            "conv.msg.content",
                            FieldCtx {
                                project: &pid,
                                parent: &tid,
                                row: &id,
                            },
                            row.get(3)?,
                        )?,
                        tool_calls: sealer.open_opt(
                            "conv.msg.tool_calls",
                            FieldCtx {
                                project: &pid,
                                parent: &tid,
                                row: &id,
                            },
                            row.get(4)?,
                        )?,
                        openui_lang: sealer.open_opt(
                            "conv.msg.openui_lang",
                            FieldCtx {
                                project: &pid,
                                parent: &tid,
                                row: &id,
                            },
                            row.get(5)?,
                        )?,
                        xui_json: sealer.open_opt(
                            "conv.msg.xui_json",
                            FieldCtx {
                                project: &pid,
                                parent: &tid,
                                row: &id,
                            },
                            row.get(6)?,
                        )?,
                        metadata: sealer.open_opt(
                            "conv.msg.metadata",
                            FieldCtx {
                                project: &pid,
                                parent: &tid,
                                row: &id,
                            },
                            row.get(7)?,
                        )?,
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
        let sealer = self.ready_sealer().await?;
        let deduction_c = sealer.seal(
            "conv.deduction.text",
            FieldCtx {
                project: &self.project_id,
                parent: "",
                row: &id,
            },
            deduction_text,
        )?;
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
        let sealer = self.ready_sealer().await?;
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
                        deduction: sealer.open(
                            "conv.deduction.text",
                            FieldCtx {
                                project: &pid,
                                parent: "",
                                row: &id,
                            },
                            row.get(3)?,
                        )?,
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
        let sealer = self.ready_sealer().await?;
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
                    let subj = sealer.open("conv.belief.subject", FieldCtx { project: &pid, parent: "", row: &rid }, row.get(1)?)?;
                    if subj != subject_c {
                        continue;
                    }
                    let matched = if let (Some(rel), Some(obj)) = (&relation_c, &object_c) {
                        sealer.open_opt("conv.belief.relation", FieldCtx { project: &pid, parent: "", row: &rid }, row.get(2)?)?.as_deref() == Some(rel.as_str())
                            && sealer.open_opt("conv.belief.object", FieldCtx { project: &pid, parent: "", row: &rid }, row.get(3)?)?.as_deref() == Some(obj.as_str())
                    } else if let Some(prop) = &proposition_c {
                        sealer.open_opt("conv.belief.proposition", FieldCtx { project: &pid, parent: "", row: &rid }, row.get(4)?)?.as_deref() == Some(prop.as_str())
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
                    let subject_sealed = sealer.seal("conv.belief.subject", FieldCtx { project: &pid, parent: "", row: &id }, &subject_c)?;
                    let relation_c = sealer.seal_opt("conv.belief.relation", FieldCtx { project: &pid, parent: "", row: &id }, relation_c.as_deref())?;
                    let object_c = sealer.seal_opt("conv.belief.object", FieldCtx { project: &pid, parent: "", row: &id }, object_c.as_deref())?;
                    let proposition_c = sealer.seal_opt("conv.belief.proposition", FieldCtx { project: &pid, parent: "", row: &id }, proposition_c.as_deref())?;
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
                    let subject_c = sealer.seal("conv.belief.subject", FieldCtx { project: &pid, parent: "", row: &new_id }, &subject_c)?;
                    let relation_c = sealer.seal_opt("conv.belief.relation", FieldCtx { project: &pid, parent: "", row: &new_id }, relation_c.as_deref())?;
                    let object_c = sealer.seal_opt("conv.belief.object", FieldCtx { project: &pid, parent: "", row: &new_id }, object_c.as_deref())?;
                    let proposition_c = sealer.seal_opt("conv.belief.proposition", FieldCtx { project: &pid, parent: "", row: &new_id }, proposition_c.as_deref())?;
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
        let sealer = self.ready_sealer().await?;
        let pid = self.project_id.clone();
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
                    subject: sealer.open("conv.belief.subject", FieldCtx { project: &pid, parent: "", row: &rid }, row.get(2)?)?,
                    relation: sealer.open_opt("conv.belief.relation", FieldCtx { project: &pid, parent: "", row: &rid }, row.get(3)?)?,
                    object: sealer.open_opt("conv.belief.object", FieldCtx { project: &pid, parent: "", row: &rid }, row.get(4)?)?,
                    proposition: sealer.open_opt("conv.belief.proposition", FieldCtx { project: &pid, parent: "", row: &rid }, row.get(5)?)?,
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
        let sealer = self.ready_sealer().await?;
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
                    subject: sealer.open("conv.belief.subject", FieldCtx { project: &pid, parent: "", row: &rid }, row.get(2)?)?,
                    relation: sealer.open_opt("conv.belief.relation", FieldCtx { project: &pid, parent: "", row: &rid }, row.get(3)?)?,
                    object: sealer.open_opt("conv.belief.object", FieldCtx { project: &pid, parent: "", row: &rid }, row.get(4)?)?,
                    proposition: sealer.open_opt("conv.belief.proposition", FieldCtx { project: &pid, parent: "", row: &rid }, row.get(5)?)?,
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
            let sealer = self.ready_sealer().await?;
            let title_c = sealer.seal_opt(
                "conv.thread.title",
                FieldCtx {
                    project: &pid,
                    parent: "",
                    row: &id,
                },
                title.as_deref(),
            )?;
            let msg_project = pid.clone();
            let started_at_c = started_at.clone();
            let updated_at_c = updated_at.clone();
            let last_preview_c = sealer.seal_opt(
                "conv.thread.preview",
                FieldCtx {
                    project: &pid,
                    parent: "",
                    row: &id,
                },
                last_preview.as_deref(),
            )?;
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
                        let ctx = FieldCtx {
                            project: &msg_project,
                            parent: &id_c,
                            row: &msg_id,
                        };
                        let mcontent = sealer.seal("conv.msg.content", ctx, mcontent)?;
                        let xui_json = sealer.seal_opt("conv.msg.xui_json", ctx, xui_json)?;
                        let openui_lang = sealer.seal_opt("conv.msg.openui_lang", ctx, openui_lang)?;
                        let metadata = sealer.seal("conv.msg.metadata", ctx, &metadata)?;

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
            floor_verified: std::sync::Mutex::new(Vec::new()),
        }
    }
}

// ---------------------------------------------------------------------------
// Encryption at rest
// ---------------------------------------------------------------------------

/// Node-tier envelope: `xcn2:` + hex(nonce ‖ ct ‖ tag). The AD binds the row context.
const NODE_SEAL_PREFIX: &str = "xcn2:";
/// Node-tier envelope from before row binding (no AD). Read only by the seal floor.
const LEGACY_NODE_SEAL_PREFIX: &str = "xcn1:";
const FIELD_AD_LABEL: &str = "xavier-conv-field";
const FIELD_AD_VERSION: &str = "2";
/// Kind and value of the seal floor marker (`conv_seal_state`, key `floor`).
const FLOOR_KIND: &str = "conv.meta.floor";
const FLOOR_VALUE: &str = "2";
/// True when any sealed column of any table already holds an `xcn2:` value.
const SQL_ANY_NODE_SEALED: &str = "SELECT
    EXISTS(SELECT 1 FROM conversation_threads WHERE title GLOB 'xcn2:*' OR last_preview GLOB 'xcn2:*')
    OR EXISTS(SELECT 1 FROM conversation_messages WHERE content GLOB 'xcn2:*' OR tool_calls GLOB 'xcn2:*'
        OR openui_lang GLOB 'xcn2:*' OR xui_json GLOB 'xcn2:*' OR metadata GLOB 'xcn2:*')
    OR EXISTS(SELECT 1 FROM deductions WHERE deduction GLOB 'xcn2:*')
    OR EXISTS(SELECT 1 FROM beliefs WHERE subject GLOB 'xcn2:*' OR relation GLOB 'xcn2:*'
        OR object GLOB 'xcn2:*' OR proposition GLOB 'xcn2:*')";

/// Row context bound into every node-tier seal. Fixed for the life of a row:
/// the handle's trusted project, the parent row (the thread for messages, empty
/// for top-level rows) and the row id.
#[derive(Clone, Copy)]
struct FieldCtx<'a> {
    project: &'a str,
    parent: &'a str,
    row: &'a str,
}

/// Key material used to seal text columns.
#[derive(Clone)]
enum Sealer {
    /// Default-space data key (`xr1:` envelope, AD binds column kind + row id).
    Dek(crate::espacio::keys::KeyHandle),
    /// Node record key (`xcn2:` envelope), used before a keystore exists.
    Node([u8; 32]),
    /// Key source is unavailable: writes must fail closed, clear legacy reads work.
    Unavailable(String),
    /// No key available: rows are written as before (clear).
    None,
}

fn is_legacy_node_sealed(v: &str) -> bool {
    v.starts_with(LEGACY_NODE_SEAL_PREFIX)
}

fn kind_to_table_column(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "conv.thread.title" => ("conversation_threads", "title"),
        "conv.thread.preview" => ("conversation_threads", "last_preview"),
        "conv.msg.content" => ("conversation_messages", "content"),
        "conv.msg.tool_calls" => ("conversation_messages", "tool_calls"),
        "conv.msg.openui_lang" => ("conversation_messages", "openui_lang"),
        "conv.msg.xui_json" => ("conversation_messages", "xui_json"),
        "conv.msg.metadata" => ("conversation_messages", "metadata"),
        "conv.deduction.text" => ("deductions", "deduction"),
        "conv.belief.subject" => ("beliefs", "subject"),
        "conv.belief.relation" => ("beliefs", "relation"),
        "conv.belief.object" => ("beliefs", "object"),
        "conv.belief.proposition" => ("beliefs", "proposition"),
        _ => ("conversations", "field"),
    }
}

/// Associated data fields of a node-tier field: label, version, tier, project,
/// column kind, parent and row id. Framed by `keys::seal_with_ad`.
fn field_ad_fields<'a>(kind: &'a str, ctx: FieldCtx<'a>) -> [&'a [u8]; 7] {
    [
        FIELD_AD_LABEL.as_bytes(),
        FIELD_AD_VERSION.as_bytes(),
        b"node",
        ctx.project.as_bytes(),
        kind.as_bytes(),
        ctx.parent.as_bytes(),
        ctx.row.as_bytes(),
    ]
}

fn seal_node(key: &[u8; 32], kind: &str, ctx: FieldCtx<'_>, plain: &str) -> Result<String> {
    let raw =
        crate::espacio::keys::seal_with_ad(key, &field_ad_fields(kind, ctx), plain.as_bytes())?;
    Ok(format!(
        "{NODE_SEAL_PREFIX}{}",
        crate::crypto::hex_encode(raw)
    ))
}

/// Open an `xcn2:` value. A malformed envelope, or one that does not authenticate
/// for this kind and row context, is an error. It is never returned as clear text.
fn open_node(key: &[u8; 32], kind: &str, ctx: FieldCtx<'_>, stored: &str) -> Result<String> {
    let body = stored
        .strip_prefix(NODE_SEAL_PREFIX)
        .ok_or_else(|| anyhow::anyhow!("conversation row is corrupt"))?;
    let well_formed = body.len() >= 56
        && body.len() % 2 == 0
        && body.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'));
    anyhow::ensure!(well_formed, "conversation row is corrupt");
    let raw = crate::crypto::hex_decode(body)
        .map_err(|e| anyhow::anyhow!("conversation row is corrupt: {e}"))?;
    let pt = crate::espacio::keys::open_with_ad(key, &field_ad_fields(kind, ctx), &raw)
        .map_err(|_| anyhow::anyhow!("conversation row failed authentication"))?;
    String::from_utf8(pt.to_vec()).map_err(|_| anyhow::anyhow!("row is not valid UTF-8"))
}

/// Open an `xcn1:` value (no AD). Only the seal floor migration reads these.
fn open_legacy_node(key: &[u8; 32], stored: &str) -> Result<String> {
    let body = stored
        .strip_prefix(LEGACY_NODE_SEAL_PREFIX)
        .ok_or_else(|| anyhow::anyhow!("missing legacy prefix"))?;
    anyhow::ensure!(
        body.len() >= 56 && body.len() % 2 == 0 && body.bytes().all(|c| c.is_ascii_hexdigit()),
        "invalid legacy node seal format"
    );
    let raw = crate::crypto::hex_decode(body)
        .map_err(|e| anyhow::anyhow!("legacy hex decode failed: {e}"))?;
    let pt = crate::crypto::encryption::aes_decrypt(&raw, key)
        .map_err(|e| anyhow::anyhow!("legacy decrypt failed: {e}"))?;
    String::from_utf8(pt).map_err(|e| anyhow::anyhow!("legacy utf-8 failed: {e}"))
}

/// Seal a legacy or clear value for the floor migration. `xr1:` and `xcn2:` values
/// are already valid and return `None`.
fn reseal_legacy(
    key: &[u8; 32],
    kind: &str,
    ctx: FieldCtx<'_>,
    stored: Option<&str>,
) -> Result<Option<String>> {
    let Some(stored) = stored else {
        return Ok(None);
    };
    if crate::espacio::keys::looks_encrypted(stored) || stored.starts_with(NODE_SEAL_PREFIX) {
        return Ok(None);
    }
    let plain = if is_legacy_node_sealed(stored) {
        let (table, column) = kind_to_table_column(kind);
        open_legacy_node(key, stored).map_err(|_| {
            anyhow::anyhow!(
                "corrupt legacy xcn1 row in table '{table}', column '{column}', row id '{}': restore from backup or delete the corrupt row",
                ctx.row
            )
        })?
    } else {
        stored.to_string()
    };
    seal_node(key, kind, ctx, &plain).map(Some)
}

impl Sealer {
    fn seal(&self, kind: &str, ctx: FieldCtx<'_>, plain: &str) -> Result<String> {
        match self {
            Sealer::Dek(k) => {
                k.seal_record(crate::espacio::keys::DEFAULT_SPACE_ID, kind, ctx.row, plain)
            }
            Sealer::Node(key) => seal_node(key, kind, ctx, plain),
            Sealer::Unavailable(msg) => anyhow::bail!("{msg}"),
            Sealer::None => Ok(plain.to_string()),
        }
    }

    fn seal_opt(
        &self,
        kind: &str,
        ctx: FieldCtx<'_>,
        plain: Option<&str>,
    ) -> Result<Option<String>> {
        if let Sealer::Unavailable(msg) = self {
            anyhow::bail!("{msg}");
        }
        plain.map(|p| self.seal(kind, ctx, p)).transpose()
    }

    /// Node key for reading `xcn2:` values: this sealer's own, else the node's.
    fn read_node_key(&self) -> Result<[u8; 32]> {
        use crate::memory::sqlite_vec_store::at_rest;
        match self {
            Sealer::Node(k) => Ok(*k),
            Sealer::Unavailable(msg) => anyhow::bail!("{msg}"),
            _ => at_rest::peek_record_key_with_source()
                .0
                .ok_or_else(|| anyhow::anyhow!("sealed conversation row but no node key")),
        }
    }

    /// Open a stored value. `xr1:` rows open under the default-space key and
    /// `xcn2:` rows under the node key. Under the node tier, a legacy or clear
    /// value is an error: the seal floor has already converted them.
    fn open(&self, kind: &str, ctx: FieldCtx<'_>, stored: String) -> Result<String> {
        use crate::memory::sqlite_vec_store::at_rest;
        if crate::espacio::keys::looks_encrypted(&stored) {
            let dek = match self {
                Sealer::Dek(k) => k.clone(),
                _ => at_rest::default_space_key()?,
            };
            return dek.open_record(
                crate::espacio::keys::DEFAULT_SPACE_ID,
                kind,
                ctx.row,
                &stored,
            );
        }
        if stored.starts_with(NODE_SEAL_PREFIX) {
            return open_node(&self.read_node_key()?, kind, ctx, &stored);
        }
        match self {
            Sealer::Node(_) => anyhow::bail!("legacy conversation value after seal floor"),
            _ if is_legacy_node_sealed(&stored) => {
                let (table, column) = kind_to_table_column(kind);
                open_legacy_node(&self.read_node_key()?, &stored).map_err(|_| {
                    anyhow::anyhow!(
                        "corrupt legacy xcn1 row in table '{table}', column '{column}', row id '{}': restore from backup or delete the corrupt row",
                        ctx.row
                    )
                })
            }
            _ => Ok(stored),
        }
    }

    fn open_opt(
        &self,
        kind: &str,
        ctx: FieldCtx<'_>,
        stored: Option<String>,
    ) -> Result<Option<String>> {
        stored.map(|s| self.open(kind, ctx, s)).transpose()
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

/// Seal floor for node-tier data, run once per file and node key. It runs in one
/// `BEGIN IMMEDIATE` transaction, so a concurrent process sees the marker or
/// nothing. Any error rolls the whole pass back.
fn ensure_seal_floor(conn: &Connection, key: &[u8; 32], handle_project: &str) -> Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    match seal_floor_in_tx(conn, key, handle_project) {
        Ok(()) => {
            if let Err(e) = conn.execute_batch("COMMIT") {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e).context("failed to commit seal floor");
            }
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
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
        let ctx = FieldCtx {
            project: &scope.project_id,
            parent: &scope.session_id,
            row: &id,
        };
        let Ok(content) = sealer.open("conv.msg.content", ctx, stored) else {
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
        // The capture opens node-tier messages, so the seal floor must hold first.
        self.ready_sealer().await.map_err(db_err)?;

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

/// A legacy value re-sealed by the floor migration, applied after the scan.
struct PendingReseal {
    table: &'static str,
    column: &'static str,
    row_id: String,
    sealed: String,
}

fn push_reseal(
    out: &mut Vec<PendingReseal>,
    table: &'static str,
    column: &'static str,
    row_id: &str,
    sealed: Option<String>,
) {
    if let Some(sealed) = sealed {
        out.push(PendingReseal {
            table,
            column,
            row_id: row_id.to_string(),
            sealed,
        });
    }
}

fn seal_floor_in_tx(conn: &Connection, key: &[u8; 32], handle_project: &str) -> Result<()> {
    let floor_ctx = FieldCtx {
        project: "",
        parent: "",
        row: "floor",
    };
    let marker: Option<String> = conn
        .query_row("SELECT v FROM conv_seal_state WHERE k = 'floor'", [], |r| {
            r.get(0)
        })
        .optional()?;
    if let Some(sealed) = marker {
        let authentic =
            open_node(key, FLOOR_KIND, floor_ctx, &sealed).is_ok_and(|v| v == FLOOR_VALUE);
        anyhow::ensure!(authentic, "seal floor marker failed authentication");
        return Ok(());
    }
    // Without a marker, `xcn2:` values mean the marker was removed. Refuse, so that
    // legacy values cannot be written back in and laundered as authentic.
    let node_sealed: bool = conn.query_row(SQL_ANY_NODE_SEALED, [], |r| r.get(0))?;
    anyhow::ensure!(!node_sealed, "seal floor marker missing");

    let mut pending = Vec::new();
    queue_legacy_reseals(conn, key, handle_project, &mut pending)?;
    for p in &pending {
        conn.execute(
            &format!("UPDATE {} SET {} = ?1 WHERE id = ?2", p.table, p.column),
            params![p.sealed, p.row_id],
        )?;
    }
    let marker = seal_node(key, FLOOR_KIND, floor_ctx, FLOOR_VALUE)?;
    conn.execute(
        "INSERT INTO conv_seal_state (k, v) VALUES ('floor', ?1)",
        params![marker],
    )?;
    Ok(())
}

/// Scan every sealed column and queue the legacy values for re-sealing under their
/// row context. A row's project is its own `project_id`, or the handle's project
/// when that is NULL. A message's project is its thread's project.
fn queue_legacy_reseals(
    conn: &Connection,
    key: &[u8; 32],
    handle_project: &str,
    out: &mut Vec<PendingReseal>,
) -> Result<()> {
    {
        let mut stmt =
            conn.prepare("SELECT id, project_id, title, last_preview FROM conversation_threads")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        for row in rows {
            let (id, project, title, preview) = row?;
            let project = project.unwrap_or_else(|| handle_project.to_string());
            let ctx = FieldCtx {
                project: &project,
                parent: "",
                row: &id,
            };
            let title = reseal_legacy(key, "conv.thread.title", ctx, title.as_deref())?;
            push_reseal(out, "conversation_threads", "title", &id, title);
            let preview = reseal_legacy(key, "conv.thread.preview", ctx, preview.as_deref())?;
            push_reseal(out, "conversation_threads", "last_preview", &id, preview);
        }
    }
    {
        let mut stmt = conn.prepare(
            "SELECT m.id, m.thread_id, t.project_id, m.content, m.tool_calls,
                    m.openui_lang, m.xui_json, m.metadata
             FROM conversation_messages m
             LEFT JOIN conversation_threads t ON t.id = m.thread_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        })?;
        for row in rows {
            let (id, thread, project, content, tool_calls, openui_lang, xui_json, metadata) = row?;
            let project = project.unwrap_or_else(|| handle_project.to_string());
            let ctx = FieldCtx {
                project: &project,
                parent: &thread,
                row: &id,
            };
            let table = "conversation_messages";
            let content = reseal_legacy(key, "conv.msg.content", ctx, Some(&content))?;
            push_reseal(out, table, "content", &id, content);
            let tool_calls = reseal_legacy(key, "conv.msg.tool_calls", ctx, tool_calls.as_deref())?;
            push_reseal(out, table, "tool_calls", &id, tool_calls);
            let openui_lang =
                reseal_legacy(key, "conv.msg.openui_lang", ctx, openui_lang.as_deref())?;
            push_reseal(out, table, "openui_lang", &id, openui_lang);
            let xui_json = reseal_legacy(key, "conv.msg.xui_json", ctx, xui_json.as_deref())?;
            push_reseal(out, table, "xui_json", &id, xui_json);
            let metadata = reseal_legacy(key, "conv.msg.metadata", ctx, metadata.as_deref())?;
            push_reseal(out, table, "metadata", &id, metadata);
        }
    }
    {
        let mut stmt = conn.prepare("SELECT id, project_id, deduction FROM deductions")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, project, text) = row?;
            let project = project.unwrap_or_else(|| handle_project.to_string());
            let ctx = FieldCtx {
                project: &project,
                parent: "",
                row: &id,
            };
            let text = reseal_legacy(key, "conv.deduction.text", ctx, Some(&text))?;
            push_reseal(out, "deductions", "deduction", &id, text);
        }
    }
    {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, subject, relation, object, proposition FROM beliefs",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?;
        for row in rows {
            let (id, project, subject, relation, object, proposition) = row?;
            let project = project.unwrap_or_else(|| handle_project.to_string());
            let ctx = FieldCtx {
                project: &project,
                parent: "",
                row: &id,
            };
            let table = "beliefs";
            let subject = reseal_legacy(key, "conv.belief.subject", ctx, Some(&subject))?;
            push_reseal(out, table, "subject", &id, subject);
            let relation = reseal_legacy(key, "conv.belief.relation", ctx, relation.as_deref())?;
            push_reseal(out, table, "relation", &id, relation);
            let object = reseal_legacy(key, "conv.belief.object", ctx, object.as_deref())?;
            push_reseal(out, table, "object", &id, object);
            let proposition =
                reseal_legacy(key, "conv.belief.proposition", ctx, proposition.as_deref())?;
            push_reseal(out, table, "proposition", &id, proposition);
        }
    }
    Ok(())
}

/// Parse an RFC 3339 datetime string.
fn parse_datetime(s: &str) -> DateTime<Utc> {
    s.parse::<DateTime<Utc>>().unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};

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
        assert!(stored.starts_with("xcn2:"), "legacy row was not re-sealed");
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

    // ------------------------------------------------------------------
    // Row-bound node seals (xcn2)
    // ------------------------------------------------------------------

    const NODE_KEY: [u8; 32] = [0x5A; 32];

    /// Field AD for project `p1`, kind `conv.msg.content`, thread `t1`, row `m1`:
    /// u32 BE length + bytes for "xavier-conv-field", "2", "node", "p1", kind, "t1", "m1".
    const GOLDEN_FIELD_AD_HEX: &str = concat!(
        "00000011",
        "7861766965722d636f6e762d6669656c64",
        "00000001",
        "32",
        "00000004",
        "6e6f6465",
        "00000002",
        "7031",
        "00000010",
        "636f6e762e6d73672e636f6e74656e74",
        "00000002",
        "7431",
        "00000002",
        "6d31",
    );

    const SEALED_COLUMNS: [(&str, &str); 12] = [
        ("conversation_threads", "title"),
        ("conversation_threads", "last_preview"),
        ("conversation_messages", "content"),
        ("conversation_messages", "tool_calls"),
        ("conversation_messages", "openui_lang"),
        ("conversation_messages", "xui_json"),
        ("conversation_messages", "metadata"),
        ("deductions", "deduction"),
        ("beliefs", "subject"),
        ("beliefs", "relation"),
        ("beliefs", "object"),
        ("beliefs", "proposition"),
    ];

    fn raw_conn(path: &Path) -> rusqlite::Connection {
        rusqlite::Connection::open(path).unwrap()
    }

    /// Values of a single-column query, in result order.
    fn raw_column(path: &Path, sql: &str) -> Vec<Option<String>> {
        let conn = raw_conn(path);
        let mut stmt = conn.prepare(sql).unwrap();
        let rows = stmt
            .query_map([], |r| r.get::<_, Option<String>>(0))
            .unwrap();
        rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
    }

    /// Every sealed column plus the floor marker, in row order.
    fn sealed_snapshot(path: &Path) -> Vec<Option<String>> {
        let mut all = Vec::new();
        for (table, column) in SEALED_COLUMNS {
            all.extend(raw_column(
                path,
                &format!("SELECT {column} FROM {table} ORDER BY rowid"),
            ));
        }
        all.extend(raw_column(path, "SELECT v FROM conv_seal_state ORDER BY k"));
        all
    }

    async fn node_db(path: &Path, project: &str) -> ConversationsDb {
        ConversationsDb::open_at(project, path)
            .await
            .unwrap()
            .with_node_key(NODE_KEY)
    }

    /// The node-tier envelope as written before the row-bound format (no AD).
    fn legacy_xcn1(key: &[u8; 32], plain: &str) -> String {
        use crate::crypto::encryption::{aes_encrypt, NonceBytes};
        let ct = aes_encrypt(plain.as_bytes(), key, &NonceBytes::generate()).unwrap();
        format!("xcn1:{}", crate::crypto::hex_encode(ct))
    }

    /// Flip the hex digit at byte offset `at` (the result is still hex and always differs).
    fn flip_at(value: &str, at: usize) -> String {
        let mut bytes = value.as_bytes().to_vec();
        bytes[at] = if bytes[at] == b'0' { b'1' } else { b'0' };
        String::from_utf8(bytes).unwrap()
    }

    #[tokio::test]
    async fn xcn2_field_ad_golden_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = node_db(&path, "p1").await;
        db.create_schema().await.unwrap();
        raw_conn(&path)
            .execute_batch(
                "INSERT INTO conversation_threads (id, project_id, started_at, updated_at)
                    VALUES ('t1', 'p1', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO conversation_messages (id, thread_id, role, content, created_at)
                    VALUES ('m1', 't1', 'user', 'hello golden', '2026-01-01T00:00:00Z');",
            )
            .unwrap();

        // The first node-tier operation seals the legacy clear content under its row context.
        let msgs = db.get_thread_messages("t1").await.unwrap();
        assert_eq!(msgs[0].content, "hello golden");

        let stored = raw_column(
            &path,
            "SELECT content FROM conversation_messages WHERE id = 'm1'",
        )
        .remove(0)
        .unwrap();
        let body = stored
            .strip_prefix("xcn2:")
            .expect("content is sealed as xcn2");
        let raw = crate::crypto::hex_decode(body).unwrap();
        let (nonce, ct) = raw.split_at(12);
        let aad = crate::crypto::hex_decode(GOLDEN_FIELD_AD_HEX).unwrap();
        let cipher = Aes256Gcm::new_from_slice(&NODE_KEY).unwrap();
        let pt = cipher
            .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad: &aad })
            .expect("golden AD authenticates the sealed content");
        assert_eq!(pt, b"hello golden");
    }

    #[tokio::test]
    async fn xcn2_written_for_every_sealed_column() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = node_db(&path, "seal-cols").await;
        let thread = db.create_thread(Some("title"), None, None).await.unwrap();
        db.store_message(
            &thread.id,
            "user",
            "content",
            Some("tools"),
            Some("lang"),
            Some("xui"),
            Some("meta"),
            None,
        )
        .await
        .unwrap();
        db.record_deduction("deduction", 0.5, None, None)
            .await
            .unwrap();
        let belief = db
            .upsert_belief(
                "subject",
                Some("rel"),
                Some("obj"),
                Some("prop"),
                0.9,
                1.0,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        for (table, column) in SEALED_COLUMNS {
            let values = raw_column(&path, &format!("SELECT {column} FROM {table}"));
            let sealed: Vec<&String> = values.iter().flatten().collect();
            assert!(!sealed.is_empty(), "{table}.{column} has no stored value");
            for v in sealed {
                assert!(
                    v.starts_with("xcn2:"),
                    "{table}.{column} stored without xcn2 seal: {v}"
                );
            }
        }

        // Reads round-trip.
        let msgs = db.get_thread_messages(&thread.id).await.unwrap();
        assert_eq!(msgs[0].content, "content");
        assert_eq!(msgs[0].tool_calls.as_deref(), Some("tools"));
        assert_eq!(msgs[0].openui_lang.as_deref(), Some("lang"));
        assert_eq!(msgs[0].xui_json.as_deref(), Some("xui"));
        assert_eq!(msgs[0].metadata.as_deref(), Some("meta"));
        let b = db.get_belief(&belief.id).await.unwrap().unwrap();
        assert_eq!(b.subject, "subject");
        assert_eq!(b.relation.as_deref(), Some("rel"));
        assert_eq!(b.object.as_deref(), Some("obj"));
        assert_eq!(b.proposition.as_deref(), Some("prop"));
        assert_eq!(
            db.list_deductions(5).await.unwrap()[0].deduction,
            "deduction"
        );
        assert_eq!(
            db.get_thread(&thread.id)
                .await
                .unwrap()
                .unwrap()
                .title
                .as_deref(),
            Some("title")
        );
    }

    #[tokio::test]
    async fn xcn2_splice_across_rows_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = node_db(&path, "splice-rows").await;
        let thread = db.create_thread(None, None, None).await.unwrap();
        let m1 = db
            .store_message(&thread.id, "user", "first", None, None, None, None, None)
            .await
            .unwrap();
        let m2 = db
            .store_message(&thread.id, "user", "second", None, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(db.get_thread_messages(&thread.id).await.unwrap().len(), 2);

        raw_conn(&path)
            .execute(
                "UPDATE conversation_messages SET content =
                    (SELECT content FROM conversation_messages WHERE id = ?1) WHERE id = ?2",
                params![m1.id, m2.id],
            )
            .unwrap();
        assert!(db.get_thread_messages(&thread.id).await.is_err());
    }

    #[tokio::test]
    async fn xcn2_splice_across_columns_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = node_db(&path, "splice-cols").await;
        let thread = db.create_thread(None, None, None).await.unwrap();
        let m1 = db
            .store_message(
                &thread.id,
                "user",
                "body",
                None,
                None,
                None,
                Some("meta"),
                None,
            )
            .await
            .unwrap();
        let belief = db
            .upsert_belief(
                "subject",
                Some("rel"),
                Some("obj"),
                None,
                0.5,
                1.0,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        // Message content into the metadata column of the same row.
        raw_conn(&path)
            .execute(
                "UPDATE conversation_messages SET metadata = content WHERE id = ?1",
                params![m1.id],
            )
            .unwrap();
        assert!(db.get_thread_messages(&thread.id).await.is_err());

        // Belief subject into the object column of the same row.
        raw_conn(&path)
            .execute(
                "UPDATE beliefs SET object = subject WHERE id = ?1",
                params![belief.id],
            )
            .unwrap();
        assert!(db.get_belief(&belief.id).await.is_err());
    }

    #[tokio::test]
    async fn xcn2_splice_across_tables_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = node_db(&path, "splice-tables").await;
        let deduction = db
            .record_deduction("secret deduction", 0.5, None, None)
            .await
            .unwrap();
        // Subject only: the other belief columns would fail for a row-id reason and hide the kind check.
        let belief = db
            .upsert_belief("subject", None, None, None, 0.5, 1.0, None, None, None)
            .await
            .unwrap();

        // The belief takes the deduction's id and the deduction's sealed text as its subject.
        raw_conn(&path)
            .execute(
                "UPDATE beliefs SET id = ?1,
                    subject = (SELECT deduction FROM deductions WHERE id = ?1)
                 WHERE id = ?2",
                params![deduction.id, belief.id],
            )
            .unwrap();
        assert!(db.get_belief(&deduction.id).await.is_err());
    }

    #[tokio::test]
    async fn xcn2_message_moved_to_other_thread_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = node_db(&path, "moved").await;
        let t1 = db.create_thread(None, None, None).await.unwrap();
        let t2 = db.create_thread(None, None, None).await.unwrap();
        let m1 = db
            .store_message(&t1.id, "user", "stays put", None, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(db.get_thread_messages(&t1.id).await.unwrap().len(), 1);

        raw_conn(&path)
            .execute(
                "UPDATE conversation_messages SET thread_id = ?1 WHERE id = ?2",
                params![t2.id, m1.id],
            )
            .unwrap();
        assert!(db.get_thread_messages(&t2.id).await.is_err());
    }

    #[tokio::test]
    async fn xcn2_row_copied_to_other_project_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let owner = node_db(&path, "proj-a").await;
        let other = node_db(&path, "proj-b").await;
        let thread = owner
            .create_thread(Some("private title"), None, None)
            .await
            .unwrap();
        owner
            .store_message(
                &thread.id,
                "user",
                "private message",
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        raw_conn(&path)
            .execute(
                "UPDATE conversation_threads SET project_id = 'proj-b' WHERE id = ?1",
                params![thread.id],
            )
            .unwrap();
        assert!(other.list_threads(10).await.is_err());
        assert!(other.get_thread(&thread.id).await.is_err());
        assert!(other.get_thread_messages(&thread.id).await.is_err());
    }

    #[tokio::test]
    async fn xcn2_tamper_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = node_db(&path, "tamper").await;
        let thread = db.create_thread(None, None, None).await.unwrap();
        let m1 = db
            .store_message(
                &thread.id,
                "user",
                "tamper target",
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        let sql = format!(
            "SELECT content FROM conversation_messages WHERE id = '{}'",
            m1.id
        );
        let stored = raw_column(&path, &sql).remove(0).unwrap();
        assert!(stored.starts_with("xcn2:"), "content is not xcn2: {stored}");
        let set_content = |value: &str| {
            raw_conn(&path)
                .execute(
                    "UPDATE conversation_messages SET content = ?1 WHERE id = ?2",
                    params![value, m1.id],
                )
                .unwrap();
        };

        // One nibble in the nonce, one in the ciphertext, one in the tag.
        let prefix = "xcn2:".len();
        let last = stored.len() - 1;
        for at in [prefix, prefix + 24, last] {
            set_content(&flip_at(&stored, at));
            assert!(
                db.get_thread_messages(&thread.id).await.is_err(),
                "tampered byte at {at} was accepted"
            );
        }

        set_content(&stored);
        assert_eq!(
            db.get_thread_messages(&thread.id).await.unwrap()[0].content,
            "tamper target"
        );
    }

    #[tokio::test]
    async fn xcn2_malformed_envelope_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = node_db(&path, "malformed").await;
        let thread = db.create_thread(Some("title"), None, None).await.unwrap();

        let malformed = [
            "xcn2:zz".to_string(),
            "xcn2:abc".to_string(),
            format!("xcn2:{}", "00".repeat(27)),
        ];
        for value in malformed {
            raw_conn(&path)
                .execute(
                    "UPDATE conversation_threads SET title = ?1 WHERE id = ?2",
                    params![value, thread.id],
                )
                .unwrap();
            assert!(
                db.get_thread(&thread.id).await.is_err(),
                "malformed {value} was returned instead of failing"
            );
        }
    }

    #[tokio::test]
    async fn xcn1_relabel_to_xcn2_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = node_db(&path, "relabel").await;
        let thread = db.create_thread(None, None, None).await.unwrap();
        let m1 = db
            .store_message(
                &thread.id,
                "user",
                "relabel target",
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let relabeled = legacy_xcn1(&NODE_KEY, "relabel target").replacen("xcn1:", "xcn2:", 1);
        raw_conn(&path)
            .execute(
                "UPDATE conversation_messages SET content = ?1 WHERE id = ?2",
                params![relabeled, m1.id],
            )
            .unwrap();
        assert!(db.get_thread_messages(&thread.id).await.is_err());
    }

    #[tokio::test]
    async fn legacy_xcn1_and_clear_rows_migrate_to_xcn2() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = node_db(&path, "mig").await;
        db.create_schema().await.unwrap();
        {
            let conn = raw_conn(&path);
            conn.execute(
                "INSERT INTO conversation_threads (id, project_id, title, last_preview, started_at, updated_at)
                 VALUES ('t1', 'mig', ?1, 'clear preview', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![legacy_xcn1(&NODE_KEY, "legacy title")],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO conversation_messages (id, thread_id, role, content, tool_calls, metadata, created_at)
                 VALUES ('m1', 't1', 'user', ?1, 'clear tool', ?2, '2026-01-01T00:00:00Z')",
                params![
                    legacy_xcn1(&NODE_KEY, "legacy content"),
                    legacy_xcn1(&NODE_KEY, "legacy meta")
                ],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO deductions (id, project_id, deduction, created_at)
                 VALUES ('d1', 'mig', ?1, '2026-01-01T00:00:00Z')",
                params![legacy_xcn1(&NODE_KEY, "legacy deduction")],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO beliefs (id, project_id, subject, object, proposition, created_at, updated_at)
                 VALUES ('b1', 'mig', 'clear subject', ?1, 'clear prop', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![legacy_xcn1(&NODE_KEY, "legacy object")],
            )
            .unwrap();
        }

        // The first operation reads the plaintext and seals every legacy value.
        let msgs = db.get_thread_messages("t1").await.unwrap();
        assert_eq!(msgs[0].content, "legacy content");
        assert_eq!(msgs[0].tool_calls.as_deref(), Some("clear tool"));
        assert_eq!(msgs[0].metadata.as_deref(), Some("legacy meta"));
        let thread = db.get_thread("t1").await.unwrap().unwrap();
        assert_eq!(thread.title.as_deref(), Some("legacy title"));
        assert_eq!(thread.last_preview.as_deref(), Some("clear preview"));
        assert_eq!(
            db.list_deductions(5).await.unwrap()[0].deduction,
            "legacy deduction"
        );
        let b = db.get_belief("b1").await.unwrap().unwrap();
        assert_eq!(b.subject, "clear subject");
        assert_eq!(b.object.as_deref(), Some("legacy object"));

        for (table, column) in SEALED_COLUMNS {
            for v in raw_column(&path, &format!("SELECT {column} FROM {table}"))
                .into_iter()
                .flatten()
            {
                assert!(
                    v.starts_with("xcn2:"),
                    "{table}.{column} left unsealed: {v}"
                );
            }
        }
        assert_eq!(
            raw_column(&path, "SELECT k FROM conv_seal_state"),
            vec![Some("floor".to_string())]
        );

        // Reopening is a no-op: stored bytes do not change.
        let before = sealed_snapshot(&path);
        let reopened = node_db(&path, "mig").await;
        assert_eq!(
            reopened.get_thread_messages("t1").await.unwrap()[0].content,
            "legacy content"
        );
        assert_eq!(sealed_snapshot(&path), before);
    }

    #[tokio::test]
    async fn downgrade_after_floor_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = node_db(&path, "downgrade").await;
        let thread = db.create_thread(Some("title"), None, None).await.unwrap();
        let m1 = db
            .store_message(&thread.id, "user", "content", None, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(db.get_thread_messages(&thread.id).await.unwrap().len(), 1);

        // A genuine legacy value for this key and row, written after the floor.
        raw_conn(&path)
            .execute(
                "UPDATE conversation_messages SET content = ?1 WHERE id = ?2",
                params![legacy_xcn1(&NODE_KEY, "old content"), m1.id],
            )
            .unwrap();
        assert!(db.get_thread_messages(&thread.id).await.is_err());

        // Clear text written after the floor.
        raw_conn(&path)
            .execute(
                "UPDATE conversation_threads SET title = 'clear title' WHERE id = ?1",
                params![thread.id],
            )
            .unwrap();
        assert!(db.get_thread(&thread.id).await.is_err());
    }

    #[tokio::test]
    async fn floor_marker_deleted_with_xcn2_rows_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = node_db(&path, "marker-del").await;
        let thread = db
            .create_thread(Some("sealed title"), None, None)
            .await
            .unwrap();
        // A second sealed value, untouched by the injection, keeps `xcn2:` rows in the file.
        db.store_message(
            &thread.id,
            "user",
            "kept sealed",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        drop(db);

        raw_conn(&path)
            .execute_batch(&format!(
                "DELETE FROM conv_seal_state;
                 UPDATE conversation_threads SET title = 'injected clear' WHERE id = '{}';",
                thread.id
            ))
            .unwrap();

        // A fresh handle runs the floor check again; it must not migrate the injected value.
        let fresh = node_db(&path, "marker-del").await;
        assert!(fresh.get_thread(&thread.id).await.is_err());
        assert_eq!(
            raw_column(
                &path,
                &format!(
                    "SELECT title FROM conversation_threads WHERE id = '{}'",
                    thread.id
                )
            ),
            vec![Some("injected clear".to_string())]
        );
    }

    #[tokio::test]
    async fn floor_marker_tampered_or_wrong_key_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = node_db(&path, "marker-tamper").await;
        let thread = db
            .create_thread(Some("sealed title"), None, None)
            .await
            .unwrap();
        drop(db);

        let marker = raw_column(&path, "SELECT v FROM conv_seal_state WHERE k = 'floor'")
            .remove(0)
            .unwrap();
        let set_marker = |value: &str| {
            raw_conn(&path)
                .execute(
                    "UPDATE conv_seal_state SET v = ?1 WHERE k = 'floor'",
                    params![value],
                )
                .unwrap();
        };
        set_marker(&flip_at(&marker, 5 + 10));
        let tampered = node_db(&path, "marker-tamper").await;
        assert!(tampered.get_thread(&thread.id).await.is_err());

        set_marker(&marker);
        let restored = node_db(&path, "marker-tamper").await;
        assert!(restored.get_thread(&thread.id).await.unwrap().is_some());

        // Another key must not migrate or write anything.
        let before = sealed_snapshot(&path);
        let wrong = ConversationsDb::open_at("marker-tamper", &path)
            .await
            .unwrap()
            .with_node_key([0x5B; 32]);
        assert!(wrong.get_thread(&thread.id).await.is_err());
        assert_eq!(sealed_snapshot(&path), before);
    }

    #[tokio::test]
    async fn seal_floor_migration_is_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = node_db(&path, "atomic").await;
        db.create_schema().await.unwrap();
        {
            let conn = raw_conn(&path);
            conn.execute_batch(
                "INSERT INTO conversation_threads (id, project_id, title, started_at, updated_at)
                    VALUES ('t1', 'atomic', 'clear title', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO conversation_messages (id, thread_id, role, content, created_at)
                    VALUES ('m1', 't1', 'user', 'clear message', '2026-01-01T00:00:00Z');
                 INSERT INTO beliefs (id, project_id, subject, created_at, updated_at)
                    VALUES ('b1', 'atomic', 'clear subject', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 CREATE TRIGGER reject_belief_update BEFORE UPDATE ON beliefs
                    BEGIN SELECT RAISE(ABORT, 'update rejected'); END;",
            )
            .unwrap();
        }

        assert!(db.get_thread_messages("t1").await.is_err());
        assert_eq!(
            raw_column(&path, "SELECT title FROM conversation_threads"),
            vec![Some("clear title".to_string())]
        );
        assert_eq!(
            raw_column(&path, "SELECT content FROM conversation_messages"),
            vec![Some("clear message".to_string())]
        );
        assert_eq!(
            raw_column(&path, "SELECT subject FROM beliefs"),
            vec![Some("clear subject".to_string())]
        );
        assert_eq!(
            raw_conn(&path)
                .query_row("SELECT COUNT(*) FROM conv_seal_state", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    struct TestEnvGuard {
        saved_data_dir: Option<std::ffi::OsString>,
        saved_record_key: Option<std::ffi::OsString>,
    }

    impl Drop for TestEnvGuard {
        fn drop(&mut self) {
            match &self.saved_data_dir {
                Some(v) => {
                    std::env::set_var(crate::memory::sqlite_vec_store::at_rest::DATA_DIR_ENV, v)
                }
                None => {
                    std::env::remove_var(crate::memory::sqlite_vec_store::at_rest::DATA_DIR_ENV)
                }
            }
            match &self.saved_record_key {
                Some(v) => {
                    std::env::set_var(crate::memory::sqlite_vec_store::at_rest::RECORD_KEY_ENV, v)
                }
                None => {
                    std::env::remove_var(crate::memory::sqlite_vec_store::at_rest::RECORD_KEY_ENV)
                }
            }
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn write_with_unavailable_key_fails_closed() {
        let _guard = TestEnvGuard {
            saved_data_dir: std::env::var_os(
                crate::memory::sqlite_vec_store::at_rest::DATA_DIR_ENV,
            ),
            saved_record_key: std::env::var_os(
                crate::memory::sqlite_vec_store::at_rest::RECORD_KEY_ENV,
            ),
        };
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(
            crate::memory::sqlite_vec_store::at_rest::DATA_DIR_ENV,
            dir.path(),
        );
        std::env::remove_var(crate::memory::sqlite_vec_store::at_rest::RECORD_KEY_ENV);

        // Make the node key file corrupt/invalid hex so KeySource::Unavailable is returned
        let key_path = crate::memory::sqlite_vec_store::at_rest::record_key_path();
        std::fs::create_dir_all(key_path.parent().unwrap()).unwrap();
        std::fs::write(&key_path, "not-valid-hex-content").unwrap();

        let db_path = dir.path().join("conversations.db");
        let db = ConversationsDb::open_at("unavail-proj", &db_path)
            .await
            .unwrap();
        db.create_schema().await.unwrap();

        // Plant an already-clear legacy thread and message directly in SQLite
        raw_conn(&db_path)
            .execute_batch(
                "INSERT INTO conversation_threads (id, project_id, title, started_at, updated_at)
                 VALUES ('t-legacy', 'unavail-proj', 'legacy clear title', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO conversation_messages (id, thread_id, role, content, created_at)
                 VALUES ('m-legacy', 't-legacy', 'user', 'legacy clear content', '2026-01-01T00:00:00Z');",
            )
            .unwrap();

        // Reads of already-clear legacy rows keep working
        let thread = db
            .get_thread("t-legacy")
            .await
            .unwrap()
            .expect("thread exists");
        assert_eq!(thread.title.as_deref(), Some("legacy clear title"));
        let msgs = db.get_thread_messages("t-legacy").await.unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "legacy clear content");

        // Attempting to write with Unavailable key must fail closed (Err)
        let store_res = db
            .store_message(
                "t-legacy",
                "user",
                "secret cleartext message",
                None,
                None,
                None,
                None,
                None,
            )
            .await;
        assert!(
            store_res.is_err(),
            "store_message must fail closed when key is unavailable"
        );

        let preview_res = db.update_last_preview("t-legacy", "secret preview").await;
        assert!(
            preview_res.is_err(),
            "update_last_preview must fail closed when key is unavailable"
        );

        let create_thread_res = db
            .create_thread(Some("secret thread title"), None, None)
            .await;
        assert!(
            create_thread_res.is_err(),
            "create_thread must fail closed when key is unavailable"
        );

        let deduction_res = db
            .record_deduction("secret deduction text", 0.8, None, None)
            .await;
        assert!(
            deduction_res.is_err(),
            "record_deduction must fail closed when key is unavailable"
        );

        let belief_res = db
            .upsert_belief(
                "secret subject",
                None,
                None,
                None,
                0.5,
                1.0,
                None,
                None,
                None,
            )
            .await;
        assert!(
            belief_res.is_err(),
            "upsert_belief must fail closed when key is unavailable"
        );

        // Assert message count unchanged (still 1 legacy message)
        let msg_count: i64 = raw_conn(&db_path)
            .query_row("SELECT COUNT(*) FROM conversation_messages", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(msg_count, 1);

        // Assert stored value unchanged
        let contents = raw_column(&db_path, "SELECT content FROM conversation_messages");
        assert_eq!(contents, vec![Some("legacy clear content".to_string())]);

        // Assert no clear text in the db file
        let raw = raw_bytes(&db_path);
        for forbidden in [
            "secret cleartext message",
            "secret preview",
            "secret thread title",
            "secret deduction text",
            "secret subject",
        ] {
            assert!(
                raw.windows(forbidden.len())
                    .all(|w| w != forbidden.as_bytes()),
                "cleartext '{forbidden}' found in db file",
            );
        }
    }

    #[tokio::test]
    async fn update_last_preview_and_store_message_reject_foreign_thread() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let owner = node_db(&path, "proj-alpha").await;
        let foreign = node_db(&path, "proj-beta").await;

        let thread = owner
            .create_thread(Some("alpha title"), None, None)
            .await
            .unwrap();
        owner
            .store_message(
                &thread.id,
                "user",
                "alpha message",
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        // Foreign project attempts update_last_preview on alpha's thread -> Err
        let prev_res = foreign
            .update_last_preview(&thread.id, "foreign preview")
            .await;
        assert!(
            prev_res.is_err(),
            "update_last_preview on foreign thread must return Err"
        );

        // Foreign project attempts store_message on alpha's thread -> Err
        let msg_res = foreign
            .store_message(
                &thread.id,
                "user",
                "foreign message",
                None,
                None,
                None,
                None,
                None,
            )
            .await;
        assert!(
            msg_res.is_err(),
            "store_message on foreign thread must return Err"
        );

        // Verify no change occurred
        let thread_after = owner.get_thread(&thread.id).await.unwrap().unwrap();
        if let Some(preview) = thread_after.last_preview {
            assert!(
                !preview.contains("foreign preview"),
                "foreign preview was stored"
            );
        }
        let msgs = owner.get_thread_messages(&thread.id).await.unwrap();
        assert_eq!(msgs.len(), 1, "message count must remain unchanged");
        assert_eq!(msgs[0].content, "alpha message");

        let raw_count: i64 = raw_conn(&path)
            .query_row(
                "SELECT COUNT(*) FROM conversation_messages WHERE thread_id = ?1",
                params![thread.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw_count, 1, "database row count must remain unchanged");
    }

    #[tokio::test]
    async fn corrupt_legacy_xcn1_row_error_contains_table_column_row_id_and_hint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.db");
        let db = node_db(&path, "corrupt-test").await;
        db.create_schema().await.unwrap();

        // Corrupt legacy xcn1 value: valid hex length but modified ciphertext/tag
        let corrupt_content = flip_at(&legacy_xcn1(&NODE_KEY, "test message"), 10);
        raw_conn(&path)
            .execute_batch(&format!(
                "INSERT INTO conversation_threads (id, project_id, title, started_at, updated_at)
                 VALUES ('t-corrupt', 'corrupt-test', 'clean title', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO conversation_messages (id, thread_id, role, content, created_at)
                 VALUES ('m-corrupt', 't-corrupt', 'user', '{corrupt_content}', '2026-01-01T00:00:00Z');",
            ))
            .unwrap();

        let err = db.get_thread_messages("t-corrupt").await.unwrap_err();
        let err_msg = format!("{err:#}");

        assert!(
            err_msg.contains("conversation_messages"),
            "error message '{err_msg}' must contain table name 'conversation_messages'"
        );
        assert!(
            err_msg.contains("content"),
            "error message '{err_msg}' must contain column name 'content'"
        );
        assert!(
            err_msg.contains("m-corrupt"),
            "error message '{err_msg}' must contain row id 'm-corrupt'"
        );
        assert!(
            err_msg.contains("restore from backup") || err_msg.contains("delete"),
            "error message '{err_msg}' must contain repair hint"
        );
        assert!(
            !err_msg.contains(&corrupt_content),
            "error message must NOT contain corrupt value"
        );
        assert!(
            !err_msg.contains(&corrupt_content[5..]),
            "error message must NOT contain raw ciphertext"
        );
    }
}

#[cfg(test)]
mod snapshot_tests;
