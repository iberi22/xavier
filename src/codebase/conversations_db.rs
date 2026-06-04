//! Private conversations database.
//!
//! Stored at `~/.xavier/conversations/{project_id}.db`, this DB holds
//! AI conversation threads, messages, deductions, agent beliefs, and
//! session checkpoints. It is never committed to a repository.

use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::codebase::connection_manager::ConnectionManager;

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
    pool: Arc<r2d2::Pool<SqliteConnectionManager>>,
    project_id: String,
}

impl ConversationsDb {
    /// Open (or create) the conversations database for `project_id`.
    ///
    /// The database is stored at `~/.xavier/conversations/{project_id}.db`.
    pub fn open(project_id: &str) -> Result<Self> {
        let manager = ConnectionManager::global();
        let conv_project_id = format!("conv_{}", project_id);
        manager.connect(&conv_project_id, "")?;
        let pool = manager.get_pool(&conv_project_id)?;

        Ok(Self { pool, project_id: project_id.to_string() })
    }

    /// Open an in-memory conversations database (for testing).
    pub fn open_in_memory(project_id: &str) -> Result<Self> {
        let manager = SqliteConnectionManager::memory();
        let pool = r2d2::Pool::new(manager)?;
        Ok(Self { pool: Arc::new(pool), project_id: project_id.to_string() })
    }

    /// Create all tables for the conversations database.
    pub fn create_schema(&self) -> Result<()> {
        let conn = self.connection()?;
        conn.execute_batch(
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
        ).context("failed to create conversations schema")?;
        Ok(())
    }

    /// Return a pooled connection.
    pub fn connection(&self) -> Result<PooledConnection<SqliteConnectionManager>> {
        self.pool.get().context("failed to get connection from pool")
    }

    /// Return the project ID.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    // ------------------------------------------------------------------
    // Thread CRUD
    // ------------------------------------------------------------------

