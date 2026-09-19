//! Inter-node human-to-human & node-to-node direct chat engine.
//!
//! Provides encrypted 1-to-1 chat rooms, message persistence, delivery receipts,
//! read receipts, off-the-record ephemeral messages, and asynchronous Tokio channels
//! for real-time inter-node communication.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

use crate::crypto::encryption::{decrypt_data, encrypt_data, NonceBytes};
use crate::crypto::NONCE_SIZE;
use crate::security::clearance::ClearanceLevel;
use crate::storage::pragma::apply_pragmas;

/// Module specific errors for telecom chat.
#[derive(Debug, Error)]
pub enum TelecomChatError {
    #[error("Room not found: {0}")]
    RoomNotFound(String),

    #[error("Message not found: {0}")]
    MessageNotFound(String),

    #[error("Participant '{0}' is not a member of room '{1}'")]
    UnauthorizedParticipant(String, String),

    #[error("Database error: {0}")]
    DatabaseError(#[from] rusqlite::Error),

    #[error("Encryption or decryption error: {0}")]
    CryptoError(String),

    #[error("Channel error: {0}")]
    ChannelError(String),
}

/// Status of a direct chat room.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RoomStatus {
    Active,
    Archived,
    Closed,
}

impl RoomStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RoomStatus::Active => "ACTIVE",
            RoomStatus::Archived => "ARCHIVED",
            RoomStatus::Closed => "CLOSED",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "ARCHIVED" => RoomStatus::Archived,
            "CLOSED" => RoomStatus::Closed,
            _ => RoomStatus::Active,
        }
    }
}

/// Status of a message receipt.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReceiptStatus {
    Delivered,
    Read,
}

impl ReceiptStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReceiptStatus::Delivered => "DELIVERED",
            ReceiptStatus::Read => "READ",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "READ" => ReceiptStatus::Read,
            _ => ReceiptStatus::Delivered,
        }
    }
}

/// Direct chat room between two nodes or humans.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectChatRoom {
    pub room_id: String,
    pub participant_a: String,
    pub participant_b: String,
    pub created_at: DateTime<Utc>,
    pub status: RoomStatus,
}

impl DirectChatRoom {
    pub fn contains_participant(&self, participant_id: &str) -> bool {
        self.participant_a == participant_id || self.participant_b == participant_id
    }

    pub fn peer_of(&self, participant_id: &str) -> Option<&str> {
        if self.participant_a == participant_id {
            Some(&self.participant_b)
        } else if self.participant_b == participant_id {
            Some(&self.participant_a)
        } else {
            None
        }
    }
}

/// A direct chat message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub message_id: String,
    pub room_id: String,
    pub sender_id: String,
    pub content_ciphertext_hex: String,
    pub nonce_hex: String,
    pub ephemeral: bool,
    pub timestamp: DateTime<Utc>,
    pub clearance_level: ClearanceLevel,
    pub delivered: bool,
    pub read: bool,
}

/// Message receipt tracking delivery or read status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageReceipt {
    pub receipt_id: String,
    pub message_id: String,
    pub room_id: String,
    pub recipient_id: String,
    pub status: ReceiptStatus,
    pub timestamp: DateTime<Utc>,
}

/// Real-time chat engine managing persistent SQLite store and Tokio async channels.
#[derive(Clone)]
pub struct DirectChatEngine {
    db_path: Option<String>,
    subscribers: Arc<RwLock<HashMap<String, mpsc::Sender<ChatMessage>>>>,
}

