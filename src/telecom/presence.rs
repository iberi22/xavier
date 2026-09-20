//! Presence manager tracking peer online/away status via keepalive beacons (WAVE-27.23 / Issue #2401).
//!
//! Tracks peer heartbeat timestamps, categorizes node reachability (Online, Idle, Offline),
//! and broadcasts status transition events to subscribers over Tokio broadcast channels.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};

/// Peer reachability status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerPresenceStatus {
    Online,
    Idle,
    Offline,
}

impl std::fmt::Display for PeerPresenceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Online => write!(f, "online"),
            Self::Idle => write!(f, "idle"),
            Self::Offline => write!(f, "offline"),
        }
    }
}

/// Metadata recorded for each peer presence beacon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerPresenceEntry {
    pub node_id: String,
    pub status: PeerPresenceStatus,
    pub last_seen: DateTime<Utc>,
    pub metadata: HashMap<String, String>,
}

/// Presence transition event emitted when peer status changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceChangeEvent {
    pub node_id: String,
    pub old_status: PeerPresenceStatus,
    pub new_status: PeerPresenceStatus,
    pub timestamp: DateTime<Utc>,
}

/// Presence manager tracking peer heartbeats and issuing lifecycle events.
#[derive(Clone)]
pub struct PresenceManager {
    peers: Arc<RwLock<HashMap<String, PeerPresenceEntry>>>,
    event_tx: broadcast::Sender<PresenceChangeEvent>,
    idle_threshold: Duration,
    offline_threshold: Duration,
}

impl PresenceManager {
    /// Create a new presence manager with specified timeout thresholds.
    pub fn new(idle_threshold: Duration, offline_threshold: Duration) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            idle_threshold,
            offline_threshold,
        }
    }

    /// Record an incoming keepalive heartbeat from a peer node.
    pub async fn record_heartbeat(
        &self,
        node_id: impl Into<String>,
        metadata: Option<HashMap<String, String>>,
    ) {
        let node_id = node_id.into();
        let now = Utc::now();
        let mut peers = self.peers.write().await;

        let entry = peers
            .entry(node_id.clone())
            .or_insert_with(|| PeerPresenceEntry {
                node_id: node_id.clone(),
                status: PeerPresenceStatus::Offline,
                last_seen: now,
                metadata: HashMap::new(),
            });

        let old_status = entry.status;
        entry.last_seen = now;
        entry.status = PeerPresenceStatus::Online;
        if let Some(meta) = metadata {
            entry.metadata.extend(meta);
        }

        if old_status != PeerPresenceStatus::Online {
            let _ = self.event_tx.send(PresenceChangeEvent {
                node_id,
                old_status,
                new_status: PeerPresenceStatus::Online,
                timestamp: now,
            });
        }
    }

    /// Get current presence state of a specific node.
    pub async fn get_peer_status(&self, node_id: &str) -> PeerPresenceStatus {
        let peers = self.peers.read().await;
        if let Some(entry) = peers.get(node_id) {
            let elapsed = match (Utc::now() - entry.last_seen).to_std() {
                Ok(d) => d,
                Err(_) => Duration::from_secs(0),
            };
            if elapsed >= self.offline_threshold {
                PeerPresenceStatus::Offline
            } else if elapsed >= self.idle_threshold {
                PeerPresenceStatus::Idle
            } else {
                entry.status
            }
        } else {
            PeerPresenceStatus::Offline
        }
    }

    /// Recompute presence statuses for all peers and emit transitions.
    pub async fn sweep_and_evaluate_timeouts(&self) -> Vec<PresenceChangeEvent> {
        let now = Utc::now();
        let mut changes = Vec::new();
        let mut peers = self.peers.write().await;

        for (node_id, entry) in peers.iter_mut() {
            let elapsed = match (now - entry.last_seen).to_std() {
                Ok(d) => d,
                Err(_) => Duration::from_secs(0),
            };

            let computed_status = if elapsed >= self.offline_threshold {
                PeerPresenceStatus::Offline
            } else if elapsed >= self.idle_threshold {
                PeerPresenceStatus::Idle
            } else {
                PeerPresenceStatus::Online
            };

            if computed_status != entry.status {
                let change = PresenceChangeEvent {
                    node_id: node_id.clone(),
                    old_status: entry.status,
                    new_status: computed_status,
                    timestamp: now,
                };
                entry.status = computed_status;
                let _ = self.event_tx.send(change.clone());
                changes.push(change);
            }
        }

        changes
    }

    /// Subscribe to live presence change events.
    pub fn subscribe(&self) -> broadcast::Receiver<PresenceChangeEvent> {
        self.event_tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_presence_heartbeat_and_transitions() {
        let mgr = PresenceManager::new(Duration::from_millis(100), Duration::from_millis(300));
        let mut rx = mgr.subscribe();

        // Initial heartbeat emits Online transition
        mgr.record_heartbeat("node-alpha", None).await;
        let event = rx.recv().await.unwrap();
        assert_eq!(event.node_id, "node-alpha");
        assert_eq!(event.old_status, PeerPresenceStatus::Offline);
        assert_eq!(event.new_status, PeerPresenceStatus::Online);

        // Immediate check is Online
        assert_eq!(
            mgr.get_peer_status("node-alpha").await,
            PeerPresenceStatus::Online
        );

        // Sleep past idle threshold
        tokio::time::sleep(Duration::from_millis(150)).await;
        let changes = mgr.sweep_and_evaluate_timeouts().await;
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].new_status, PeerPresenceStatus::Idle);

        // Sleep past offline threshold
        tokio::time::sleep(Duration::from_millis(200)).await;
        let changes_off = mgr.sweep_and_evaluate_timeouts().await;
        assert_eq!(changes_off.len(), 1);
        assert_eq!(changes_off[0].new_status, PeerPresenceStatus::Offline);
        assert_eq!(
            mgr.get_peer_status("node-alpha").await,
            PeerPresenceStatus::Offline
        );
    }
}
