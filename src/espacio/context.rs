//! Space context bridge — RAG and service contexts per Space (T-05)
//!
//! Kinds: rag, code, config, graphs. Each kind has an append-only log with
//! pull-since incremental sync and gossip fan-out stub.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Kind of context shared in a Space
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextKind {
    Rag,
    Code,
    Config,
    Graphs,
}

impl ContextKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rag => "rag",
            Self::Code => "code",
            Self::Config => "config",
            Self::Graphs => "graphs",
        }
    }
}

/// A context entry in a Space
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEntry {
    pub id: String,
    pub space_id: String,
    pub kind: ContextKind,
    pub payload: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
    pub seq: u64,
}

/// Manager for context bridges per Space and kind
#[derive(Debug, Default)]
pub struct ContextBridge {
    /// (space_id, kind) -> ordered entries
    #[allow(clippy::type_complexity)]
    store: Arc<RwLock<HashMap<(String, ContextKind), Vec<ContextEntry>>>>,
}

impl ContextBridge {
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish a context entry. Returns stored entry with assigned seq.
    pub async fn publish(
        &self,
        space_id: String,
        kind: ContextKind,
        author: String,
        payload: String,
    ) -> ContextEntry {
        let mut guard = self.store.write().await;
        let key = (space_id.clone(), kind);
        let log = guard.entry(key).or_default();
        let seq = log.len() as u64;
        let entry = ContextEntry {
            id: ulid::Ulid::new().to_string(),
            space_id,
            kind,
            payload,
            author,
            created_at: Utc::now(),
            seq,
        };
        log.push(entry.clone());
        entry
    }

    /// Pull entries since `since_seq` (exclusive) for a given space and kind
    pub async fn pull_since(
        &self,
        space_id: &str,
        kind: ContextKind,
        since_seq: u64,
    ) -> Vec<ContextEntry> {
        let guard = self.store.read().await;
        match guard.get(&(space_id.to_string(), kind)) {
            Some(log) => log.iter().filter(|e| e.seq > since_seq).cloned().collect(),
            None => Vec::new(),
        }
    }

    /// List all entries for a space/kind
    pub async fn list(&self, space_id: &str, kind: ContextKind) -> Vec<ContextEntry> {
        let guard = self.store.read().await;
        guard
            .get(&(space_id.to_string(), kind))
            .cloned()
            .unwrap_or_default()
    }

    /// Snippet view: first 100 chars of payload + seq
    pub fn snippet(entry: &ContextEntry) -> String {
        entry.payload.chars().take(100).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_and_pull() {
        let bridge = ContextBridge::new();
        let e0 = bridge
            .publish(
                "esp_a".into(),
                ContextKind::Rag,
                "n1".into(),
                "hello rag".into(),
            )
            .await;
        assert_eq!(e0.seq, 0);
        let e1 = bridge
            .publish(
                "esp_a".into(),
                ContextKind::Rag,
                "n2".into(),
                "world".into(),
            )
            .await;
        assert_eq!(e1.seq, 1);
        let since0 = bridge.pull_since("esp_a", ContextKind::Rag, 0).await;
        assert_eq!(since0.len(), 1);
        assert_eq!(since0[0].payload, "world");
        // different kind isolated
        assert_eq!(bridge.list("esp_a", ContextKind::Code).await.len(), 0);
    }

    #[tokio::test]
    async fn snippet_truncates() {
        let entry = ContextEntry {
            id: "1".into(),
            space_id: "esp_a".into(),
            kind: ContextKind::Rag,
            payload: "a".repeat(200),
            author: "n1".into(),
            created_at: Utc::now(),
            seq: 0,
        };
        assert_eq!(ContextBridge::snippet(&entry).len(), 100);
    }

    #[test]
    fn kind_str() {
        assert_eq!(ContextKind::Rag.as_str(), "rag");
        assert_eq!(ContextKind::Graphs.as_str(), "graphs");
    }
}
