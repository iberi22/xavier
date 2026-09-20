//! Tokio background daemon for purging expired ephemeral messages (WAVE-27.21 / Issue #2399).
//!
//! Spawns an asynchronous periodic worker that cleans expired OTR messages
//! from SQLite using `DELETE FROM telecom_messages WHERE is_ephemeral = 1 AND sent_at < ?`.

use chrono::{Duration as ChronoDuration, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

use crate::telecom::store::{TelecomStore, TelecomStoreError};

/// Configuration options for the ephemeral message cleaner daemon.
#[derive(Debug, Clone)]
pub struct EphemeralCleanupConfig {
    /// Interval between execution passes.
    pub interval: Duration,
    /// Time-to-live (TTL) for ephemeral messages before they become eligible for deletion.
    pub ttl: Duration,
}

impl Default for EphemeralCleanupConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            ttl: Duration::from_secs(3600), // 1 hour default TTL
        }
    }
}

/// Periodic worker daemon managing background deletion of expired ephemeral messages.
pub struct EphemeralCleanupWorker {
    store: Arc<TelecomStore>,
    config: EphemeralCleanupConfig,
    shutdown_tx: Option<watch::Sender<bool>>,
}

impl EphemeralCleanupWorker {
    /// Create a new worker instance.
    pub fn new(store: Arc<TelecomStore>, config: EphemeralCleanupConfig) -> Self {
        Self {
            store,
            config,
            shutdown_tx: None,
        }
    }

    /// Purge expired ephemeral messages directly on the store.
    /// Returns the count of deleted messages.
    pub fn purge_expired(store: &TelecomStore, ttl: Duration) -> Result<usize, TelecomStoreError> {
        let cutoff =
            Utc::now() - ChronoDuration::from_std(ttl).unwrap_or_else(|_| ChronoDuration::hours(1));
        let cutoff_str = cutoff.to_rfc3339();

        let conn = store.connection();
        let conn_guard = conn.lock().unwrap();
        let deleted = conn_guard.execute(
            "DELETE FROM telecom_messages WHERE is_ephemeral != 0 AND sent_at < ?1",
            rusqlite::params![cutoff_str],
        )?;

        debug!(
            "Purged {} expired ephemeral messages older than {}",
            deleted, cutoff_str
        );
        Ok(deleted)
    }

    /// Spawn the periodic cleanup task onto the Tokio runtime.
    pub fn spawn(&mut self) -> JoinHandle<()> {
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        self.shutdown_tx = Some(shutdown_tx);

        let store = self.store.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            info!(
                "Starting EphemeralCleanupWorker with interval {:?} and TTL {:?}",
                config.interval, config.ttl
            );
            let mut ticker = tokio::time::interval(config.interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let store_clone = store.clone();
                        let ttl = config.ttl;
                        // Execute DB deletion in blocking task to avoid stalling the async reactor
                        let res = tokio::task::spawn_blocking(move || {
                            Self::purge_expired(&store_clone, ttl)
                        }).await;

                        match res {
                            Ok(Ok(count)) => {
                                if count > 0 {
                                    info!("Cleaned up {} expired ephemeral messages", count);
                                }
                            }
                            Ok(Err(e)) => error!("Error purging ephemeral messages: {}", e),
                            Err(e) => error!("Join error in ephemeral cleanup worker: {}", e),
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            info!("Shutting down EphemeralCleanupWorker gracefully");
                            break;
                        }
                    }
                }
            }
        })
    }

    /// Stop the background task gracefully.
    pub fn stop(&self) {
        if let Some(ref tx) = self.shutdown_tx {
            let _ = tx.send(true);
        }
    }
}

impl Drop for EphemeralCleanupWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telecom::store::{StoredMessage, StoredRoom};

    #[tokio::test]
    async fn test_purge_expired_ephemeral_messages() {
        let store = Arc::new(TelecomStore::open_in_memory().unwrap());

        // Create room first
        let room = StoredRoom {
            room_id: "room_ephemeral_test".to_string(),
            name: "Ephemeral Test".to_string(),
            clearance: "Secret".to_string(),
            is_group: false,
            created_at: Utc::now(),
        };
        store.persist_room(&room).unwrap();

        // Insert non-ephemeral message
        let permanent_msg = StoredMessage {
            message_id: "msg_permanent".to_string(),
            room_id: "room_ephemeral_test".to_string(),
            sender_node_id: "node_1".to_string(),
            encrypted_payload: b"keep me".to_vec(),
            sequence: 1,
            is_ephemeral: false,
            sent_at: Utc::now() - ChronoDuration::hours(2),
        };
        store.persist_message(&permanent_msg).unwrap();

        // Insert expired ephemeral message (2 hours old)
        let expired_msg = StoredMessage {
            message_id: "msg_expired".to_string(),
            room_id: "room_ephemeral_test".to_string(),
            sender_node_id: "node_1".to_string(),
            encrypted_payload: b"delete me".to_vec(),
            sequence: 2,
            is_ephemeral: true,
            sent_at: Utc::now() - ChronoDuration::hours(2),
        };
        store.persist_message(&expired_msg).unwrap();

        // Insert fresh ephemeral message (10 seconds old)
        let fresh_msg = StoredMessage {
            message_id: "msg_fresh".to_string(),
            room_id: "room_ephemeral_test".to_string(),
            sender_node_id: "node_1".to_string(),
            encrypted_payload: b"keep me too".to_vec(),
            sequence: 3,
            is_ephemeral: true,
            sent_at: Utc::now(),
        };
        store.persist_message(&fresh_msg).unwrap();

        // Purge with 1 hour TTL
        let purged =
            EphemeralCleanupWorker::purge_expired(&store, Duration::from_secs(3600)).unwrap();
        assert_eq!(purged, 1);

        // Fetch remaining messages
        let history = store.fetch_room_history("room_ephemeral_test", 10).unwrap();
        let remaining_ids: Vec<String> = history.into_iter().map(|m| m.message_id).collect();

        assert!(remaining_ids.contains(&"msg_permanent".to_string()));
        assert!(remaining_ids.contains(&"msg_fresh".to_string()));
        assert!(!remaining_ids.contains(&"msg_expired".to_string()));
    }

    #[tokio::test]
    async fn test_ephemeral_worker_spawn_and_shutdown() {
        let store = Arc::new(TelecomStore::open_in_memory().unwrap());
        let config = EphemeralCleanupConfig {
            interval: Duration::from_millis(50),
            ttl: Duration::from_secs(1),
        };

        let mut worker = EphemeralCleanupWorker::new(store, config);
        let handle = worker.spawn();

        tokio::time::sleep(Duration::from_millis(120)).await;
        worker.stop();

        let res = tokio::time::timeout(Duration::from_millis(500), handle).await;
        assert!(res.is_ok(), "Worker did not shut down in time");
    }
}
