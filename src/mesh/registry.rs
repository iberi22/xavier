//! Peer Registry Sync Adapter
//!
//! Connects `PeerRegistry` with `PeerMemorySync` for background reconciliation
//! and register-triggered synchronization.

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::memory::sync::PeerMemorySync;
use crate::mesh::peer::{PeerInfo, PeerRegistry};

/// Adapter wiring `PeerRegistry` and `PeerMemorySync`.
pub struct PeerRegistrySyncAdapter {
    registry: Arc<RwLock<PeerRegistry>>,
    sync_service: Arc<PeerMemorySync>,
}

impl PeerRegistrySyncAdapter {
    /// Create a new `PeerRegistrySyncAdapter`.
    pub fn new(registry: Arc<RwLock<PeerRegistry>>, sync_service: Arc<PeerMemorySync>) -> Self {
        Self {
            registry,
            sync_service,
        }
    }

    /// Retrieve active sync-enabled peer endpoint URLs from the registry.
    pub async fn active_peer_urls(&self) -> Vec<String> {
        let reg = self.registry.read().await;
        reg.list_peers()
            .into_iter()
            .filter(|p| p.sync_enabled)
            .map(|p| p.endpoint_url.clone())
            .collect()
    }

    /// Execute `PeerMemorySync::sync_loop` using active peer URLs from the registry.
    pub async fn sync_loop(&self, stop: Arc<std::sync::atomic::AtomicBool>) {
        let peers = self.active_peer_urls().await;
        self.sync_service.sync_loop(peers, stop).await;
    }

    /// Spawn a background periodic reconciliation task querying the registry for peers.
    pub fn start_background_sync(
        &self,
        stop: Arc<std::sync::atomic::AtomicBool>,
    ) -> tokio::task::JoinHandle<()> {
        let registry = self.registry.clone();
        let sync_service = self.sync_service.clone();

        tokio::spawn(async move {
            info!("PeerRegistrySyncAdapter: background sync loop started");
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let peers: Vec<String> = {
                    let reg = registry.read().await;
                    reg.list_peers()
                        .into_iter()
                        .filter(|p| p.sync_enabled && p.is_healthy())
                        .map(|p| p.endpoint_url.clone())
                        .collect()
                };

                for peer in &peers {
                    if stop.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    if !sync_service.ping(peer).await {
                        tracing::debug!("PeerRegistrySyncAdapter: skipping offline peer {peer}");
                        continue;
                    }
                    match sync_service.sync_with(peer).await {
                        Ok(session) => {
                            info!(
                                "PeerRegistrySyncAdapter: sync_with {peer} ok (sent={}, recv={})",
                                session.chunks_sent, session.chunks_received
                            );
                        }
                        Err(e) => {
                            warn!("PeerRegistrySyncAdapter: sync_with {peer} failed: {e:#}");
                        }
                    }
                }

                let interval = sync_service.sync_interval;
                for _ in 0..(interval.as_secs() / 5).max(1) {
                    if stop.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        })
    }

    /// Register a peer in the registry and immediately trigger an initial sync if sync is enabled.
    pub async fn register_peer_and_sync(&self, peer: PeerInfo) -> anyhow::Result<()> {
        let peer_url = peer.endpoint_url.clone();
        let sync_enabled = peer.sync_enabled;

        {
            let mut reg = self.registry.write().await;
            reg.add_peer(peer)?;
        }

        if sync_enabled {
            let sync_service = self.sync_service.clone();
            tokio::spawn(async move {
                info!("Immediate initial sync triggered for peer {peer_url}");
                if let Err(e) = sync_service.sync_with(&peer_url).await {
                    warn!("Initial sync for peer {peer_url} failed: {e:#}");
                }
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::sync::manifest::tests::TestStore;
    use crate::memory::store::MemoryStore;
    use crate::mesh::node::NodeId;
    use std::collections::HashMap;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_adapter_active_peer_urls() {
        let dir = tempdir().unwrap();
        let storage_path = dir.path().join("peers.json");
        let reg = PeerRegistry::load_from(storage_path).unwrap();
        let registry = Arc::new(RwLock::new(reg));

        let store: Arc<dyn MemoryStore> = Arc::new(TestStore {
            records: std::sync::Mutex::new(Vec::new()),
        });
        let sync_service = Arc::new(PeerMemorySync::new(store, "test-node".to_string()));

        let adapter = PeerRegistrySyncAdapter::new(registry.clone(), sync_service);

        let peer = PeerInfo {
            node_id: NodeId("xv1-p1".to_string()),
            alias: Some("P1".to_string()),
            endpoint_url: "http://localhost:9001".to_string(),
            public_key_hex: "1234".to_string(),
            added_at: 100,
            last_seen_at: Some(chrono::Utc::now().timestamp()),
            sync_enabled: true,
            is_cloud: false,
            iroh_addr: None,
            shared_workspace_ids: Vec::new(),
            shared_workspace_tokens: HashMap::new(),
        };

        adapter.register_peer_and_sync(peer).await.unwrap();

        let urls = adapter.active_peer_urls().await;
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "http://localhost:9001");
    }
}
