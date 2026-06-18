//! Reward Ledger — Append-only record of reward-producing events.
//!
//! Provides a deterministic, auditable history of all XP rewards issued by the node.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::mesh::tokenomics::rewards::RewardEvent;

/// A signed entry in the reward ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub event: RewardEvent,
    /// Unix timestamp when the entry was recorded.
    pub recorded_at: i64,
    /// Signature of the event data by the issuing node.
    pub signature_hex: String,
}

/// Persistent storage for the reward ledger.
pub struct RewardLedger {
    entries: Arc<Mutex<Vec<LedgerEntry>>>,
}

impl RewardLedger {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Records a reward event in the ledger.
    pub async fn record_event(&self, event: RewardEvent, signature_hex: String) -> Result<()> {
        let entry = LedgerEntry {
            event,
            recorded_at: Utc::now().timestamp(),
            signature_hex,
        };

        let mut entries = self.entries.lock().await;
        entries.push(entry);

        // In production, this would persist to a SQLite table.
        // For now, it stays in memory.
        Ok(())
    }

    /// Returns all entries in the ledger.
    pub async fn get_entries(&self) -> Vec<LedgerEntry> {
        let entries = self.entries.lock().await;
        entries.clone()
    }

    /// Verifies the integrity of the ledger.
    pub async fn verify_integrity(&self) -> bool {
        // TODO: Implement chain-of-trust verification (each entry hashes previous).
        true
    }
}

impl Default for RewardLedger {
    fn default() -> Self {
        Self::new()
    }
}
