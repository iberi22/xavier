//! Mesh Telemetry Collector
//!
//! Tracks peer uptime, message count, latency, and consensus agreement ratio
//! using an in-memory sliding window.

use crate::mesh::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const WINDOW_SIZE: usize = 100;

/// Metrics for a single peer node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMetrics {
    pub uptime_secs: u64,
    pub message_count: u64,
    pub latencies_ms: VecDeque<u64>,
    pub agreement_outcomes: VecDeque<bool>,
    pub last_seen: u64,
}

impl PeerMetrics {
    /// New.
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            uptime_secs: 0,
            message_count: 0,
            latencies_ms: VecDeque::with_capacity(WINDOW_SIZE),
            agreement_outcomes: VecDeque::with_capacity(WINDOW_SIZE),
            last_seen: now,
        }
    }
}

impl Default for PeerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl PeerMetrics {
    /// Record latency.
    pub fn record_latency(&mut self, latency_ms: u64) {
        if self.latencies_ms.len() >= WINDOW_SIZE {
            self.latencies_ms.pop_front();
        }
        self.latencies_ms.push_back(latency_ms);
        self.message_count += 1;
        self.last_seen = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }

    /// Record agreement.
    pub fn record_agreement(&mut self, agreed: bool) {
        if self.agreement_outcomes.len() >= WINDOW_SIZE {
            self.agreement_outcomes.pop_front();
        }
        self.agreement_outcomes.push_back(agreed);
    }

    /// Agreement ratio.
    pub fn agreement_ratio(&self) -> f64 {
        if self.agreement_outcomes.is_empty() {
            return 1.0; // Default to 1.0 if no data
        }
        let agreed_count = self.agreement_outcomes.iter().filter(|&&a| a).count();
        agreed_count as f64 / self.agreement_outcomes.len() as f64
    }
}

/// Collector for mesh-wide peer telemetry.
#[derive(Debug)]
pub struct MeshTelemetryCollector {
    peer_metrics: Arc<Mutex<HashMap<NodeId, PeerMetrics>>>,
    #[expect(dead_code, reason = "Reservado para telemetria mesh futura")]
    started_at: Instant,
}

impl MeshTelemetryCollector {
    /// New.
    pub fn new() -> Self {
        Self {
            peer_metrics: Arc::new(Mutex::new(HashMap::new())),
            started_at: Instant::now(),
        }
    }

    /// Record latency.
    pub fn record_latency(&self, node_id: &NodeId, latency_ms: u64) {
        let mut metrics = self.peer_metrics.lock().expect("metrics lock poisoned");
        metrics
            .entry(node_id.clone())
            .or_default()
            .record_latency(latency_ms);
    }

    /// Record agreement.
    pub fn record_agreement(&self, node_id: &NodeId, agreed: bool) {
        let mut metrics = self.peer_metrics.lock().expect("metrics lock poisoned");
        metrics
            .entry(node_id.clone())
            .or_default()
            .record_agreement(agreed);
    }

    /// Get peer agreement ratio.
    pub fn get_peer_agreement_ratio(&self, node_id: &NodeId) -> f64 {
        let metrics = self.peer_metrics.lock().expect("metrics lock poisoned");
        metrics
            .get(node_id)
            .map(|m| m.agreement_ratio())
            .unwrap_or(1.0)
    }

    /// Get overall agreement ratio.
    pub fn get_overall_agreement_ratio(&self) -> f64 {
        let metrics = self.peer_metrics.lock().expect("metrics lock poisoned");
        if metrics.is_empty() {
            return 1.0;
        }
        let sum: f64 = metrics.values().map(|m| m.agreement_ratio()).sum();
        sum / metrics.len() as f64
    }

