//! 24h Health Status History Ring Buffer
//!
//! Stores historical health snapshots and status transitions over a rolling 24-hour window.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, OnceLock, RwLock};

/// Maximum entries in the ring buffer.
const MAX_HISTORY_CAPACITY: usize = 288; // e.g. 5-min intervals over 24h = 288 entries
/// 24 hours in seconds
const SECONDS_24H: u64 = 86_400;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthHistoryEntry {
    pub timestamp_secs: u64,
    pub status: String,
    pub component_statuses: BTreeMap<String, String>,
    pub component_latencies_ms: BTreeMap<String, f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthHistoryResponse {
    pub window_secs: u64,
    pub total_entries: usize,
    pub entries: Vec<HealthHistoryEntry>,
}

#[derive(Debug)]
pub struct HealthHistoryRingBuffer {
    capacity: usize,
    buffer: VecDeque<HealthHistoryEntry>,
}

impl Default for HealthHistoryRingBuffer {
    fn default() -> Self {
        Self::new(MAX_HISTORY_CAPACITY)
    }
}

impl HealthHistoryRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            buffer: VecDeque::with_capacity(capacity),
        }
    }

    pub fn record(&mut self, entry: HealthHistoryEntry) {
        // Prune entries older than 24h relative to the new entry
        let cutoff = entry.timestamp_secs.saturating_sub(SECONDS_24H);
        while let Some(front) = self.buffer.front() {
            if front.timestamp_secs < cutoff {
                self.buffer.pop_front();
            } else {
                break;
            }
        }

        // If at capacity, pop oldest
        if self.buffer.len() >= self.capacity {
            self.buffer.pop_front();
        }

        self.buffer.push_back(entry);
    }

    pub fn get_history(&self, now_secs: u64) -> Vec<HealthHistoryEntry> {
        let cutoff = now_secs.saturating_sub(SECONDS_24H);
        self.buffer
            .iter()
            .filter(|e| e.timestamp_secs >= cutoff)
            .cloned()
            .collect()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

static HEALTH_HISTORY: OnceLock<Arc<RwLock<HealthHistoryRingBuffer>>> = OnceLock::new();

pub fn get_health_history_instance() -> Arc<RwLock<HealthHistoryRingBuffer>> {
    HEALTH_HISTORY
        .get_or_init(|| Arc::new(RwLock::new(HealthHistoryRingBuffer::default())))
        .clone()
}

pub fn record_health_history(entry: HealthHistoryEntry) {
    if let Ok(mut lock) = get_health_history_instance().write() {
        lock.record(entry);
    }
}

pub fn fetch_health_history(now_secs: u64) -> HealthHistoryResponse {
    let entries = if let Ok(lock) = get_health_history_instance().read() {
        lock.get_history(now_secs)
    } else {
        Vec::new()
    };

    HealthHistoryResponse {
        window_secs: SECONDS_24H,
        total_entries: entries.len(),
        entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_capacity_and_pruning() {
        let mut buf = HealthHistoryRingBuffer::new(3);

        let mut comp = BTreeMap::new();
        comp.insert("db".to_string(), "healthy".to_string());
        let mut lat = BTreeMap::new();
        lat.insert("db".to_string(), 1.2);

        buf.record(HealthHistoryEntry {
            timestamp_secs: 1000,
            status: "healthy".to_string(),
            component_statuses: comp.clone(),
            component_latencies_ms: lat.clone(),
            transition_reason: None,
        });

        buf.record(HealthHistoryEntry {
            timestamp_secs: 2000,
            status: "healthy".to_string(),
            component_statuses: comp.clone(),
            component_latencies_ms: lat.clone(),
            transition_reason: None,
        });

        buf.record(HealthHistoryEntry {
            timestamp_secs: 3000,
            status: "degraded".to_string(),
            component_statuses: comp.clone(),
            component_latencies_ms: lat.clone(),
            transition_reason: Some("high latency".to_string()),
        });

        assert_eq!(buf.len(), 3);

        // Record 4th entry beyond capacity (capacity=3)
        buf.record(HealthHistoryEntry {
            timestamp_secs: 4000,
            status: "healthy".to_string(),
            component_statuses: comp,
            component_latencies_ms: lat,
            transition_reason: None,
        });

        assert_eq!(buf.len(), 3);
        let history = buf.get_history(4000);
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].timestamp_secs, 2000);
        assert_eq!(history[2].timestamp_secs, 4000);
    }

    #[test]
    fn test_prune_entries_older_than_24h() {
        let mut buf = HealthHistoryRingBuffer::new(100);

        buf.record(HealthHistoryEntry {
            timestamp_secs: 100,
            status: "healthy".to_string(),
            component_statuses: BTreeMap::new(),
            component_latencies_ms: BTreeMap::new(),
            transition_reason: None,
        });

        // 100 + 86401 = 86501
        buf.record(HealthHistoryEntry {
            timestamp_secs: 86_501,
            status: "healthy".to_string(),
            component_statuses: BTreeMap::new(),
            component_latencies_ms: BTreeMap::new(),
            transition_reason: None,
        });

        assert_eq!(buf.len(), 1);
        let history = buf.get_history(86_501);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].timestamp_secs, 86_501);
    }
}
