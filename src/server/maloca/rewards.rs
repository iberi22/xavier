//! Maloca Data Node Proof-of-Contribution Reward Tracker
//!
//! Handles metric collection (bytes sent, active uptime seconds), encrypted SQLite storage,
//! and Proof-of-Contribution score calculation for Data Nodes in SWAL Maloca network.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Result as SqliteResult};
use serde::{Deserialize, Serialize};

use crate::crypto::encryption::{aes_decrypt, aes_encrypt, NonceBytes};

const INIT_SQL: &str = "
    CREATE TABLE IF NOT EXISTS data_node_metrics (
        node_id TEXT NOT NULL,
        record_date TEXT NOT NULL,
        encrypted_payload BLOB NOT NULL,
        PRIMARY KEY (node_id, record_date)
    );
    CREATE INDEX IF NOT EXISTS idx_dnm_node ON data_node_metrics(node_id);
    CREATE INDEX IF NOT EXISTS idx_dnm_date ON data_node_metrics(record_date);
";

/// Data Node telemetry metric record
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataNodeMetrics {
    pub node_id: String,
    pub record_date: String, // YYYY-MM-DD
    pub active_uptime_secs: u64,
    pub bytes_sent: u64,
    pub contribution_score: f64,
    pub last_updated: DateTime<Utc>,
}

/// Configuration for ContributionTracker
#[derive(Debug, Clone)]
pub struct ContributionTrackerConfig {
    pub db_path: PathBuf,
    pub encryption_key: [u8; 32],
}

/// Local encrypted tracker for Data Node Proof-of-Contribution telemetry metrics
pub struct ContributionTracker {
    conn: Mutex<Connection>,
    key: [u8; 32],
}

impl ContributionTracker {
    /// Initialize tracker with persistent database file path and 32-byte AES key
    pub fn new(config: ContributionTrackerConfig) -> SqliteResult<Self> {
        let conn = Connection::open(&config.db_path)?;
        conn.execute_batch(INIT_SQL)?;
        Ok(Self {
            conn: Mutex::new(conn),
            key: config.encryption_key,
        })
    }

    /// Initialize tracker in-memory (useful for testing and temporary sessions)
    pub fn in_memory(key: [u8; 32]) -> SqliteResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(INIT_SQL)?;
        Ok(Self {
            conn: Mutex::new(conn),
            key,
        })
    }

    /// Calculate Proof-of-Contribution score based on uptime and data volume.
    /// Formula: (uptime_hours * 1.0) + (gigabytes_sent * 2.0)
    pub fn calculate_score(active_uptime_secs: u64, bytes_sent: u64) -> f64 {
        let uptime_hours = active_uptime_secs as f64 / 3600.0;
        let gigabytes_sent = bytes_sent as f64 / (1024.0 * 1024.0 * 1024.0);
        let score = (uptime_hours * 1.0) + (gigabytes_sent * 2.0);
        (score * 10000.0).round() / 10000.0 // Round to 4 decimal places
    }

    /// Record activity for a Data Node. Aggregates uptime and bytes sent for the current date (UTC).
    pub fn record_activity(
        &self,
        node_id: &str,
        active_uptime_secs: u64,
        bytes_sent: u64,
    ) -> SqliteResult<DataNodeMetrics> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        self.record_activity_for_date(node_id, &today, active_uptime_secs, bytes_sent)
    }

    /// Record activity for a Data Node on a specific date ("YYYY-MM-DD")
    pub fn record_activity_for_date(
        &self,
        node_id: &str,
        record_date: &str,
        active_uptime_secs: u64,
        bytes_sent: u64,
    ) -> SqliteResult<DataNodeMetrics> {
        let existing = self.get_metrics(node_id, record_date)?;

        let mut metrics = match existing {
            Some(mut m) => {
                m.active_uptime_secs = m.active_uptime_secs.saturating_add(active_uptime_secs);
                m.bytes_sent = m.bytes_sent.saturating_add(bytes_sent);
                m.last_updated = Utc::now();
                m
            }
            None => DataNodeMetrics {
                node_id: node_id.to_string(),
                record_date: record_date.to_string(),
                active_uptime_secs,
                bytes_sent,
                contribution_score: 0.0,
                last_updated: Utc::now(),
            },
        };

        metrics.contribution_score = Self::calculate_score(metrics.active_uptime_secs, metrics.bytes_sent);

        // Serialize and encrypt metrics
        let payload_json = serde_json::to_vec(&metrics).map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(e))
        })?;

        let nonce = NonceBytes::generate();
        let encrypted_bytes = aes_encrypt(&payload_json, &self.key, &nonce).map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(e))
        })?;

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO data_node_metrics (node_id, record_date, encrypted_payload)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(node_id, record_date) DO UPDATE SET encrypted_payload = excluded.encrypted_payload",
            params![node_id, record_date, encrypted_bytes],
        )?;

        Ok(metrics)
    }

    /// Get metrics for a given node_id and record_date ("YYYY-MM-DD")
    pub fn get_metrics(
        &self,
        node_id: &str,
        record_date: &str,
    ) -> SqliteResult<Option<DataNodeMetrics>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT encrypted_payload FROM data_node_metrics WHERE node_id = ?1 AND record_date = ?2",
        )?;

        let mut rows = stmt.query(params![node_id, record_date])?;
        if let Some(row) = rows.next()? {
            let encrypted_bytes: Vec<u8> = row.get(0)?;
            let decrypted_bytes = aes_decrypt(&encrypted_bytes, &self.key).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Blob,
                    Box::new(e),
                )
            })?;

            let metrics: DataNodeMetrics = serde_json::from_slice(&decrypted_bytes).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Blob,
                    Box::new(e),
                )
            })?;

            Ok(Some(metrics))
        } else {
            Ok(None)
        }
    }

    /// Get historical metrics for a specific node_id sorted by record_date descending
    pub fn list_metrics_for_node(&self, node_id: &str) -> SqliteResult<Vec<DataNodeMetrics>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT encrypted_payload FROM data_node_metrics WHERE node_id = ?1 ORDER BY record_date DESC",
        )?;

        let mut rows = stmt.query(params![node_id])?;
        let mut list = Vec::new();

        while let Some(row) = rows.next()? {
            let encrypted_bytes: Vec<u8> = row.get(0)?;
            let decrypted_bytes = aes_decrypt(&encrypted_bytes, &self.key).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Blob,
                    Box::new(e),
                )
            })?;

            let metrics: DataNodeMetrics = serde_json::from_slice(&decrypted_bytes).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Blob,
                    Box::new(e),
                )
            })?;

            list.push(metrics);
        }

        Ok(list)
    }

    /// Calculate total cumulative Proof-of-Contribution score across all records for a node
    pub fn get_total_contribution_score(&self, node_id: &str) -> SqliteResult<f64> {
        let metrics_list = self.list_metrics_for_node(node_id)?;
        let total_score: f64 = metrics_list.iter().map(|m| m.contribution_score).sum();
        Ok((total_score * 10000.0).round() / 10000.0)
    }
}
