//! Private conversations database.
//!
//! Stored at `~/.xavier/conversations/{project_id}.db`, this DB holds
//! AI conversation threads, messages, deductions, agent beliefs, and
//! session checkpoints. It is never committed to a repository.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use libsql::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const CONVERSATIONS_DIR: &str = "conversations";

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

/// A single message within a conversation thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub thread_id: String,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<String>,
    pub openui_lang: Option<String>,
    pub xui_json: Option<String>,
    pub metadata: Option<String>,
    pub created_at: DateTime<Utc>,
    pub tokens: Option<i64>,
}

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
    conn: Connection,
    project_id: String,
}

impl ConversationsDb {
    /// Open (or create) the conversations database for `project_id`.
    ///
    /// The database is stored at `~/.xavier/conversations/{project_id}.db`.
    /// The directory is created if it doesn't exist.
    pub async fn open(project_id: &str) -> Result<Self> {
        let path = Self::db_path(project_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create conversations directory: {}", parent.display()))?;
        }
        let path_str = path.to_string_lossy().to_string();
        let db = libsql::Builder::new_local(&path_str)
            .build()
            .await
            .with_context(|| format!("failed to open conversations database at {}", path.display()))?;
        let conn = db.connect().context("failed to connect to libSQL database")?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        ).await.context("failed to set PRAGMAs")?;
        Ok(Self { conn, project_id: project_id.to_string() })
    }

    /// Open an in-memory conversations database (for testing).
    pub async fn open_in_memory(project_id: &str) -> Result<Self> {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .context("failed to build in-memory libSQL database")?;
        let conn = db.connect().context("failed to connect to in-memory libSQL database")?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;",
        ).await.context("failed to set PRAGMAs")?;
        Ok(Self { conn, project_id: project_id.to_string() })
    }

    /// Create all tables for the conversations database.
    pub async fn create_schema(&self) -> Result<()> {
        self.conn.execute_batch(
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
            );",
        ).await.context("failed to create conversations schema")?;
        Ok(())
    }

    /// Return a reference to the underlying connection.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Return the project ID.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    // ------------------------------------------------------------------
    // Thread CRUD
    // ------------------------------------------------------------------

    /// Create a new conversation thread.
    pub async fn create_thread(&self, title: Option<&str>, model: Option<&str>, source: Option<&str>) -> Result<Thread> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO conversation_threads (id, project_id, title, started_at, updated_at, model, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id.clone(), self.project_id.clone(), title, now.clone(), now, model, source],
        ).await.context("failed to create thread")?;
        Ok(Thread {
            id, project_id: Some(self.project_id.clone()),
            title: title.map(|s| s.to_string()),
            started_at: Utc::now(), updated_at: Utc::now(),
            last_preview: None,
            model: model.map(|s| s.to_string()),
            source: source.map(|s| s.to_string()),
        })
    }

    /// Get a thread by ID.
    pub async fn get_thread(&self, thread_id: &str) -> Result<Option<Thread>> {
        let mut rows = self.conn.query(
            "SELECT id, project_id, title, started_at, updated_at, last_preview, model, source
             FROM conversation_threads WHERE id = ?1",
            params![thread_id],
        ).await?;

        if let Some(row) = rows.next().await? {
            Ok(Some(Thread {
                id: row.get(0)?,
                project_id: row.get(1)?,
                title: row.get(2)?,
                started_at: parse_datetime(&row.get::<String>(3)?),
                updated_at: parse_datetime(&row.get::<String>(4)?),
                last_preview: row.get(5)?,
                model: row.get(6)?,
                source: row.get(7)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// List all threads for a project.
    pub async fn list_threads(&self, limit: usize) -> Result<Vec<Thread>> {
        let mut rows = self.conn.query(
            "SELECT id, project_id, title, started_at, updated_at, last_preview, model, source
             FROM conversation_threads
             WHERE project_id = ?1
             ORDER BY updated_at DESC
             LIMIT ?2",
            params![self.project_id.clone(), limit as i64],
        ).await?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(Thread {
                id: row.get(0)?, project_id: row.get(1)?,
                title: row.get(2)?,
                started_at: parse_datetime(&row.get::<String>(3)?),
                updated_at: parse_datetime(&row.get::<String>(4)?),
                last_preview: row.get(5)?,
                model: row.get(6)?, source: row.get(7)?,
            });
        }
        Ok(results)
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
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO conversation_messages (id, thread_id, role, content, tool_calls, openui_lang, xui_json, metadata, created_at, tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![id.clone(), thread_id, role, content, tool_calls, openui_lang, xui_json, metadata, now.clone(), tokens],
        ).await.context("failed to store message")?;

        // Update thread's updated_at timestamp and last_preview
        let preview = if content.len() > 120 { &content[..120] } else { content };
        let _ = self.conn.execute(
            "UPDATE conversation_threads SET updated_at = ?1, last_preview = ?2 WHERE id = ?3",
            params![now, preview, thread_id],
        ).await;

        Ok(Message {
            id, thread_id: thread_id.to_string(),
            role: role.to_string(), content: content.to_string(),
            tool_calls: tool_calls.map(|s| s.to_string()),
            openui_lang: openui_lang.map(|s| s.to_string()),
            xui_json: xui_json.map(|s| s.to_string()),
            metadata: metadata.map(|s| s.to_string()),
            created_at: Utc::now(), tokens,
        })
    }

    /// Get all messages for a thread, ordered by creation time.
    pub async fn get_thread_messages(&self, thread_id: &str) -> Result<Vec<Message>> {
        let mut rows = self.conn.query(
            "SELECT id, thread_id, role, content, tool_calls, openui_lang, xui_json, metadata, created_at, tokens
             FROM conversation_messages
             WHERE thread_id = ?1
             ORDER BY created_at ASC",
            params![thread_id],
        ).await?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(Message {
                id: row.get(0)?, thread_id: row.get(1)?,
                role: row.get(2)?, content: row.get(3)?,
                tool_calls: row.get(4)?,
                openui_lang: row.get(5)?,
                xui_json: row.get(6)?,
                metadata: row.get(7)?,
                created_at: parse_datetime(&row.get::<String>(8)?),
                tokens: row.get(9)?,
            });
        }
        Ok(results)
    }

    // ------------------------------------------------------------------
    // Deduction CRUD
    // ------------------------------------------------------------------

    /// Record a deduction.
    pub async fn record_deduction(
        &self, deduction_text: &str, confidence: f64,
        category: Option<&str>, source_thread: Option<&str>,
    ) -> Result<Deduction> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO deductions (id, project_id, source_thread, deduction, confidence, created_at, last_accessed, category)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id.clone(), self.project_id.clone(), source_thread, deduction_text, confidence, now.clone(), now, category],
        ).await.context("failed to record deduction")?;
        Ok(Deduction {
            id, project_id: Some(self.project_id.clone()),
            source_thread: source_thread.map(|s| s.to_string()),
            deduction: deduction_text.to_string(), confidence,
            created_at: Utc::now(), last_accessed: Some(Utc::now()),
            category: category.map(|s| s.to_string()),
        })
    }

    /// List deductions for this project.
    pub async fn list_deductions(&self, limit: usize) -> Result<Vec<Deduction>> {
        let mut rows = self.conn.query(
            "SELECT id, project_id, source_thread, deduction, confidence, created_at, last_accessed, category
             FROM deductions WHERE project_id = ?1 ORDER BY created_at DESC LIMIT ?2",
            params![self.project_id.clone(), limit as i64],
        ).await?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(Deduction {
                id: row.get(0)?, project_id: row.get(1)?,
                source_thread: row.get(2)?, deduction: row.get(3)?,
                confidence: row.get(4)?,
                created_at: parse_datetime(&row.get::<String>(5)?),
                last_accessed: row.get::<Option<String>>(6)?.map(|s| parse_datetime(&s)),
                category: row.get(7)?,
            });
        }
        Ok(results)
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
        let now = Utc::now().to_rfc3339();

        // Check if a belief with this subject and relation/object or proposition already exists
        let existing_id = if let (Some(rel), Some(obj)) = (relation, object) {
            let mut rows = self.conn.query(
                "SELECT id FROM beliefs WHERE subject = ?1 AND relation = ?2 AND object = ?3 AND project_id = ?4",
                params![subject, rel, obj, self.project_id.clone()],
            ).await?;
            if let Some(row) = rows.next().await? {
                Some(row.get::<String>(0)?)
            } else {
                None
            }
        } else if let Some(prop) = proposition {
            let mut rows = self.conn.query(
                "SELECT id FROM beliefs WHERE subject = ?1 AND proposition = ?2 AND project_id = ?3",
                params![subject, prop, self.project_id.clone()],
            ).await?;
            if let Some(row) = rows.next().await? {
                Some(row.get::<String>(0)?)
            } else {
                None
            }
        } else {
            let mut rows = self.conn.query(
                "SELECT id FROM beliefs WHERE subject = ?1 AND project_id = ?2 LIMIT 1",
                params![subject, self.project_id.clone()],
            ).await?;
            if let Some(row) = rows.next().await? {
                Some(row.get::<String>(0)?)
            } else {
                None
            }
        };

        let id = match existing_id {
            Some(id) => {
                self.conn.execute(
                    "UPDATE beliefs SET relation = ?1, object = ?2, proposition = ?3, confidence = ?4, \
                     weight = ?5, source = ?6, provenance_id = ?7, contradicts_id = ?8, updated_at = ?9 WHERE id = ?10",
                    params![
                        relation, object, proposition, confidence,
                        weight, source, provenance_id, contradicts_id,
                        now, id.clone()
                    ],
                ).await.context("failed to update belief")?;
                id
            }
            None => {
                let new_id = Uuid::new_v4().to_string();
                self.conn.execute(
                    "INSERT INTO beliefs (id, project_id, subject, relation, object, proposition, \
                     confidence, weight, source, provenance_id, contradicts_id, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![
                        new_id.clone(), self.project_id.clone(), subject, relation, object, proposition,
                        confidence, weight, source, provenance_id, contradicts_id, now.clone(), now
                    ],
                ).await.context("failed to insert belief")?;
                new_id
            }
        };

        self.get_belief(&id).await?.ok_or_else(|| anyhow::anyhow!("failed to retrieve upserted belief"))
    }

    /// Get a belief by ID.
    pub async fn get_belief(&self, id: &str) -> Result<Option<Belief>> {
        let mut rows = self.conn.query(
            "SELECT id, project_id, subject, relation, object, proposition, confidence, weight, source, provenance_id, contradicts_id, created_at, updated_at
             FROM beliefs WHERE id = ?1",
            params![id],
        ).await?;

        if let Some(row) = rows.next().await? {
            Ok(Some(Belief {
                id: row.get(0)?,
                project_id: row.get(1)?,
                subject: row.get(2)?,
                relation: row.get(3)?,
                object: row.get(4)?,
                proposition: row.get(5)?,
                confidence: row.get(6)?,
                weight: row.get(7)?,
                source: row.get(8)?,
                provenance_id: row.get(9)?,
                contradicts_id: row.get(10)?,
                created_at: parse_datetime(&row.get::<String>(11)?),
                updated_at: parse_datetime(&row.get::<String>(12)?),
            }))
        } else {
            Ok(None)
        }
    }

    /// List all beliefs for this project.
    pub async fn list_beliefs(&self, limit: usize) -> Result<Vec<Belief>> {
        let mut rows = self.conn.query(
            "SELECT id, project_id, subject, relation, object, proposition, confidence, weight, source, provenance_id, contradicts_id, created_at, updated_at
             FROM beliefs WHERE project_id = ?1 ORDER BY updated_at DESC LIMIT ?2",
            params![self.project_id.clone(), limit as i64],
        ).await?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(Belief {
                id: row.get(0)?,
                project_id: row.get(1)?,
                subject: row.get(2)?,
                relation: row.get(3)?,
                object: row.get(4)?,
                proposition: row.get(5)?,
                confidence: row.get(6)?,
                weight: row.get(7)?,
                source: row.get(8)?,
                provenance_id: row.get(9)?,
                contradicts_id: row.get(10)?,
                created_at: parse_datetime(&row.get::<String>(11)?),
                updated_at: parse_datetime(&row.get::<String>(12)?),
            });
        }
        Ok(results)
    }

    // ------------------------------------------------------------------
    // Checkpoint CRUD
    // ------------------------------------------------------------------

    /// Create or update a checkpoint.
    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_checkpoint(
        &self,
        workspace_id: Option<&str>,
        session_id: Option<&str>,
        task_id: Option<&str>,
        name: Option<&str>,
        state: &str,
        expires_at: Option<DateTime<Utc>>,
        kind: Option<&str>,
    ) -> Result<Checkpoint> {
        let now = Utc::now().to_rfc3339();
        let expires = expires_at.map(|dt| dt.to_rfc3339());

        // Try to find existing by workspace+task+name or session
        let existing_id = if let (Some(ws), Some(task), Some(n)) = (workspace_id, task_id, name) {
            let mut rows = self.conn.query(
                "SELECT id FROM checkpoints WHERE workspace_id = ?1 AND task_id = ?2 AND name = ?3 AND project_id = ?4",
                params![ws, task, n, self.project_id.clone()],
            ).await?;
            if let Some(row) = rows.next().await? {
                Some(row.get::<String>(0)?)
            } else {
                None
            }
        } else if let Some(sess) = session_id {
            let mut rows = self.conn.query(
                "SELECT id FROM checkpoints WHERE session_id = ?1 AND project_id = ?2 ORDER BY created_at DESC LIMIT 1",
                params![sess, self.project_id.clone()],
            ).await?;
            if let Some(row) = rows.next().await? {
                Some(row.get::<String>(0)?)
            } else {
                None
            }
        } else {
            None
        };

        let id = match existing_id {
            Some(id) => {
                self.conn.execute(
                    "UPDATE checkpoints SET workspace_id = ?1, session_id = ?2, task_id = ?3, name = ?4, \
                     state = ?5, expires_at = ?6, kind = ?7, created_at = ?8 WHERE id = ?9",
                    params![workspace_id, session_id, task_id, name, state, expires, kind, now, id.clone()],
                ).await.context("failed to update checkpoint")?;
                id
            }
            None => {
                let new_id = Uuid::new_v4().to_string();
                self.conn.execute(
                    "INSERT INTO checkpoints (id, project_id, workspace_id, session_id, task_id, name, \
                     state, created_at, expires_at, kind)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        new_id.clone(), self.project_id.clone(), workspace_id, session_id, task_id, name,
                        state, now, expires, kind
                    ],
                ).await.context("failed to insert checkpoint")?;
                new_id
            }
        };

        self.get_checkpoint_by_id(&id).await?.ok_or_else(|| anyhow::anyhow!("failed to retrieve upserted checkpoint"))
    }

    /// Get a checkpoint by ID.
    pub async fn get_checkpoint_by_id(&self, id: &str) -> Result<Option<Checkpoint>> {
        let mut rows = self.conn.query(
            "SELECT id, project_id, workspace_id, session_id, task_id, name, state, created_at, expires_at, kind
             FROM checkpoints WHERE id = ?1",
            params![id],
        ).await?;

        if let Some(row) = rows.next().await? {
            Ok(Some(Checkpoint {
                id: row.get(0)?,
                project_id: row.get(1)?,
                workspace_id: row.get::<Option<String>>(2).ok().flatten(),
                session_id: row.get::<Option<String>>(3).ok().flatten(),
                task_id: row.get::<Option<String>>(4).ok().flatten(),
                name: row.get::<Option<String>>(5).ok().flatten(),
                state: row.get(6)?,
                created_at: parse_datetime(&row.get::<String>(7)?),
                expires_at: row.get::<Option<String>>(8).ok().flatten().map(|s| parse_datetime(&s)),
                kind: row.get(9)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Get the most recent checkpoint for a session.
    pub async fn get_checkpoint(&self, session_id: &str) -> Result<Option<Checkpoint>> {
        let mut rows = self.conn.query(
            "SELECT id, project_id, workspace_id, session_id, task_id, name, state, created_at, expires_at, kind
             FROM checkpoints
             WHERE session_id = ?1 AND project_id = ?2
             ORDER BY created_at DESC LIMIT 1",
            params![session_id, self.project_id.clone()],
        ).await?;

        if let Some(row) = rows.next().await? {
            Ok(Some(Checkpoint {
                id: row.get(0)?,
                project_id: row.get(1)?,
                workspace_id: row.get::<Option<String>>(2).ok().flatten(),
                session_id: row.get::<Option<String>>(3).ok().flatten(),
                task_id: row.get::<Option<String>>(4).ok().flatten(),
                name: row.get::<Option<String>>(5).ok().flatten(),
                state: row.get(6)?,
                created_at: parse_datetime(&row.get::<String>(7)?),
                expires_at: row.get::<Option<String>>(8).ok().flatten().map(|s| parse_datetime(&s)),
                kind: row.get(9)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Get a checkpoint by workspace+task+name.
    pub async fn get_task_checkpoint(&self, workspace_id: &str, task_id: &str, name: &str) -> Result<Option<Checkpoint>> {
        let mut rows = self.conn.query(
            "SELECT id, project_id, workspace_id, session_id, task_id, name, state, created_at, expires_at, kind
             FROM checkpoints
             WHERE workspace_id = ?1 AND task_id = ?2 AND name = ?3 AND project_id = ?4
             ORDER BY created_at DESC LIMIT 1",
            params![workspace_id, task_id, name, self.project_id.clone()],
        ).await?;

        if let Some(row) = rows.next().await? {
            Ok(Some(Checkpoint {
                id: row.get(0)?,
                project_id: row.get(1)?,
                workspace_id: row.get::<Option<String>>(2).ok().flatten(),
                session_id: row.get::<Option<String>>(3).ok().flatten(),
                task_id: row.get::<Option<String>>(4).ok().flatten(),
                name: row.get::<Option<String>>(5).ok().flatten(),
                state: row.get(6)?,
                created_at: parse_datetime(&row.get::<String>(7)?),
                expires_at: row.get::<Option<String>>(8).ok().flatten().map(|s| parse_datetime(&s)),
                kind: row.get(9)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// List checkpoints for a task.
    pub async fn list_checkpoints(&self, workspace_id: &str, task_id: &str) -> Result<Vec<Checkpoint>> {
        let mut rows = self.conn.query(
            "SELECT id, project_id, workspace_id, session_id, task_id, name, state, created_at, expires_at, kind
             FROM checkpoints
             WHERE workspace_id = ?1 AND task_id = ?2 AND project_id = ?3
             ORDER BY created_at DESC",
            params![workspace_id, task_id, self.project_id.clone()],
        ).await?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(Checkpoint {
                id: row.get(0)?,
                project_id: row.get(1)?,
                workspace_id: row.get::<Option<String>>(2).ok().flatten(),
                session_id: row.get::<Option<String>>(3).ok().flatten(),
                task_id: row.get::<Option<String>>(4).ok().flatten(),
                name: row.get::<Option<String>>(5).ok().flatten(),
                state: row.get(6)?,
                created_at: parse_datetime(&row.get::<String>(7)?),
                expires_at: row.get::<Option<String>>(8).ok().flatten().map(|s| parse_datetime(&s)),
                kind: row.get(9)?,
            });
        }
        Ok(results)
    }

    /// Delete a checkpoint by workspace+task+name.
    pub async fn delete_checkpoint(&self, workspace_id: &str, task_id: &str, name: &str) -> Result<bool> {
        let affected = self.conn.execute(
            "DELETE FROM checkpoints WHERE workspace_id = ?1 AND task_id = ?2 AND name = ?3 AND project_id = ?4",
            params![workspace_id, task_id, name, self.project_id.clone()],
        ).await?;
        Ok(affected > 0)
    }

    // ------------------------------------------------------------------
    // Utilities
    // ------------------------------------------------------------------

    /// Get the filesystem path for this project's conversations DB.
    pub fn db_path(project_id: &str) -> PathBuf {
        // Sanitize: only alphanumeric + hyphens/underscores
        let sanitized: String = project_id
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        assert!(!sanitized.is_empty(), "Invalid project_id: must contain at least one alphanumeric character, hyphen, or underscore");

        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        let mut path = PathBuf::from(home);
        path.push(".xavier");
        path.push(CONVERSATIONS_DIR);

        let mut db_file = path.clone();
        db_file.push(format!("{}.db", sanitized));

        // Verify that the resolved path is within the expected directory
        if let Ok(canonical_base) = std::fs::canonicalize(&path) {
            // Note: we only check if the base exists. The file itself might not exist yet.
            // Since we sanitized the filename, joining it to a canonical path should be safe.
            let resolved = canonical_base.join(format!("{}.db", sanitized));
            assert!(resolved.starts_with(&canonical_base), "Path escape detected");
        }

        db_file
    }

    /// Migrate legacy JSON session files to the database.
    pub async fn migrate_legacy_sessions(&self, sessions_dir: &Path) -> Result<usize> {
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

            let title = thread_data["title"].as_str();
            let started_at = thread_data["created_at"].as_str().unwrap_or_default();
            let updated_at = thread_data["updated_at"].as_str().unwrap_or_default();
            let last_preview = thread_data["last_preview"].as_str();

            self.conn.execute(
                "INSERT INTO conversation_threads (id, project_id, title, started_at, updated_at, last_preview)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id.clone(), self.project_id.clone(), title, started_at, updated_at, last_preview],
            ).await?;

            if let Some(messages) = thread_data["messages"].as_array() {
                for msg in messages {
                    let msg_id = msg["id"].as_str().unwrap_or_default().to_string();
                    let role = msg["role"].as_str().unwrap_or("user");
                    let content = msg["plain_text"].as_str().unwrap_or("");
                    let created_at = msg["created_at"].as_str().unwrap_or_default();
                    let xui_json = msg["xui_json"].as_str();
                    let openui_lang = msg["openui_lang"].as_str();
                    let metadata = msg["metadata"].to_string();

                    self.conn.execute(
                        "INSERT INTO conversation_messages (id, thread_id, role, content, xui_json, openui_lang, metadata, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![msg_id, id.clone(), role, content, xui_json, openui_lang, metadata, created_at],
                    ).await?;
                }
            }

            count += 1;
        }

        Ok(count)
    }
}

/// Parse an RFC 3339 datetime string.
fn parse_datetime(s: &str) -> DateTime<Utc> {
    s.parse::<DateTime<Utc>>().unwrap_or_else(|_| Utc::now())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_db() -> ConversationsDb {
        let db = ConversationsDb::open_in_memory("test-project").await.unwrap();
        db.create_schema().await.unwrap();
        db
    }

    #[tokio::test]
    async fn test_create_and_get_thread() {
        let db = setup_db().await;
        let thread = db.create_thread(Some("Test Thread"), Some("gpt-4"), Some("chat")).await.unwrap();
        let fetched = db.get_thread(&thread.id).await.unwrap().expect("thread should exist");
        assert_eq!(fetched.title.unwrap(), "Test Thread");
        assert_eq!(fetched.model.unwrap(), "gpt-4");
        assert_eq!(fetched.source.unwrap(), "chat");
    }

    #[tokio::test]
    async fn test_get_nonexistent_thread() {
        let db = setup_db().await;
        let result = db.get_thread("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_threads() {
        let db = setup_db().await;
        db.create_thread(Some("Thread A"), None, None).await.unwrap();
        db.create_thread(Some("Thread B"), None, None).await.unwrap();
        let threads = db.list_threads(10).await.unwrap();
        assert_eq!(threads.len(), 2);
    }

    #[tokio::test]
    async fn test_store_and_get_messages() {
        let db = setup_db().await;
        let thread = db.create_thread(Some("Test"), None, None).await.unwrap();

        db.store_message(&thread.id, "user", "Hello!", None, None, None, None, Some(10)).await.unwrap();
        db.store_message(&thread.id, "assistant", "Hi there!", None, None, None, None, Some(15)).await.unwrap();

        let messages = db.get_thread_messages(&thread.id).await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
    }

    #[tokio::test]
    async fn test_record_and_list_deductions() {
        let db = setup_db().await;
        db.record_deduction("User prefers Rust over Python", 0.8, Some("preference"), None).await.unwrap();
        db.record_deduction("User works at SWAL", 0.9, Some("work"), None).await.unwrap();

        let deductions = db.list_deductions(10).await.unwrap();
        assert_eq!(deductions.len(), 2);
        assert!(deductions[0].deduction.contains("SWAL") || deductions[1].deduction.contains("SWAL"));
    }

    #[tokio::test]
    async fn test_upsert_belief_create_then_update() {
        let db = setup_db().await;

        // Create
        let belief = db.upsert_belief("xavier", None, None, Some("is a memory system"), 0.7, 0.7, Some("chat"), None, None).await.unwrap();
        assert_eq!(belief.subject, "xavier");

        // Update — same subject and proposition should upsert
        let updated = db.upsert_belief("xavier", None, None, Some("is a memory system"), 0.95, 0.95, Some("code"), None, None).await.unwrap();
        assert_eq!(updated.id, belief.id);
        assert!((updated.confidence - 0.95).abs() < 0.01);

        // Should only be 1 belief
        let beliefs = db.list_beliefs(10).await.unwrap();
        assert_eq!(beliefs.len(), 1);
    }

    #[tokio::test]
    async fn test_create_and_get_checkpoint() {
        let db = setup_db().await;
        let cp = db.upsert_checkpoint(None, Some("sess-1"), None, None, "{\"state\": \"idle\"}", None, Some("work")).await.unwrap();
        assert_eq!(cp.state, "{\"state\": \"idle\"}");

        let fetched = db.get_checkpoint("sess-1").await.unwrap().expect("checkpoint should exist");
        assert_eq!(fetched.state, cp.state);
        assert_eq!(fetched.kind.unwrap(), "work");
    }

    #[tokio::test]
    async fn test_get_nonexistent_checkpoint() {
        let db = setup_db().await;
        let result = db.get_checkpoint("no-such-session").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_store_message_updates_thread_timestamp() {
        let db = setup_db().await;
        let thread = db.create_thread(Some("Test"), None, None).await.unwrap();
        let original_updated = thread.updated_at;

        // Small delay to ensure timestamp changes
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        db.store_message(&thread.id, "user", "msg", None, None, None, None, None).await.unwrap();

        // Re-fetch the thread
        let fetched = db.get_thread(&thread.id).await.unwrap().unwrap();
        assert!(fetched.updated_at > original_updated);
    }

    #[tokio::test]
    async fn test_beliefs_are_per_project() {
        let db1 = ConversationsDb::open_in_memory("proj-a").await.unwrap();
        db1.create_schema().await.unwrap();
        db1.upsert_belief("rust", None, None, Some("is great"), 0.9, 0.9, None, None, None).await.unwrap();

        let db2 = ConversationsDb::open_in_memory("proj-b").await.unwrap();
        db2.create_schema().await.unwrap();
        db2.upsert_belief("python", None, None, Some("is great"), 0.8, 0.8, None, None, None).await.unwrap();

        assert_eq!(db1.list_beliefs(10).await.unwrap().len(), 1);
        assert_eq!(db2.list_beliefs(10).await.unwrap().len(), 1);
    }

    #[test]
    fn test_db_path_traversal() {
        let malicious_id = "../../etc/passwd";
        let path = ConversationsDb::db_path(malicious_id);

        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        let mut expected_base = PathBuf::from(home);
        expected_base.push(".xavier");
        expected_base.push(CONVERSATIONS_DIR);

        assert!(path.starts_with(&expected_base), "Path traversal detected: {:?}", path);
        assert!(!path.to_string_lossy().contains(".."), "Path contains '..': {:?}", path);
        assert!(path.to_string_lossy().contains("etcpasswd.db"), "Sanitization failed: {:?}", path);
    }
}
