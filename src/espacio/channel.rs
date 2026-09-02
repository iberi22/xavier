//! Space channel — Telegram-like text channel per Space (T-03)
//!
//! Append-only log with Loro CRDT semantics (stub). Each message has a
//! monotonic sequence per Space, author node, timestamp and content.
//! Gossip fan-out 3 and offline queue will be wired via Iroh QUIC in a
//! follow-up iteration; this module provides the local log and merge.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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

/// In-memory channel manager per Space. Append-only, CRDT merge via last-write-wins on seq.
#[derive(Debug, Default)]
pub struct ChannelManager {
    /// space_id -> ordered messages
    channels: Arc<RwLock<HashMap<String, Vec<ChannelMessage>>>>,
}

impl ChannelManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a message to a Space channel. Returns the stored message with assigned seq.
    pub async fn post(&self, space_id: String, author: String, content: String) -> ChannelMessage {
        let mut guard = self.channels.write().await;
        let log = guard.entry(space_id.clone()).or_default();
        let seq = log.len() as u64;
        let msg = ChannelMessage {
            seq,
            space_id,
            author,
            content: content.chars().take(4096).collect(),
            created_at: Utc::now(),
        };
        log.push(msg.clone());
        msg
    }

    /// List messages for a Space since `since_seq` (exclusive). Ordered by seq asc.
    pub async fn list_since(&self, space_id: &str, since_seq: u64) -> Vec<ChannelMessage> {
        let guard = self.channels.read().await;
        match guard.get(space_id) {
            Some(log) => log.iter().filter(|m| m.seq > since_seq).cloned().collect(),
            None => Vec::new(),
        }
    }

    /// List all messages for a Space
    pub async fn list_all(&self, space_id: &str) -> Vec<ChannelMessage> {
        let guard = self.channels.read().await;
        guard.get(space_id).cloned().unwrap_or_default()
    }

    /// Merge remote messages into local log (CRDT stub: dedup by seq, keep max seq)
    pub async fn merge(&self, space_id: String, remote: Vec<ChannelMessage>) {
        if remote.is_empty() {
            return;
        }
        let mut guard = self.channels.write().await;
        let log = guard.entry(space_id).or_default();
        let mut max_seq = log.last().map(|m| m.seq).unwrap_or(0);
        // Simple dedup: only append messages with seq > max_seq
        for msg in remote {
            if msg.seq > max_seq {
                max_seq = msg.seq;
                log.push(msg);
            }
        }
        // Keep sorted by seq
        log.sort_by_key(|m| m.seq);
    }

    /// Message count for a Space
    pub async fn len(&self, space_id: &str) -> usize {
        let guard = self.channels.read().await;
        guard.get(space_id).map(|v| v.len()).unwrap_or(0)
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
