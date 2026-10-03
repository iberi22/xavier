//! Space channel — Telegram-like text channel per Space (T-03)
//!
//! Append-only log with Loro CRDT semantics (stub). Each message has a
//! monotonic sequence per Space, author node, timestamp and content.
//! Gossip fan-out 3 and offline queue will be wired via Iroh QUIC in a
//! follow-up iteration; this module provides the local log and merge.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

use super::store::SpaceStores;

/// A message in a Space channel
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelMessage {
    /// Monotonic sequence within the Space (0..)
    pub seq: u64,
    /// Space id
    pub space_id: String,
    /// Author node id
    pub author: String,
    /// Text content (max 4KB)
    pub content: String,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
}

/// Channel manager per Space, backed by each space's `espacio.sqlite`
/// (in-memory databases for `new()`). Append-only, CRDT merge via
/// last-write-wins on seq. Reads always come from the store, so a reopened
/// manager sees every previously posted message in order.
#[derive(Debug, Default)]
pub struct ChannelManager {
    stores: Arc<SpaceStores>,
}

impl ChannelManager {
    /// Non-persistent manager (tests, ephemeral use).
    pub fn new() -> Self {
        Self::default()
    }

    /// Persistent manager over `{root}/spaces/{space_id}/espacio.sqlite`.
    pub fn open(root: impl AsRef<Path>) -> Self {
        Self::with_stores(SpaceStores::open(root))
    }

    /// Manager sharing a store registry (e.g. `SpaceManager::stores()`).
    pub fn with_stores(stores: Arc<SpaceStores>) -> Self {
        Self { stores }
    }

    /// Append a message, reporting storage failures (invalid space id, I/O).
    pub async fn try_post(
        &self,
        space_id: String,
        author: String,
        content: String,
    ) -> Result<ChannelMessage> {
        let content: String = content.chars().take(4096).collect();
        self.stores
            .get(&space_id)?
            .append_message(&space_id, &author, &content)
    }

    /// Append a message to a Space channel. Returns the stored message with assigned seq.
    ///
    /// Infallible for API compatibility: if the store rejects the write (invalid
    /// space id, I/O error) the error is logged and an unpersisted message
    /// with `seq` 0 is returned. Use [`ChannelManager::try_post`] to observe errors.
    pub async fn post(&self, space_id: String, author: String, content: String) -> ChannelMessage {
        let fallback = ChannelMessage {
            seq: 0,
            space_id: space_id.clone(),
            author: author.clone(),
            content: content.chars().take(4096).collect(),
            created_at: Utc::now(),
        };
        match self.try_post(space_id, author, content).await {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("espacio: channel post not persisted: {e}");
                fallback
            }
        }
    }

    fn read(&self, space_id: &str, since: Option<u64>) -> Vec<ChannelMessage> {
        let res = self
            .stores
            .get_existing(space_id)
            .and_then(|s| s.map(|s| s.messages_since(since)).transpose());
        match res {
            Ok(v) => v.unwrap_or_default(),
            Err(e) => {
                tracing::error!("espacio: channel read failed for {space_id}: {e}");
                Vec::new()
            }
        }
    }

    /// List messages for a Space since `since_seq` (exclusive). Ordered by seq asc.
    pub async fn list_since(&self, space_id: &str, since_seq: u64) -> Vec<ChannelMessage> {
        self.read(space_id, Some(since_seq))
    }

    /// List all messages for a Space
    pub async fn list_all(&self, space_id: &str) -> Vec<ChannelMessage> {
        self.read(space_id, None)
    }

    /// Merge remote messages into local log (CRDT stub: dedup by seq, keep max seq)
    pub async fn merge(&self, space_id: String, remote: Vec<ChannelMessage>) {
        if remote.is_empty() {
            return;
        }
        let res = self
            .stores
            .get(&space_id)
            .and_then(|s| s.merge_messages(&space_id, &remote));
        if let Err(e) = res {
            tracing::error!("espacio: channel merge failed for {space_id}: {e}");
        }
    }

    /// Message count for a Space
    pub async fn len(&self, space_id: &str) -> usize {
        let res = self
            .stores
            .get_existing(space_id)
            .and_then(|s| s.map(|s| s.message_count()).transpose());
        res.ok().flatten().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn post_and_list() {
        let mgr = ChannelManager::new();
        let m0 = mgr
            .post("esp_a".into(), "xv1_alice".into(), "hello".into())
            .await;
        assert_eq!(m0.seq, 0);
        let m1 = mgr
            .post("esp_a".into(), "xv1_bob".into(), "world".into())
            .await;
        assert_eq!(m1.seq, 1);
        assert_eq!(mgr.len("esp_a").await, 2);
        let all = mgr.list_all("esp_a").await;
        assert_eq!(all.len(), 2);
        let since0 = mgr.list_since("esp_a", 0).await;
        assert_eq!(since0.len(), 1);
        assert_eq!(since0[0].content, "world");
    }

    #[tokio::test]
    async fn merge_dedup() {
        let mgr = ChannelManager::new();
        mgr.post("esp_a".into(), "n1".into(), "a".into()).await;
        // remote has seq 1 and seq 5 (gap)
        let remote = vec![
            ChannelMessage {
                seq: 1,
                space_id: "esp_a".into(),
                author: "n2".into(),
                content: "b".into(),
                created_at: Utc::now(),
            },
            ChannelMessage {
                seq: 5,
                space_id: "esp_a".into(),
                author: "n2".into(),
                content: "c".into(),
                created_at: Utc::now(),
            },
        ];
        mgr.merge("esp_a".into(), remote).await;
        // should have seq 0 (local) + 1 + 5
        assert_eq!(mgr.len("esp_a").await, 3);
    }

    #[tokio::test]
    async fn empty_space() {
        let mgr = ChannelManager::new();
        assert_eq!(mgr.list_all("unknown").await.len(), 0);
        assert_eq!(mgr.list_since("unknown", 0).await.len(), 0);
    }

    #[tokio::test]
    async fn list_since_boundary_equals_seq() {
        let mgr = ChannelManager::new();
        mgr.post("esp_boundary".into(), "author1".into(), "msg0".into())
            .await;
        mgr.post("esp_boundary".into(), "author2".into(), "msg1".into())
            .await;

        // since_seq == 1 (max_seq) must return empty
        let since_max = mgr.list_since("esp_boundary", 1).await;
        assert!(since_max.is_empty());

        // since_seq == 0 must return seq 1 only
        let since_zero = mgr.list_since("esp_boundary", 0).await;
        assert_eq!(since_zero.len(), 1);
        assert_eq!(since_zero[0].seq, 1);
    }

    #[tokio::test]
    async fn multi_space_seq_isolation() {
        let mgr = ChannelManager::new();
        let m0_s1 = mgr
            .post("space_1".into(), "n1".into(), "s1_m0".into())
            .await;
        let m0_s2 = mgr
            .post("space_2".into(), "n2".into(), "s2_m0".into())
            .await;

        assert_eq!(m0_s1.seq, 0);
        assert_eq!(m0_s2.seq, 0);

        let m1_s1 = mgr
            .post("space_1".into(), "n1".into(), "s1_m1".into())
            .await;
        assert_eq!(m1_s1.seq, 1);

        assert_eq!(mgr.len("space_1").await, 2);
        assert_eq!(mgr.len("space_2").await, 1);
    }
}