impl DirectChatEngine {
    /// Constructs a new [`DirectChatEngine`] with in-memory or file-backed database.
    pub fn new(db_path: Option<String>) -> Self {
        Self {
            db_path,
            subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Subscribes to real-time chat messages for a participant.
    pub async fn subscribe(&self, participant_id: &str) -> mpsc::Receiver<ChatMessage> {
        let (tx, rx) = mpsc::channel(100);
        let mut subs = self.subscribers.write().await;
        subs.insert(participant_id.to_string(), tx);
        rx
    }

    /// Broadcasts a message to an active subscriber if online.
    pub async fn dispatch_realtime(&self, recipient_id: &str, msg: ChatMessage) -> bool {
        let subs = self.subscribers.read().await;
        if let Some(tx) = subs.get(recipient_id) {
            tx.send(msg).await.is_ok()
        } else {
            false
        }
    }
}

/// Initializes database tables and applies performance PRAGMAs (WAL mode, busy_timeout = 5000).
pub fn init_chat_db(conn: &Connection) -> Result<()> {
    apply_pragmas(conn)?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS direct_rooms (
            room_id TEXT PRIMARY KEY,
            participant_a TEXT NOT NULL,
            participant_b TEXT NOT NULL,
            created_at TEXT NOT NULL,
            status TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS chat_messages (
            message_id TEXT PRIMARY KEY,
            room_id TEXT NOT NULL,
            sender_id TEXT NOT NULL,
            content_ciphertext_hex TEXT NOT NULL,
            nonce_hex TEXT NOT NULL,
            ephemeral INTEGER NOT NULL,
            timestamp TEXT NOT NULL,
            clearance_level TEXT NOT NULL,
            delivered INTEGER NOT NULL,
            read INTEGER NOT NULL,
            FOREIGN KEY (room_id) REFERENCES direct_rooms(room_id)
        );

        CREATE TABLE IF NOT EXISTS message_receipts (
            receipt_id TEXT PRIMARY KEY,
            message_id TEXT NOT NULL,
            room_id TEXT NOT NULL,
            recipient_id TEXT NOT NULL,
            status TEXT NOT NULL,
            timestamp TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_chat_messages_room ON chat_messages(room_id);
        CREATE INDEX IF NOT EXISTS idx_chat_messages_sender ON chat_messages(sender_id);
        CREATE INDEX IF NOT EXISTS idx_receipts_message ON message_receipts(message_id);",
    )?;

    Ok(())
}

/// Derives a deterministic room key for symmetric ChaCha20-Poly1305 / AES-256-GCM encryption.
pub fn derive_room_key(room_id: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"telecom_direct_chat_key_v1:");
    hasher.update(room_id.as_bytes());
    hasher.finalize().into()
}

/// Creates a new direct chat room between `participant_a` and `participant_b`.
pub fn create_room(
    conn: &Connection,
    participant_a: &str,
    participant_b: &str,
) -> Result<DirectChatRoom> {
    init_chat_db(conn)?;

    // Check if room already exists between these participants
    let existing: Option<DirectChatRoom> = conn
        .query_row(
            "SELECT room_id, participant_a, participant_b, created_at, status FROM direct_rooms \
             WHERE (participant_a = ?1 AND participant_b = ?2) OR (participant_a = ?2 AND participant_b = ?1)",
            params![participant_a, participant_b],
            |row| {
                let created_str: String = row.get(3)?;
                let status_str: String = row.get(4)?;
                Ok(DirectChatRoom {
                    room_id: row.get(0)?,
                    participant_a: row.get(1)?,
                    participant_b: row.get(2)?,
                    created_at: DateTime::parse_from_rfc3339(&created_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    status: RoomStatus::from_str(&status_str),
                })
            },
        )
        .optional()?;

    if let Some(room) = existing {
        return Ok(room);
    }

    let room_id = format!("room-{}", Uuid::new_v4());
    let now = Utc::now();
    let room = DirectChatRoom {
        room_id: room_id.clone(),
        participant_a: participant_a.to_string(),
        participant_b: participant_b.to_string(),
        created_at: now,
        status: RoomStatus::Active,
    };

    conn.execute(
        "INSERT INTO direct_rooms (room_id, participant_a, participant_b, created_at, status) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            room.room_id,
            room.participant_a,
            room.participant_b,
            room.created_at.to_rfc3339(),
            room.status.as_str(),
        ],
    )?;

    Ok(room)
}

/// Sends a direct message in a room. Encrypts message content using the room key.
/// If `ephemeral` is true, off-the-record message payload is generated but not persisted long-term.
pub fn send_direct_message(
    conn: &Connection,
    room: &DirectChatRoom,
    sender_id: &str,
    content: &str,
    ephemeral: bool,
    clearance: ClearanceLevel,
) -> Result<ChatMessage> {
    if !room.contains_participant(sender_id) {
        return Err(anyhow!(TelecomChatError::UnauthorizedParticipant(
            sender_id.to_string(),
            room.room_id.clone()
        )));
    }

    let key = derive_room_key(&room.room_id);
    let nonce = NonceBytes::generate();

    let blob = encrypt_data(content.as_bytes(), &key, &nonce)
        .map_err(|e| anyhow!("Failed to encrypt message: {:?}", e))?;

    let message_id = format!("msg-{}", Uuid::new_v4());
    let now = Utc::now();

    let message = ChatMessage {
        message_id: message_id.clone(),
        room_id: room.room_id.clone(),
        sender_id: sender_id.to_string(),
        content_ciphertext_hex: crate::crypto::hex_encode(&blob.ciphertext),
        nonce_hex: crate::crypto::hex_encode(&blob.nonce),
        ephemeral,
        timestamp: now,
        clearance_level: clearance,
        delivered: false,
        read: false,
    };

    // If not ephemeral (off-the-record), persist message to database
    if !ephemeral {
        init_chat_db(conn)?;
        conn.execute(
            "INSERT INTO chat_messages (message_id, room_id, sender_id, content_ciphertext_hex, nonce_hex, ephemeral, timestamp, clearance_level, delivered, read) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                message.message_id,
                message.room_id,
                message.sender_id,
                message.content_ciphertext_hex,
                message.nonce_hex,
                if message.ephemeral { 1 } else { 0 },
                message.timestamp.to_rfc3339(),
                message.clearance_level.as_str(),
                0,
                0,
            ],
        )?;
    }

    Ok(message)
}

/// Decrypts a [`ChatMessage`] using the room key.
pub fn decrypt_chat_message(room_id: &str, message: &ChatMessage) -> Result<String> {
    let key = derive_room_key(room_id);
    let ciphertext = crate::crypto::hex_decode(&message.content_ciphertext_hex)
        .map_err(|e| anyhow!("Invalid ciphertext hex: {:?}", e))?;
    let nonce_bytes = crate::crypto::hex_decode(&message.nonce_hex)
        .map_err(|e| anyhow!("Invalid nonce hex: {:?}", e))?;

    let nonce: [u8; NONCE_SIZE] = nonce_bytes
        .try_into()
        .map_err(|_| anyhow!("Invalid nonce size"))?;

    let plaintext = decrypt_data(&ciphertext, &key, &nonce)
        .map_err(|e| anyhow!("Decryption failed: {:?}", e))?;

    let content = String::from_utf8(plaintext)?;
    Ok(content)
}

/// Acknowledges message receipt by the recipient, updating delivery status.
pub fn acknowledge_receipt(
    conn: &Connection,
    message_id: &str,
    recipient_id: &str,
) -> Result<MessageReceipt> {
    init_chat_db(conn)?;

    let msg_info: Option<(String, String)> = conn
        .query_row(
            "SELECT room_id, sender_id FROM chat_messages WHERE message_id = ?1",
            params![message_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;

    let (room_id, _sender_id) = match msg_info {
        Some(info) => info,
        None => {
            return Err(anyhow!(TelecomChatError::MessageNotFound(
                message_id.to_string()
            )))
        }
    };

    conn.execute(
        "UPDATE chat_messages SET delivered = 1 WHERE message_id = ?1",
        params![message_id],
    )?;

    let receipt_id = format!("rcpt-{}", Uuid::new_v4());
    let now = Utc::now();
    let receipt = MessageReceipt {
        receipt_id: receipt_id.clone(),
        message_id: message_id.to_string(),
        room_id,
        recipient_id: recipient_id.to_string(),
        status: ReceiptStatus::Delivered,
        timestamp: now,
    };

    conn.execute(
        "INSERT INTO message_receipts (receipt_id, message_id, room_id, recipient_id, status, timestamp) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            receipt.receipt_id,
            receipt.message_id,
            receipt.room_id,
            receipt.recipient_id,
            receipt.status.as_str(),
            receipt.timestamp.to_rfc3339(),
        ],
    )?;

    Ok(receipt)
}

/// Marks a message (or room messages) as read by reader_id.
pub fn mark_read(
    conn: &Connection,
    room_id: &str,
    message_id: &str,
    reader_id: &str,
) -> Result<MessageReceipt> {
    init_chat_db(conn)?;

    conn.execute(
        "UPDATE chat_messages SET read = 1, delivered = 1 WHERE room_id = ?1 AND message_id = ?2",
        params![room_id, message_id],
    )?;

    let receipt_id = format!("read-{}", Uuid::new_v4());
    let now = Utc::now();
    let receipt = MessageReceipt {
        receipt_id: receipt_id.clone(),
        message_id: message_id.to_string(),
        room_id: room_id.to_string(),
        recipient_id: reader_id.to_string(),
        status: ReceiptStatus::Read,
        timestamp: now,
    };

    conn.execute(
        "INSERT INTO message_receipts (receipt_id, message_id, room_id, recipient_id, status, timestamp) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            receipt.receipt_id,
            receipt.message_id,
            receipt.room_id,
            receipt.recipient_id,
            receipt.status.as_str(),
            receipt.timestamp.to_rfc3339(),
        ],
    )?;

    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_create_room_success() {
        let conn = Connection::open_in_memory().unwrap();
        let room = create_room(&conn, "node-alpha", "node-beta").unwrap();

        assert_eq!(room.participant_a, "node-alpha");
        assert_eq!(room.participant_b, "node-beta");
        assert_eq!(room.status, RoomStatus::Active);

        // Re-creating room with same participants should return existing room
        let room2 = create_room(&conn, "node-beta", "node-alpha").unwrap();
        assert_eq!(room.room_id, room2.room_id);
    }

    #[test]
    fn test_send_and_decrypt_direct_message() {
        let conn = Connection::open_in_memory().unwrap();
        let room = create_room(&conn, "alice", "bob").unwrap();

        let original_text = "Hello Bob! Secret node payload.";
        let msg = send_direct_message(
            &conn,
            &room,
            "alice",
            original_text,
            false,
            ClearanceLevel::Confidential,
        )
        .unwrap();

        assert_eq!(msg.sender_id, "alice");
        assert!(!msg.ephemeral);

        let decrypted = decrypt_chat_message(&room.room_id, &msg).unwrap();
        assert_eq!(decrypted, original_text);
    }

    #[test]
    fn test_acknowledge_and_mark_read() {
        let conn = Connection::open_in_memory().unwrap();
        let room = create_room(&conn, "node-1", "node-2").unwrap();

        let msg = send_direct_message(
            &conn,
            &room,
            "node-1",
            "Urgent payload",
            false,
            ClearanceLevel::Internal,
        )
        .unwrap();

        let delivered_receipt = acknowledge_receipt(&conn, &msg.message_id, "node-2").unwrap();
        assert_eq!(delivered_receipt.status, ReceiptStatus::Delivered);

        let read_receipt = mark_read(&conn, &room.room_id, &msg.message_id, "node-2").unwrap();
        assert_eq!(read_receipt.status, ReceiptStatus::Read);
    }

    #[test]
    fn test_ephemeral_off_the_record_message() {
        let conn = Connection::open_in_memory().unwrap();
        let room = create_room(&conn, "agent-1", "agent-2").unwrap();

        let ephemeral_text = "Burn after reading payload";
        let msg = send_direct_message(
            &conn,
            &room,
            "agent-1",
            ephemeral_text,
            true,
            ClearanceLevel::TopSecret,
        )
        .unwrap();

        assert!(msg.ephemeral);

        // Decryption still works in memory
        let decrypted = decrypt_chat_message(&room.room_id, &msg).unwrap();
        assert_eq!(decrypted, ephemeral_text);

        // Verify message was NOT inserted into SQLite chat_messages table
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chat_messages WHERE message_id = ?1",
                params![msg.message_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}