    /// Create a new conversation thread.
    pub fn create_thread(&self, title: Option<&str>, model: Option<&str>, source: Option<&str>) -> Result<Thread> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO conversation_threads (id, project_id, title, started_at, updated_at, model, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, self.project_id, title, now, now, model, source],
        ).context("failed to create thread")?;
        Ok(Thread {
            id, project_id: Some(self.project_id.clone()),
            title: title.map(|s| s.to_string()),
            started_at: Utc::now(), updated_at: Utc::now(),
            model: model.map(|s| s.to_string()),
            source: source.map(|s| s.to_string()),
        })
    }

    /// Get a thread by ID.
    pub fn get_thread(&self, thread_id: &str) -> Result<Option<Thread>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, project_id, title, started_at, updated_at, model, source
             FROM conversation_threads WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![thread_id], |row| {
            Ok(Thread {
                id: row.get(0)?,
                project_id: row.get(1)?,
                title: row.get(2)?,
                started_at: parse_datetime(&row.get::<_, String>(3)?),
                updated_at: parse_datetime(&row.get::<_, String>(4)?),
                model: row.get(5)?,
                source: row.get(6)?,
            })
        })?;
        match rows.next() {
            Some(Ok(t)) => Ok(Some(t)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// List all threads for a project.
    pub fn list_threads(&self, limit: usize) -> Result<Vec<Thread>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, project_id, title, started_at, updated_at, model, source
             FROM conversation_threads
             WHERE project_id = ?1
             ORDER BY updated_at DESC
             LIMIT ?2",
        )?;
        let results = stmt
            .query_map(params![self.project_id, limit as i64], |row| {
                Ok(Thread {
                    id: row.get(0)?, project_id: row.get(1)?,
                    title: row.get(2)?,
                    started_at: parse_datetime(&row.get::<_, String>(3)?),
                    updated_at: parse_datetime(&row.get::<_, String>(4)?),
                    model: row.get(5)?, source: row.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to list threads")?;
        Ok(results)
    }

    // ------------------------------------------------------------------
    // Message CRUD
    // ------------------------------------------------------------------

    /// Store a message in a thread.
    pub fn store_message(
        &self, thread_id: &str, role: &str, content: &str,
        tool_calls: Option<&str>, tokens: Option<i64>,
    ) -> Result<Message> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO conversation_messages (id, thread_id, role, content, tool_calls, created_at, tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, thread_id, role, content, tool_calls, now, tokens],
        ).context("failed to store message")?;

        // Update thread's updated_at timestamp
        conn.execute(
            "UPDATE conversation_threads SET updated_at = ?1 WHERE id = ?2",
            params![now, thread_id],
        ).ok();

        Ok(Message {
            id, thread_id: thread_id.to_string(),
            role: role.to_string(), content: content.to_string(),
            tool_calls: tool_calls.map(|s| s.to_string()),
            created_at: Utc::now(), tokens,
        })
    }

    /// Get all messages for a thread, ordered by creation time.
    pub fn get_thread_messages(&self, thread_id: &str) -> Result<Vec<Message>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, thread_id, role, content, tool_calls, created_at, tokens
             FROM conversation_messages
             WHERE thread_id = ?1
             ORDER BY created_at ASC",
        )?;
        let results = stmt
            .query_map(params![thread_id], |row| {
                Ok(Message {
                    id: row.get(0)?, thread_id: row.get(1)?,
                    role: row.get(2)?, content: row.get(3)?,
                    tool_calls: row.get(4)?,
                    created_at: parse_datetime(&row.get::<_, String>(5)?),
                    tokens: row.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to get thread messages")?;
        Ok(results)
    }

    // ------------------------------------------------------------------
    // Deduction CRUD
    // ------------------------------------------------------------------

    /// Record a deduction.
    pub fn record_deduction(
        &self, deduction_text: &str, confidence: f64,
        category: Option<&str>, source_thread: Option<&str>,
    ) -> Result<Deduction> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO deductions (id, project_id, source_thread, deduction, confidence, created_at, last_accessed, category)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, self.project_id, source_thread, deduction_text, confidence, now, now, category],
        ).context("failed to record deduction")?;
        Ok(Deduction {
            id, project_id: Some(self.project_id.clone()),
            source_thread: source_thread.map(|s| s.to_string()),
            deduction: deduction_text.to_string(), confidence,
            created_at: Utc::now(), last_accessed: Some(Utc::now()),
            category: category.map(|s| s.to_string()),
        })
    }

    /// List deductions for this project.
    pub fn list_deductions(&self, limit: usize) -> Result<Vec<Deduction>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, project_id, source_thread, deduction, confidence, created_at, last_accessed, category
             FROM deductions WHERE project_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let results = stmt
            .query_map(params![self.project_id, limit as i64], |row| {
                Ok(Deduction {
                    id: row.get(0)?, project_id: row.get(1)?,
                    source_thread: row.get(2)?, deduction: row.get(3)?,
                    confidence: row.get(4)?,
                    created_at: parse_datetime(&row.get::<_, String>(5)?),
                    last_accessed: row.get::<_, Option<String>>(6)?.map(|s| parse_datetime(&s)),
                    category: row.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to list deductions")?;
        Ok(results)
    }

    // ------------------------------------------------------------------
    // Belief CRUD
    // ------------------------------------------------------------------

    /// Upsert a belief (create or update by subject+proposition).
    pub fn upsert_belief(
        &self, subject: &str, proposition: &str, confidence: f64, source: Option<&str>,
    ) -> Result<Belief> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connection()?;

        // Check if a belief with this subject and proposition already exists
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM agent_beliefs WHERE subject = ?1 AND proposition = ?2 AND project_id = ?3",
                params![subject, proposition, self.project_id],
                |row| row.get(0),
            ).ok();

        let id = match existing {
            Some(existing_id) => {
                conn.execute(
                    "UPDATE agent_beliefs SET confidence = ?1, source = ?2, updated_at = ?3 WHERE id = ?4",
                    params![confidence, source, now, existing_id],
                ).context("failed to update belief")?;
                existing_id
            }
            None => {
                let new_id = Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO agent_beliefs (id, project_id, subject, proposition, confidence, source, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![new_id, self.project_id, subject, proposition, confidence, source, now, now],
                ).context("failed to insert belief")?;
                new_id
            }
        };

        let mut stmt = conn.prepare(
            "SELECT id, project_id, subject, proposition, confidence, source, created_at, updated_at
             FROM agent_beliefs WHERE id = ?1",
        )?;
        let belief = stmt.query_row(params![id], |row| {
            Ok(Belief {
                id: row.get(0)?, project_id: row.get(1)?,
                subject: row.get(2)?, proposition: row.get(3)?,
                confidence: row.get(4)?, source: row.get(5)?,
                created_at: parse_datetime(&row.get::<_, String>(6)?),
                updated_at: parse_datetime(&row.get::<_, String>(7)?),
            })
        })?;
        Ok(belief)
    }

    /// List all beliefs for this project.
    pub fn list_beliefs(&self, limit: usize) -> Result<Vec<Belief>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, project_id, subject, proposition, confidence, source, created_at, updated_at
             FROM agent_beliefs WHERE project_id = ?1 ORDER BY updated_at DESC LIMIT ?2",
        )?;
        let results = stmt
            .query_map(params![self.project_id, limit as i64], |row| {
                Ok(Belief {
                    id: row.get(0)?, project_id: row.get(1)?,
                    subject: row.get(2)?, proposition: row.get(3)?,
                    confidence: row.get(4)?, source: row.get(5)?,
                    created_at: parse_datetime(&row.get::<_, String>(6)?),
                    updated_at: parse_datetime(&row.get::<_, String>(7)?),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to list beliefs")?;
        Ok(results)
    }

    // ------------------------------------------------------------------
    // Checkpoint CRUD
    // ------------------------------------------------------------------

    /// Create a session checkpoint.
    pub fn create_checkpoint(
        &self, session_id: &str, state: &str,
        expires_at: Option<DateTime<Utc>>, kind: Option<&str>,
    ) -> Result<Checkpoint> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let expires = expires_at.map(|dt| dt.to_rfc3339());
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO session_checkpoints (id, project_id, session_id, state, created_at, expires_at, kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, self.project_id, session_id, state, now, expires, kind],
        ).context("failed to create checkpoint")?;
        Ok(Checkpoint {
            id, project_id: Some(self.project_id.clone()),
            session_id: Some(session_id.to_string()),
            state: state.to_string(),
            created_at: Utc::now(), expires_at, kind: kind.map(|s| s.to_string()),
        })
    }

    /// Get the most recent checkpoint for a session.
    pub fn get_checkpoint(&self, session_id: &str) -> Result<Option<Checkpoint>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, project_id, session_id, state, created_at, expires_at, kind
             FROM session_checkpoints
             WHERE session_id = ?1 AND project_id = ?2
             ORDER BY created_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![session_id, self.project_id], |row| {
            Ok(Checkpoint {
                id: row.get(0)?, project_id: row.get(1)?,
                session_id: row.get(2)?, state: row.get(3)?,
                created_at: parse_datetime(&row.get::<_, String>(4)?),
                expires_at: row.get::<_, Option<String>>(5)?.map(|s| parse_datetime(&s)),
                kind: row.get(6)?,
            })
        })?;
        match rows.next() {
            Some(Ok(c)) => Ok(Some(c)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
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

    fn setup_db() -> ConversationsDb {
        let db = ConversationsDb::open_in_memory("test-project").unwrap();
        db.create_schema().unwrap();
        db
    }

    #[test]
    fn test_create_and_get_thread() {
        let db = setup_db();
        let thread = db.create_thread(Some("Test Thread"), Some("gpt-4"), Some("chat")).unwrap();
        let fetched = db.get_thread(&thread.id).unwrap().expect("thread should exist");
        assert_eq!(fetched.title.unwrap(), "Test Thread");
        assert_eq!(fetched.model.unwrap(), "gpt-4");
        assert_eq!(fetched.source.unwrap(), "chat");
    }

    #[test]
    fn test_get_nonexistent_thread() {
        let db = setup_db();
        let result = db.get_thread("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_threads() {
        let db = setup_db();
        db.create_thread(Some("Thread A"), None, None).unwrap();
        db.create_thread(Some("Thread B"), None, None).unwrap();
        let threads = db.list_threads(10).unwrap();
        assert_eq!(threads.len(), 2);
    }

    #[test]
    fn test_store_and_get_messages() {
        let db = setup_db();
        let thread = db.create_thread(Some("Test"), None, None).unwrap();

        db.store_message(&thread.id, "user", "Hello!", None, Some(10)).unwrap();
        db.store_message(&thread.id, "assistant", "Hi there!", None, Some(15)).unwrap();

        let messages = db.get_thread_messages(&thread.id).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
    }

    #[test]
    fn test_record_and_list_deductions() {
        let db = setup_db();
        db.record_deduction("User prefers Rust over Python", 0.8, Some("preference"), None).unwrap();
        db.record_deduction("User works at SWAL", 0.9, Some("work"), None).unwrap();

        let deductions = db.list_deductions(10).unwrap();
        assert_eq!(deductions.len(), 2);
        assert!(deductions[0].deduction.contains("SWAL"));
    }

    #[test]
    fn test_upsert_belief_create_then_update() {
        let db = setup_db();

        // Create
        let belief = db.upsert_belief("xavier", "is a memory system", 0.7, Some("chat")).unwrap();
        assert_eq!(belief.subject, "xavier");

        // Update — same subject and proposition should upsert
        let updated = db.upsert_belief("xavier", "is a memory system", 0.95, Some("code")).unwrap();
        assert_eq!(updated.id, belief.id);
        assert!((updated.confidence - 0.95).abs() < 0.01);

        // Should only be 1 belief
        let beliefs = db.list_beliefs(10).unwrap();
        assert_eq!(beliefs.len(), 1);
    }

    #[test]
    fn test_create_and_get_checkpoint() {
        let db = setup_db();
        let cp = db.create_checkpoint("sess-1", "{\"state\": \"idle\"}", None, Some("work")).unwrap();
        assert_eq!(cp.state, "{\"state\": \"idle\"}");

        let fetched = db.get_checkpoint("sess-1").unwrap().expect("checkpoint should exist");
        assert_eq!(fetched.state, cp.state);
        assert_eq!(fetched.kind.unwrap(), "work");
    }

    #[test]
    fn test_get_nonexistent_checkpoint() {
        let db = setup_db();
        let result = db.get_checkpoint("no-such-session").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_store_message_updates_thread_timestamp() {
        let db = setup_db();
        let thread = db.create_thread(Some("Test"), None, None).unwrap();
        let original_updated = thread.updated_at;

        // Small delay to ensure timestamp changes
        std::thread::sleep(std::time::Duration::from_millis(10));

        db.store_message(&thread.id, "user", "msg", None, None).unwrap();

        // Re-fetch the thread
        let fetched = db.get_thread(&thread.id).unwrap().unwrap();
        assert!(fetched.updated_at > original_updated);
    }

    #[test]
    fn test_beliefs_are_per_project() {
        let db1 = ConversationsDb::open_in_memory("proj-a").unwrap();
        db1.create_schema().unwrap();
        db1.upsert_belief("rust", "is great", 0.9, None).unwrap();

        let db2 = ConversationsDb::open_in_memory("proj-b").unwrap();
        db2.create_schema().unwrap();
        db2.upsert_belief("python", "is great", 0.8, None).unwrap();

        assert_eq!(db1.list_beliefs(10).unwrap().len(), 1);
        assert_eq!(db2.list_beliefs(10).unwrap().len(), 1);
    }
}
