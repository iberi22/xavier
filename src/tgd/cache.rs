// SPDX-License-Identifier: MIT OR LICENSE-MESH
use crate::agents::runtime::ConversationMessage;
use crate::agents::system1::RetrievedDocument;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Cache for TGD executions to avoid redundant analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TgdCache {
    /// SHA256 hash of the last analyzed history and context
    pub last_hash: String,
    /// Timestamp of the last successful TGD run
    pub last_run: DateTime<Utc>,
}

impl TgdCache {
    /// Loads cache from disk, returns default if not found or invalid
    pub async fn load(path: &Path) -> Self {
        if let Ok(content) = tokio::fs::read_to_string(path).await {
            if let Ok(cache) = serde_json::from_str(&content) {
                return cache;
            }
        }
        Self {
            last_hash: String::new(),
            last_run: DateTime::UNIX_EPOCH,
        }
    }

    /// Persists cache to disk
    pub async fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        let content = serde_json::to_string_pretty(self)?;
        tokio::fs::write(path, content).await?;
        Ok(())
    }

    /// Calculates a stable hash for history and context
    pub fn calculate_hash(
        history: &[ConversationMessage],
        context: &[RetrievedDocument],
    ) -> String {
        let mut hasher = Sha256::new();

        hasher.update(b"history:");
        for msg in history {
            hasher.update(msg.id.as_bytes());
            hasher.update(b"|");
            hasher.update(msg.content.as_bytes());
            hasher.update(b"|");
            hasher.update(format!("{:?}", msg.role).as_bytes());
            hasher.update(b";");
        }

        hasher.update(b"context:");
        for doc in context {
            hasher.update(doc.id.as_bytes());
            hasher.update(b"|");
            hasher.update(doc.content.as_bytes());
            hasher.update(b"|");
            hasher.update(doc.path.as_bytes());
            hasher.update(b";");
        }

        crate::crypto::hex_encode(hasher.finalize())
    }

    /// Determines if TGD should be skipped
    pub fn should_skip(&self, current_hash: &str, min_interval_seconds: i64) -> bool {
        // If history/context changed, do not skip
        if self.last_hash != current_hash {
            return false;
        }

        // If history is the same, check if enough time has passed
        let now = Utc::now();
        let elapsed = now.signed_duration_since(self.last_run).num_seconds();

        elapsed < min_interval_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::runtime::MessageRole;
    use tempfile::NamedTempFile;

    fn mock_message(content: &str) -> ConversationMessage {
        ConversationMessage {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::User,
            content: content.to_string(),
            timestamp: Utc::now(),
        }
    }

    fn mock_document(content: &str) -> RetrievedDocument {
        RetrievedDocument {
            id: uuid::Uuid::new_v4().to_string(),
            path: "test.md".to_string(),
            content: content.to_string(),
            relevance_score: 1.0,
            token_count: 10,
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn test_hash_consistency() {
        let history = vec![mock_message("hello")];
        let context = vec![mock_document("world")];

        let hash1 = TgdCache::calculate_hash(&history, &context);
        let hash2 = TgdCache::calculate_hash(&history, &context);

        assert_eq!(hash1, hash2);
        assert!(!hash1.is_empty());
    }

    #[test]
    fn test_hash_changes() {
        let history1 = vec![mock_message("hello")];
        let context1 = vec![mock_document("world")];
        let hash1 = TgdCache::calculate_hash(&history1, &context1);

        let history2 = vec![mock_message("hello there")];
        let hash2 = TgdCache::calculate_hash(&history2, &context1);
        assert_ne!(hash1, hash2);

        let context2 = vec![mock_document("world!")];
        let hash3 = TgdCache::calculate_hash(&history1, &context2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_skip_logic() {
        let cache = TgdCache {
            last_hash: "abc".to_string(),
            last_run: Utc::now() - chrono::Duration::minutes(30),
        };

        // Same hash, within interval -> skip
        assert!(cache.should_skip("abc", 3600));

        // Same hash, outside interval -> don't skip
        assert!(!cache.should_skip("abc", 600));

        // Different hash -> don't skip
        assert!(!cache.should_skip("def", 3600));
    }

    #[tokio::test]
    async fn test_save_load() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();

        let cache = TgdCache {
            last_hash: "test-hash".to_string(),
            last_run: Utc::now(),
        };

        cache.save(path).await.unwrap();

        let loaded = TgdCache::load(path).await;
        assert_eq!(loaded.last_hash, cache.last_hash);
        // Compare timestamps with some tolerance
        assert!((loaded.last_run - cache.last_run).num_seconds().abs() < 2);
    }
}
