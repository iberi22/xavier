//! Private conversations database.
//!
//! Stored at `~/.xavier/conversations/{project_id}.db`, this DB holds
//! AI conversation threads, messages, deductions, agent beliefs, and
//! session checkpoints. It is never committed to a repository.

use std::path::PathBuf;

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
    pub created_at: DateTime<Utc>,
    pub tokens: Option<i64>,
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
    pub proposition: String,
    pub confidence: f64,
    pub source: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A session checkpoint for resuming interrupted work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
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
                model TEXT,
                source TEXT
            );

            CREATE TABLE IF NOT EXISTS conversation_messages (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                tool_calls TEXT,
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

            CREATE TABLE IF NOT EXISTS agent_beliefs (
                id TEXT PRIMARY KEY,
                project_id TEXT,
                subject TEXT NOT NULL,
                proposition TEXT NOT NULL,
                confidence REAL DEFAULT 0.0,
                source TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS session_checkpoints (
                id TEXT PRIMARY KEY,
                project_id TEXT,
                session_id TEXT,
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
            model: model.map(|s| s.to_string()),
            source: source.map(|s| s.to_string()),
        })
    }

    /// Get a thread by ID.
    pub async fn get_thread(&self, thread_id: &str) -> Result<Option<Thread>> {
        let mut rows = self.conn.query(
            "SELECT id, project_id, title, started_at, updated_at, model, source
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
                model: row.get(5)?,
                source: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// List all threads for a project.
    pub async fn list_threads(&self, limit: usize) -> Result<Vec<Thread>> {
        let mut rows = self.conn.query(
            "SELECT id, project_id, title, started_at, updated_at, model, source
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
                model: row.get(5)?, source: row.get(6)?,
            });
        }
        Ok(results)
    }

    // ------------------------------------------------------------------
    // Message CRUD
    // ------------------------------------------------------------------

    /// Store a message in a thread.
    pub async fn store_message(
        &self, thread_id: &str, role: &str, content: &str,
        tool_calls: Option<&str>, tokens: Option<i64>,
    ) -> Result<Message> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO conversation_messages (id, thread_id, role, content, tool_calls, created_at, tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id.clone(), thread_id, role, content, tool_calls, now.clone(), tokens],
        ).await.context("failed to store message")?;

        // Update thread's updated_at timestamp
        let _ = self.conn.execute(
            "UPDATE conversation_threads SET updated_at = ?1 WHERE id = ?2",
            params![now, thread_id],
        ).await;

        Ok(Message {
            id, thread_id: thread_id.to_string(),
            role: role.to_string(), content: content.to_string(),
            tool_calls: tool_calls.map(|s| s.to_string()),
            created_at: Utc::now(), tokens,
        })
    }

    /// Get all messages for a thread, ordered by creation time.
    pub async fn get_thread_messages(&self, thread_id: &str) -> Result<Vec<Message>> {
        let mut rows = self.conn.query(
            "SELECT id, thread_id, role, content, tool_calls, created_at, tokens
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
                created_at: parse_datetime(&row.get::<String>(5)?),
                tokens: row.get(6)?,
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

    /// Upsert a belief (create or update by subject+proposition).
    pub async fn upsert_belief(
        &self, subject: &str, proposition: &str, confidence: f64, source: Option<&str>,
    ) -> Result<Belief> {
        let now = Utc::now().to_rfc3339();

        // Check if a belief with this subject and proposition already exists
        let mut rows = self.conn.query(
            "SELECT id FROM agent_beliefs WHERE subject = ?1 AND proposition = ?2 AND project_id = ?3",
            params![subject, proposition, self.project_id.clone()],
        ).await?;

        let existing_id = if let Some(row) = rows.next().await? {
            Some(row.get::<String>(0)?)
        } else {
            None
        };

        let id = match existing_id {
            Some(id) => {
                self.conn.execute(
                    "UPDATE agent_beliefs SET confidence = ?1, source = ?2, updated_at = ?3 WHERE id = ?4",
                    params![confidence, source, now, id.clone()],
                ).await.context("failed to update belief")?;
                id
            }
            None => {
                let new_id = Uuid::new_v4().to_string();
                self.conn.execute(
                    "INSERT INTO agent_beliefs (id, project_id, subject, proposition, confidence, source, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![new_id.clone(), self.project_id.clone(), subject, proposition, confidence, source, now.clone(), now],
                ).await.context("failed to insert belief")?;
                new_id
            }
        };

        let mut rows = self.conn.query(
            "SELECT id, project_id, subject, proposition, confidence, source, created_at, updated_at
             FROM agent_beliefs WHERE id = ?1",
            params![id],
        ).await?;

        if let Some(row) = rows.next().await? {
            Ok(Belief {
                id: row.get(0)?, project_id: row.get(1)?,
                subject: row.get(2)?, proposition: row.get(3)?,
                confidence: row.get(4)?, source: row.get(5)?,
                created_at: parse_datetime(&row.get::<String>(6)?),
                updated_at: parse_datetime(&row.get::<String>(7)?),
            })
        } else {
            anyhow::bail!("Failed to retrieve upserted belief")
        }
    }

    /// List all beliefs for this project.
    pub async fn list_beliefs(&self, limit: usize) -> Result<Vec<Belief>> {
        let mut rows = self.conn.query(
            "SELECT id, project_id, subject, proposition, confidence, source, created_at, updated_at
             FROM agent_beliefs WHERE project_id = ?1 ORDER BY updated_at DESC LIMIT ?2",
            params![self.project_id.clone(), limit as i64],
        ).await?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(Belief {
                id: row.get(0)?, project_id: row.get(1)?,
                subject: row.get(2)?, proposition: row.get(3)?,
                confidence: row.get(4)?, source: row.get(5)?,
                created_at: parse_datetime(&row.get::<String>(6)?),
                updated_at: parse_datetime(&row.get::<String>(7)?),
            });
        }
        Ok(results)
    }

    // ------------------------------------------------------------------
    // Checkpoint CRUD
    // ------------------------------------------------------------------

    /// Create a session checkpoint.
    pub async fn create_checkpoint(
        &self, session_id: &str, state: &str,
        expires_at: Option<DateTime<Utc>>, kind: Option<&str>,
    ) -> Result<Checkpoint> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let expires = expires_at.map(|dt| dt.to_rfc3339());
        self.conn.execute(
            "INSERT INTO session_checkpoints (id, project_id, session_id, state, created_at, expires_at, kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id.clone(), self.project_id.clone(), session_id, state, now, expires, kind],
        ).await.context("failed to create checkpoint")?;
        Ok(Checkpoint {
            id, project_id: Some(self.project_id.clone()),
            session_id: Some(session_id.to_string()),
            state: state.to_string(),
            created_at: Utc::now(), expires_at, kind: kind.map(|s| s.to_string()),
        })
    }

    /// Get the most recent checkpoint for a session.
    pub async fn get_checkpoint(&self, session_id: &str) -> Result<Option<Checkpoint>> {
        let mut rows = self.conn.query(
            "SELECT id, project_id, session_id, state, created_at, expires_at, kind
             FROM session_checkpoints
             WHERE session_id = ?1 AND project_id = ?2
             ORDER BY created_at DESC LIMIT 1",
            params![session_id, self.project_id.clone()],
        ).await?;

        if let Some(row) = rows.next().await? {
            Ok(Some(Checkpoint {
                id: row.get(0)?, project_id: row.get(1)?,
                session_id: row.get(2)?, state: row.get(3)?,
                created_at: parse_datetime(&row.get::<String>(4)?),
                expires_at: row.get::<Option<String>>(5)?.map(|s| parse_datetime(&s)),
                kind: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
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

        db.store_message(&thread.id, "user", "Hello!", None, Some(10)).await.unwrap();
        db.store_message(&thread.id, "assistant", "Hi there!", None, Some(15)).await.unwrap();

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
        let belief = db.upsert_belief("xavier", "is a memory system", 0.7, Some("chat")).await.unwrap();
        assert_eq!(belief.subject, "xavier");

        // Update — same subject and proposition should upsert
        let updated = db.upsert_belief("xavier", "is a memory system", 0.95, Some("code")).await.unwrap();
        assert_eq!(updated.id, belief.id);
        assert!((updated.confidence - 0.95).abs() < 0.01);

        // Should only be 1 belief
        let beliefs = db.list_beliefs(10).await.unwrap();
        assert_eq!(beliefs.len(), 1);
    }

    #[tokio::test]
    async fn test_create_and_get_checkpoint() {
        let db = setup_db().await;
        let cp = db.create_checkpoint("sess-1", "{\"state\": \"idle\"}", None, Some("work")).await.unwrap();
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

        db.store_message(&thread.id, "user", "msg", None, None).await.unwrap();

        // Re-fetch the thread
        let fetched = db.get_thread(&thread.id).await.unwrap().unwrap();
        assert!(fetched.updated_at > original_updated);
    }

    #[tokio::test]
    async fn test_beliefs_are_per_project() {
        let db1 = ConversationsDb::open_in_memory("proj-a").await.unwrap();
        db1.create_schema().await.unwrap();
        db1.upsert_belief("rust", "is great", 0.9, None).await.unwrap();

        let db2 = ConversationsDb::open_in_memory("proj-b").await.unwrap();
        db2.create_schema().await.unwrap();
        db2.upsert_belief("python", "is great", 0.8, None).await.unwrap();

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
