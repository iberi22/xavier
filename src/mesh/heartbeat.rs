// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Periodic heartbeat payloads for Xavier Mesh peers.

use crate::mesh::node::NodeId;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone)]
pub struct HeartbeatService {
    node_id: NodeId,
    interval: Duration,
    peer_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeartbeatPayload {
    pub node_id: NodeId,
    pub timestamp: i64,
    pub version: String,
    pub load_avg: f32,
    pub peer_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeartbeatReceipt {
    pub node_id: NodeId,
    pub observed_at: i64,
    pub peer_count: usize,
    pub version: String,
}

impl HeartbeatService {
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            interval: DEFAULT_HEARTBEAT_INTERVAL,
            peer_count: 0,
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    pub fn with_peer_count(mut self, peer_count: usize) -> Self {
        self.peer_count = peer_count;
        self
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    pub fn payload(&self) -> HeartbeatPayload {
        HeartbeatPayload {
            node_id: self.node_id.clone(),
            timestamp: unix_timestamp(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            load_avg: current_load_average(),
            peer_count: self.peer_count,
        }
    }

    pub fn start(self, tx: broadcast::Sender<HeartbeatPayload>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.interval);
            loop {
                interval.tick().await;
                let _ = tx.send(self.payload());
            }
        })
    }

    pub fn handle_heartbeat(&self, payload: HeartbeatPayload) -> Result<HeartbeatReceipt> {
        ensure!(
            !payload.node_id.as_str().is_empty(),
            "Heartbeat payload is missing node_id"
        );
        ensure!(
            payload.timestamp > 0,
            "Heartbeat payload has invalid timestamp"
        );
        ensure!(
            !payload.version.trim().is_empty(),
            "Heartbeat payload is missing Xavier version"
        );
        Ok(HeartbeatReceipt {
            node_id: payload.node_id,
            observed_at: unix_timestamp(),
            peer_count: payload.peer_count,
            version: payload.version,
        })
    }
}

impl Default for HeartbeatService {
    fn default() -> Self {
        Self::new(NodeId("xv1-uninitialized".to_string()))
    }
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn current_load_average() -> f32 {
    #[cfg(unix)]
    {
        let avg = sysinfo::System::load_average();
        avg.one as f32
    }

    #[cfg(not(unix))]
    {
        0.0
    }
}
