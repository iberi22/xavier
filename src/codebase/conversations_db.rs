// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Private conversations database.
//!
//! Stored at `~/.xavier/conversations/{project_id}.db`, this DB holds
//! AI conversation threads, messages, deductions, agent beliefs, and
//! session checkpoints. It is never committed to a repository.

use std::path::{Path, PathBuf};

use crate::codebase::connection_manager::ConnectionManager;
use crate::codebase::validate_project_id;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
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
    project_id: String,
    full_project_id: String,
    schema_initialized: OnceCell<()>,
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
        })
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
        })
    }

    /// Ensure the schema is created (lazy initialization).
    async fn ensure_schema(&self) -> Result<()> {
        self.schema_initialized
            .get_or_try_init(|| async { self.create_schema().await })
            .await?;
        Ok(())
    }

    /// Create all tables for the conversations database.
    pub async fn create_schema(&self) -> Result<()> {
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
                );",
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
        let title_c = title.map(|s| s.to_string());
        let model_c = model.map(|s| s.to_string());
        let source_c = source.map(|s| s.to_string());

        let id_for_return = id.clone();
        let title_for_return = title_c.clone();
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
        let tid = thread_id.to_string();
        ConnectionManager::global()
            .with_conn(&self.full_project_id, move |conn| {
                let mut stmt = conn.prepare(
                "SELECT id, project_id, title, started_at, updated_at, last_preview, model, source
                 FROM conversation_threads WHERE id = ?1",
            )?;

                let mut rows = stmt.query(params![tid])?;

                if let Some(row) = rows.next()? {
                    Ok(Some(Thread {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        title: row.get(2)?,
                        started_at: parse_datetime(&row.get::<_, String>(3)?),
                        updated_at: parse_datetime(&row.get::<_, String>(4)?),
                        last_preview: row.get(5)?,
                        model: row.get(6)?,
                        source: row.get(7)?,
                    }))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    /// List all threads for a project.
    pub async fn list_threads(&self, limit: usize) -> Result<Vec<Thread>> {
        self.ensure_schema().await?;
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
                    results.push(Thread {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        title: row.get(2)?,
                        started_at: parse_datetime(&row.get::<_, String>(3)?),
                        updated_at: parse_datetime(&row.get::<_, String>(4)?),
                        last_preview: row.get(5)?,
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
        let content_c = content.to_string();
        let tool_calls_c = tool_calls.map(|s| s.to_string());
        let openui_lang_c = openui_lang.map(|s| s.to_string());
        let xui_json_c = xui_json.map(|s| s.to_string());
        let metadata_c = metadata.map(|s| s.to_string());

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
            let summary = crate::memory::episodic::summarize_session_extractive(&messages);
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
        let preview_c = preview.to_string();
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
        ConnectionManager::global().with_conn(&self.full_project_id, move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, thread_id, role, content, tool_calls, openui_lang, xui_json, metadata, created_at, tokens
                 FROM conversation_messages
                 WHERE thread_id = ?1
                 ORDER BY created_at ASC",
            )?;

            let mut rows = stmt.query(params![tid])?;

            let mut results = Vec::new();
            while let Some(row) = rows.next()? {
                results.push(Message {
                    id: row.get(0)?, thread_id: row.get(1)?,
                    role: row.get(2)?, content: row.get(3)?,
                    tool_calls: row.get(4)?,
                    openui_lang: row.get(5)?,
                    xui_json: row.get(6)?,
                    metadata: row.get(7)?,
                    created_at: parse_datetime(&row.get::<_, String>(8)?),
                    tokens: row.get(9)?,
                });
            }
            Ok(results)
        }).await
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
        let deduction_c = deduction_text.to_string();
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
        let pid = self.project_id.clone();
        ConnectionManager::global().with_conn(&self.full_project_id, move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, source_thread, deduction, confidence, created_at, last_accessed, category
                 FROM deductions WHERE project_id = ?1 ORDER BY created_at DESC LIMIT ?2",
            )?;

            let mut rows = stmt.query(params![pid, limit as i64])?;

            let mut results = Vec::new();
            while let Some(row) = rows.next()? {
                results.push(Deduction {
                    id: row.get(0)?, project_id: row.get(1)?,
                    source_thread: row.get(2)?, deduction: row.get(3)?,
                    confidence: row.get(4)?,
                    created_at: parse_datetime(&row.get::<_, String>(5)?),
                    last_accessed: row.get::<_, Option<String>>(6)?.map(|s| parse_datetime(&s)),
                    category: row.get(7)?,
                });
            }
            Ok(results)
        }).await
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
        let subject_c = subject.to_string();
        let relation_c = relation.map(|s| s.to_string());
        let object_c = object.map(|s| s.to_string());
        let proposition_c = proposition.map(|s| s.to_string());
        let source_c = source.map(|s| s.to_string());
        let provenance_id_c = provenance_id.map(|s| s.to_string());
        let contradicts_id_c = contradicts_id.map(|s| s.to_string());
        let now_c = now.clone();

        let id = ConnectionManager::global().with_conn(&self.full_project_id, move |conn| {
            // Check if a belief with this subject and relation/object or proposition already exists
            let existing_id = if let (Some(ref rel), Some(ref obj)) = (&relation_c, &object_c) {
                conn.query_row(
                    "SELECT id FROM beliefs WHERE subject = ?1 AND relation = ?2 AND object = ?3 AND project_id = ?4",
                    params![subject_c.clone(), rel, obj, pid.clone()],
                    |row| row.get::<_, String>(0)
                ).ok()
            } else if let Some(ref prop) = &proposition_c {
                conn.query_row(
                    "SELECT id FROM beliefs WHERE subject = ?1 AND proposition = ?2 AND project_id = ?3",
                    params![subject_c.clone(), prop, pid.clone()],
                    |row| row.get::<_, String>(0)
                ).ok()
            } else {
                conn.query_row(
                    "SELECT id FROM beliefs WHERE subject = ?1 AND project_id = ?2 LIMIT 1",
                    params![subject_c.clone(), pid.clone()],
                    |row| row.get::<_, String>(0)
                ).ok()
            };

            match existing_id {
                Some(id) => {
                    conn.execute(
                        "UPDATE beliefs SET relation = ?1, object = ?2, proposition = ?3, confidence = ?4, \
                         weight = ?5, source = ?6, provenance_id = ?7, contradicts_id = ?8, updated_at = ?9 WHERE id = ?10",
                        params![
                            relation_c, object_c, proposition_c, confidence,
                            weight, source_c, provenance_id_c, contradicts_id_c,
                            now_c.clone(), id
                        ],
                    ).context("failed to update belief")?;
                    Ok(id)
                }
                None => {
                    let new_id = Uuid::new_v4().to_string();
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
        ConnectionManager::global().with_conn(&self.full_project_id, move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, subject, relation, object, proposition, confidence, weight, source, provenance_id, contradicts_id, created_at, updated_at
                 FROM beliefs WHERE id = ?1",
            )?;

            let mut rows = stmt.query(params![bid])?;

            if let Some(row) = rows.next()? {
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
        let pid = self.project_id.clone();
        ConnectionManager::global().with_conn(&self.full_project_id, move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, subject, relation, object, proposition, confidence, weight, source, provenance_id, contradicts_id, created_at, updated_at
                 FROM beliefs WHERE project_id = ?1 ORDER BY updated_at DESC LIMIT ?2",
            )?;

            let mut rows = stmt.query(params![pid, limit as i64])?;

            let mut results = Vec::new();
            while let Some(row) = rows.next()? {
                results.push(Belief {
                    id: row.get(0)?, project_id: row.get(1)?,
                    subject: row.get(2)?, relation: row.get(3)?,
                    object: row.get(4)?, proposition: row.get(5)?,
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
        ConnectionManager::global().with_conn(&self.full_project_id, move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, workspace_id, session_id, task_id, name, state, created_at, expires_at, kind
                 FROM checkpoints WHERE project_id = ?1 AND state = 'active' ORDER BY created_at DESC LIMIT ?2",
            )?;

            let mut rows = stmt.query(params![pid, limit as i64])?;

            let mut results = Vec::new();
            while let Some(row) = rows.next()? {
                results.push(Checkpoint {
                    id: row.get(0)?, project_id: row.get(1)?,
                    workspace_id: row.get(2)?, session_id: row.get(3)?,
                    task_id: row.get(4)?, name: row.get(5)?,
                    state: row.get(6)?,
                    created_at: parse_datetime(&row.get::<_, String>(7)?),
                    expires_at: row.get::<_, Option<String>>(8)?.map(|s| parse_datetime(&s)),
                    kind: row.get(9)?,
                });
            }
            Ok(results)
        }).await
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
            let title_c = title.clone();
            let started_at_c = started_at.clone();
            let updated_at_c = updated_at.clone();
            let last_preview_c = last_preview.clone();
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
        }
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
}
