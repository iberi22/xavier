//! MKT-11 append-only hash-chained audit log.

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use thiserror::Error;

pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
pub const ENTRY_KINDS: [&str; 3] = ["grant", "purchase", "receipt"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    pub seq: i64,
    pub prev_hash: String,
    pub entry_json: String,
    pub hash: String,
}

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("invalid kind: {0}")]
    InvalidKind(String),
    #[error("broken chain at seq: {seq}")]
    Broken { seq: i64 },
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub struct AuditChain {
    conn: Mutex<Connection>,
}

impl AuditChain {
    pub fn open(path: &Path) -> Result<Self, AuditError> {
        let conn = Connection::open(path)?;
        Self::init(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self, AuditError> {
        let conn = Connection::open_in_memory()?;
        Self::init(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn init(conn: &Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS market_audit_log (
                seq INTEGER PRIMARY KEY AUTOINCREMENT, prev_hash TEXT NOT NULL,
                entry_json TEXT NOT NULL, hash TEXT NOT NULL);",
        )
    }

    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn append(&self, kind: &str, entry: &serde_json::Value) -> Result<AuditRecord, AuditError> {
        if !ENTRY_KINDS.contains(&kind) {
            return Err(AuditError::InvalidKind(kind.to_string()));
        }
        let entry_json = json!({"kind": kind, "entry": entry}).to_string();
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        let prev = tx
            .query_row(
                "SELECT seq, hash FROM market_audit_log ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;

        let (p_seq, p_hash) = prev.unwrap_or((0, GENESIS_HASH.to_string()));
        let seq = p_seq + 1;
        let mut hasher = Sha256::new();
        hasher.update(format!("{p_hash}|{seq}|{entry_json}").as_bytes());
        let hash = hex::encode(hasher.finalize());

        tx.execute(
            "INSERT INTO market_audit_log (seq, prev_hash, entry_json, hash) VALUES (?1, ?2, ?3, ?4)",
            params![seq, p_hash, entry_json, hash],
        )?;
        tx.commit()?;
        Ok(AuditRecord {
            seq,
            prev_hash: p_hash,
            entry_json,
            hash,
        })
    }

    #[rustfmt::skip]
    pub fn records(&self) -> Result<Vec<AuditRecord>, AuditError> {
        let rows = self.conn().prepare("SELECT seq, prev_hash, entry_json, hash FROM market_audit_log ORDER BY seq ASC")?.query_map([], |r| {
            Ok(AuditRecord { seq: r.get(0)?, prev_hash: r.get(1)?, entry_json: r.get(2)?, hash: r.get(3)? })
        })?.map(|r| r.map_err(AuditError::Sqlite)).collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn verify(&self) -> Result<usize, AuditError> {
        let records = self.records()?;
        let mut expected_prev = GENESIS_HASH.to_string();
        for r in &records {
            if r.prev_hash != expected_prev {
                return Err(AuditError::Broken { seq: r.seq });
            }
            let mut hasher = Sha256::new();
            hasher.update(format!("{}|{}|{}", r.prev_hash, r.seq, r.entry_json).as_bytes());
            if r.hash != hex::encode(hasher.finalize()) {
                return Err(AuditError::Broken { seq: r.seq });
            }
            expected_prev = r.hash.clone();
        }
        Ok(records.len())
    }
}

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*;

    #[test]
    fn test_append_and_verify() {
        let chain = AuditChain::open_in_memory().unwrap();
        let r1 = chain.append("grant", &json!(1)).unwrap();
        let r2 = chain.append("purchase", &json!(2)).unwrap();
        let r3 = chain.append("receipt", &json!(3)).unwrap();
        assert_eq!(r1.prev_hash, GENESIS_HASH);
        assert_eq!(r2.prev_hash, r1.hash);
        assert_eq!(r3.prev_hash, r2.hash);
        assert_eq!(chain.verify().unwrap(), 3);
    }

    #[test]
    fn test_invalid_kind_rejected() {
        assert!(AuditChain::open_in_memory().unwrap().append("x", &json!(1)).is_err());
    }

    #[test]
    fn test_tamper_edit_breaks_verify() {
        let chain = AuditChain::open_in_memory().unwrap();
        chain.append("grant", &json!(1)).unwrap();
        chain.append("purchase", &json!(2)).unwrap();
        chain.conn().execute("UPDATE market_audit_log SET entry_json = 'x' WHERE seq = 2", []).unwrap();
        assert!(matches!(chain.verify(), Err(AuditError::Broken { seq: 2 })));
    }

    #[test]
    fn test_tamper_delete_breaks_verify() {
        let chain = AuditChain::open_in_memory().unwrap();
        chain.append("grant", &json!(1)).unwrap();
        chain.append("purchase", &json!(2)).unwrap();
        chain.conn().execute("DELETE FROM market_audit_log WHERE seq = 1", []).unwrap();
        assert!(matches!(chain.verify(), Err(AuditError::Broken { seq: 2 })));
    }

    #[test]
    fn test_first_record_prev_hash_is_genesis() {
        assert_eq!(AuditChain::open_in_memory().unwrap().append("grant", &json!(1)).unwrap().prev_hash, GENESIS_HASH);
    }

    #[test]
    fn test_chain_survives_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.db");
        AuditChain::open(&path).unwrap().append("grant", &json!(1)).unwrap();
        assert_eq!(AuditChain::open(&path).unwrap().verify().unwrap(), 1);
    }
}
