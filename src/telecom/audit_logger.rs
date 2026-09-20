//! Structured security audit logger for clearance rejections & signature verification (#2398 / WAVE-27.20)

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::storage::pragma::apply_pragmas;

#[derive(Debug, Error)]
pub enum TelecomAuditError {
    #[error("Database error: {0}")]
    SqliteError(#[from] rusqlite::Error),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

/// Security event types for telecom operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecurityEventType {
    ClearanceRejection,
    UntrustedPubkey,
    RateLimitTriggered,
    ReplayAttackDetected,
    MalformedPacket,
}

impl SecurityEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SecurityEventType::ClearanceRejection => "CLEARANCE_REJECTION",
            SecurityEventType::UntrustedPubkey => "UNTRUSTED_PUBKEY",
            SecurityEventType::RateLimitTriggered => "RATE_LIMIT_TRIGGERED",
            SecurityEventType::ReplayAttackDetected => "REPLAY_ATTACK_DETECTED",
            SecurityEventType::MalformedPacket => "MALFORMED_PACKET",
        }
    }
}

/// A structured security audit record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityAuditRecord {
    pub event_id: String,
    pub event_type: SecurityEventType,
    pub node_id: String,
    pub details: String,
    pub timestamp: DateTime<Utc>,
}

/// Thread-safe audit logger storing security events in SQLite.
#[derive(Clone)]
pub struct TelecomAuditLogger {
    conn: Arc<Mutex<Connection>>,
    db_path: PathBuf,
}

impl TelecomAuditLogger {
    /// Opens or creates an audit database at `path`.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, TelecomAuditError> {
        let conn = Connection::open(path.as_ref())?;
        apply_pragmas(&conn)?;
        let logger = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: path.as_ref().to_path_buf(),
        };
        logger.init_schema()?;
        Ok(logger)
    }

    /// In-memory audit database for testing.
    pub fn open_in_memory() -> Result<Self, TelecomAuditError> {
        let conn = Connection::open_in_memory()?;
        apply_pragmas(&conn)?;
        let logger = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: PathBuf::from(":memory:"),
        };
        logger.init_schema()?;
        Ok(logger)
    }

    fn init_schema(&self) -> Result<(), TelecomAuditError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS telecom_security_audit (
                event_id TEXT PRIMARY KEY,
                event_type TEXT NOT NULL,
                node_id TEXT NOT NULL,
                details TEXT NOT NULL,
                timestamp TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_telecom_audit_type ON telecom_security_audit(event_type);
            CREATE INDEX IF NOT EXISTS idx_telecom_audit_node ON telecom_security_audit(node_id);",
        )?;
        Ok(())
    }

    /// Records a security audit event into the ledger.
    pub fn record_security_event(
        &self,
        event_type: SecurityEventType,
        node_id: &str,
        details: &str,
    ) -> Result<SecurityAuditRecord, TelecomAuditError> {
        let record = SecurityAuditRecord {
            event_id: format!("audit-{}", uuid::Uuid::new_v4()),
            event_type,
            node_id: node_id.to_string(),
            details: details.to_string(),
            timestamp: Utc::now(),
        };

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO telecom_security_audit (event_id, event_type, node_id, details, timestamp) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                record.event_id,
                record.event_type.as_str(),
                record.node_id,
                record.details,
                record.timestamp.to_rfc3339()
            ],
        )?;

        Ok(record)
    }

    /// Queries audit events by node.
    pub fn get_events_for_node(
        &self,
        node_id: &str,
        limit: usize,
    ) -> Result<Vec<SecurityAuditRecord>, TelecomAuditError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT event_id, event_type, node_id, details, timestamp \
             FROM telecom_security_audit WHERE node_id = ?1 ORDER BY timestamp DESC LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![node_id, limit], |row| {
            let type_str: String = row.get(1)?;
            let event_type = match type_str.as_str() {
                "CLEARANCE_REJECTION" => SecurityEventType::ClearanceRejection,
                "UNTRUSTED_PUBKEY" => SecurityEventType::UntrustedPubkey,
                "RATE_LIMIT_TRIGGERED" => SecurityEventType::RateLimitTriggered,
                "REPLAY_ATTACK_DETECTED" => SecurityEventType::ReplayAttackDetected,
                _ => SecurityEventType::MalformedPacket,
            };
            let ts_str: String = row.get(4)?;
            let timestamp = DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(SecurityAuditRecord {
                event_id: row.get(0)?,
                event_type,
                node_id: row.get(2)?,
                details: row.get(3)?,
                timestamp,
            })
        })?;

        let mut events = Vec::new();
        for r in rows {
            events.push(r?);
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_retrieve_audit_events() {
        let logger = TelecomAuditLogger::open_in_memory().expect("open memory db");
        let rec = logger
            .record_security_event(
                SecurityEventType::ClearanceRejection,
                "node-malicious",
                "Access denied for TopSecret clearance",
            )
            .expect("record event");

        assert_eq!(rec.event_type, SecurityEventType::ClearanceRejection);
        assert_eq!(rec.node_id, "node-malicious");

        let events = logger
            .get_events_for_node("node-malicious", 10)
            .expect("query events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].details, "Access denied for TopSecret clearance");
    }
}