    /// Get unhealthy peers.
    pub fn get_unhealthy_peers(&self, threshold: f64) -> Vec<NodeId> {
        let metrics = self.peer_metrics.lock().expect("metrics lock poisoned");
        metrics
            .iter()
            .filter(|(_, m)| m.agreement_ratio() < threshold)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Simulated consensus round to update agreement metrics.
    /// In a real P2P implementation, this would involve a network ping/pong.
    pub async fn run_consensus_round(&self, peers: Vec<NodeId>) {
        for peer in peers {
            // Simulated consensus check: 95% success rate
            let agreed = rand::random::<f64>() > 0.05;
            self.record_agreement(&peer, agreed);
            // Simulated latency: 10ms - 210ms
            self.record_latency(&peer, (rand::random::<u64>() % 200) + 10);
        }
    }

    /// Get peer latency.
    pub fn get_peer_latency(&self, node_id: &NodeId) -> f64 {
        let metrics = self.peer_metrics.lock().expect("metrics lock poisoned");
        metrics
            .get(node_id)
            .and_then(|m| m.latencies_ms.iter().last().copied())
            .map(|l| l as f64)
            .unwrap_or(0.0)
    }

    /// Get peer message count.
    pub fn get_peer_message_count(&self, node_id: &NodeId) -> u64 {
        let metrics = self.peer_metrics.lock().expect("metrics lock poisoned");
        metrics.get(node_id).map(|m| m.message_count).unwrap_or(0)
    }

    /// Get total message count.
    pub fn get_total_message_count(&self) -> u64 {
        let metrics = self.peer_metrics.lock().expect("metrics lock poisoned");
        metrics.values().map(|m| m.message_count).sum()
    }

    /// Get telemetry collector uptime duration.
    pub fn uptime(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }

    /// Number of known peers.
    pub fn peer_count(&self) -> u32 {
        let metrics = self.peer_metrics.lock().expect("metrics lock poisoned");
        metrics.len() as u32
    }

    /// Number of peers considered healthy (agreement ratio >= 0.5).
    pub fn connected_peer_count(&self) -> u32 {
        let metrics = self.peer_metrics.lock().expect("metrics lock poisoned");
        metrics
            .values()
            .filter(|m| m.agreement_ratio() >= 0.5)
            .count() as u32
    }

    /// Average latency across all peers in milliseconds. Returns 0.0 if no data.
    pub fn average_latency_ms(&self) -> f64 {
        let metrics = self.peer_metrics.lock().expect("metrics lock poisoned");
        let total: u64 = metrics
            .values()
            .filter_map(|m| m.latencies_ms.iter().last().copied())
            .sum();
        let count = metrics
            .values()
            .filter(|m| !m.latencies_ms.is_empty())
            .count();
        if count == 0 {
            0.0
        } else {
            total as f64 / count as f64
        }
    }

    /// Update uptime for all known peers.
    pub fn update_uptimes(&self) {
        let mut metrics = self.peer_metrics.lock().expect("metrics lock poisoned");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        for m in metrics.values_mut() {
            if now > m.last_seen {
                m.uptime_secs += now - m.last_seen;
                m.last_seen = now;
            }
        }
    }
}

impl Default for MeshTelemetryCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mesh_telemetry_sliding_window() {
        let collector = MeshTelemetryCollector::new();
        let node_id = NodeId("xv1-test".to_string());

        // Record 110 agreement outcomes
        for i in 0..110 {
            collector.record_agreement(&node_id, i < 55); // 50/50 initially, then more false
        }

        let metrics_lock = collector.peer_metrics.lock().unwrap();
        let metrics = metrics_lock.get(&node_id).unwrap();

        assert_eq!(metrics.agreement_outcomes.len(), 100);
        // The first 10 (which were true) should have been popped
        // So we have 45 true and 55 false
        assert_eq!(metrics.agreement_ratio(), 0.45);
    }

    #[tokio::test]
    async fn test_overall_agreement_ratio() {
        let collector = MeshTelemetryCollector::new();
        let node1 = NodeId("xv1-node1".to_string());
        let node2 = NodeId("xv1-node2".to_string());

        collector.record_agreement(&node1, true);
        collector.record_agreement(&node2, false);

        assert_eq!(collector.get_overall_agreement_ratio(), 0.5);
    }
}
