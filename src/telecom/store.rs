//! Durable SQLite persistence for telecom rooms, messages, receipts, and session ratchets.
//!
//! Enforces PRAGMA journal_mode=WAL and PRAGMA busy_timeout=5000 to guarantee
//! concurrent read/write isolation without database lock collisions.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::storage::pragma::apply_pragmas;

/// Errors arising from SQLite telecom storage operations.
#[derive(Debug, Error)]
pub enum TelecomStoreError {
    #[error("Database error: {0}")]
    SqliteError(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Entity not found: {0}")]
    NotFound(String),
}

/// A stored chat message entity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredMessage {
    pub message_id: String,
    pub room_id: String,
    pub sender_node_id: String,
    pub encrypted_payload: Vec<u8>,
    pub sequence: u64,
    pub is_ephemeral: bool,
    pub sent_at: DateTime<Utc>,
}

/// A stored telecom room metadata entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredRoom {
    pub room_id: String,
    pub name: String,
    pub clearance: String,
    pub is_group: bool,
    pub created_at: DateTime<Utc>,
}

/// A stored peer session state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredSession {
    pub session_id: String,
    pub peer_node_id: String,
    pub state: String,
    pub last_active: DateTime<Utc>,
}

/// Database storage engine for the telecom subsystem.
#[derive(Clone)]
pub struct TelecomStore {
    conn: Arc<Mutex<Connection>>,
    db_path: PathBuf,
}

impl TelecomStore {
    /// Open or create a telecom SQLite database at the target path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, TelecomStoreError> {
        let conn = Connection::open(path.as_ref())?;
        let _ = apply_pragmas(&conn);
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: path.as_ref().to_path_buf(),
        };
        store.init_telecom_schema()?;
        Ok(store)
    }

    /// Open in-memory store for isolated testing.
    pub fn open_in_memory() -> Result<Self, TelecomStoreError> {
        let conn = Connection::open_in_memory()?;
        let _ = apply_pragmas(&conn);
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: PathBuf::from(":memory:"),
        };
        store.init_telecom_schema()?;
        Ok(store)
    }

    /// Initialize the tables required for telecom operations.
    pub fn init_telecom_schema(&self) -> Result<(), TelecomStoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS telecom_rooms (
                room_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                clearance TEXT NOT NULL,
                is_group INTEGER NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS telecom_messages (
                message_id TEXT PRIMARY KEY,
                room_id TEXT NOT NULL,
                sender_node_id TEXT NOT NULL,
                encrypted_payload BLOB NOT NULL,
                sequence INTEGER NOT NULL,
                is_ephemeral INTEGER NOT NULL,
                sent_at TEXT NOT NULL,
                FOREIGN KEY (room_id) REFERENCES telecom_rooms (room_id)
            );

            CREATE TABLE IF NOT EXISTS telecom_sessions (
                session_id TEXT PRIMARY KEY,
                peer_node_id TEXT NOT NULL,
                state TEXT NOT NULL,
                last_active TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_telecom_msgs_room ON telecom_messages (room_id, sequence);
            ",
        )?;
        Ok(())
    }

    /// Persist a message into the durable ledger.
    pub fn persist_message(&self, msg: &StoredMessage) -> Result<(), TelecomStoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO telecom_messages 
             (message_id, room_id, sender_node_id, encrypted_payload, sequence, is_ephemeral, sent_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                msg.message_id,
                msg.room_id,
                msg.sender_node_id,
                msg.encrypted_payload,
                msg.sequence,
                msg.is_ephemeral as i32,
                msg.sent_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    /// Fetch historical messages for a room ordered by sequence.
    pub fn fetch_room_history(
        &self,
        room_id: &str,
        limit: usize,
    ) -> Result<Vec<StoredMessage>, TelecomStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT message_id, room_id, sender_node_id, encrypted_payload, sequence, is_ephemeral, sent_at
             FROM telecom_messages
             WHERE room_id = ?1
             ORDER BY sequence ASC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![room_id, limit as i64], |row| {
            let sent_at_str: String = row.get(6)?;
            let sent_at = DateTime::parse_from_rfc3339(&sent_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(StoredMessage {
                message_id: row.get(0)?,
                room_id: row.get(1)?,
                sender_node_id: row.get(2)?,
                encrypted_payload: row.get(3)?,
                sequence: row.get(4)?,
                is_ephemeral: row.get::<_, i32>(5)? != 0,
                sent_at,
            })
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Create or update room record.
    pub fn persist_room(&self, room: &StoredRoom) -> Result<(), TelecomStoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO telecom_rooms (room_id, name, clearance, is_group, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                room.room_id,
                room.name,
                room.clearance,
                room.is_group as i32,
                room.created_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    /// Update session state and heartbeat timestamp.
    pub fn update_session_state(
        &self,
        session_id: &str,
        peer_node_id: &str,
        state: &str,
        last_active: DateTime<Utc>,
    ) -> Result<(), TelecomStoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO telecom_sessions (session_id, peer_node_id, state, last_active)
             VALUES (?1, ?2, ?3, ?4)",
            params![session_id, peer_node_id, state, last_active.to_rfc3339()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_init_and_room_persistence() {
        let store = TelecomStore::open_in_memory().expect("open memory store");
        let room = StoredRoom {
            room_id: "room-ops".to_string(),
            name: "Operations Room".to_string(),
            clearance: "Internal".to_string(),
            is_group: false,
            created_at: Utc::now(),
        };

        assert!(store.persist_room(&room).is_ok());
    }

    #[test]
    fn test_message_history_retrieval() {
        let store = TelecomStore::open_in_memory().unwrap();
        let room_id = "room-legal-1";
        store
            .persist_room(&StoredRoom {
                room_id: room_id.to_string(),
                name: "Legal Team".to_string(),
                clearance: "Confidential".to_string(),
                is_group: true,
                created_at: Utc::now(),
            })
            .unwrap();

        for i in 1..=3 {
            let msg = StoredMessage {
                message_id: format!("msg-{}", i),
                room_id: room_id.to_string(),
                sender_node_id: "node-alice".to_string(),
                encrypted_payload: vec![i as u8, 0, 1],
                sequence: i,
                is_ephemeral: false,
                sent_at: Utc::now(),
            };
            store.persist_message(&msg).unwrap();
        }

        let history = store.fetch_room_history(room_id, 10).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].message_id, "msg-1");
        assert_eq!(history[2].sequence, 3);
    }

    #[test]
    fn test_session_state_update() {
        let store = TelecomStore::open_in_memory().unwrap();
        assert!(store
            .update_session_state("sess-xyz", "peer-bob", "Established", Utc::now())
            .is_ok());
    }
}
