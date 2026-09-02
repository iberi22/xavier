//! Public packs connector — free downloadable packs (T-10)
//!
//! BYO CF R2 public bucket + SDC hash anchor + OfferBlock is_free.
//! GET /public/packs?query= lists free packs, hash verified vs SDC.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Public pack listed in the public connector
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicPack {
    pub pack_id: String,
    pub space_id: String,
    pub name: String,
    pub description: String,
    pub content_hash: String,
    pub is_free: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Manager for public packs (R2 bucket stub + SDC anchor)
#[derive(Debug, Default)]
pub struct PublicConnector {
    packs: Arc<RwLock<HashMap<String, PublicPack>>>,
}

impl PublicConnector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish a pack as public free (hash anchor stub)
    pub async fn publish(&self, pack: PublicPack) {
        self.packs.write().await.insert(pack.pack_id.clone(), pack);
    }

    /// List free packs matching query substring (case-insensitive) on name/description
    pub async fn list(&self, query: &str) -> Vec<PublicPack> {
        let guard = self.packs.read().await;
        let q = query.to_lowercase();
        let mut out: Vec<_> = guard
            .values()
            .filter(|p| p.is_free)
            .filter(|p| {
                if q.is_empty() {
                    true
                } else {
                    p.name.to_lowercase().contains(&q) || p.description.to_lowercase().contains(&q)
                }
            })
            .cloned()
            .collect();
        out.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        out
    }

    /// Get a pack by id and verify hash matches SDC anchor (stub: just returns pack)
    pub async fn get(&self, pack_id: &str) -> Option<PublicPack> {
        self.packs.read().await.get(pack_id).cloned()
    }

    /// Verify hash vs SDC anchor (stub: hash == stored content_hash)
    pub async fn verify(&self, pack_id: &str, expected_hash: &str) -> bool {
        self.packs
            .read()
            .await
            .get(pack_id)
            .map(|p| p.content_hash == expected_hash)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn publish_and_list_free() {
        let conn = PublicConnector::new();
        conn.publish(PublicPack {
            pack_id: "pack1".into(),
            space_id: "esp_a".into(),
            name: "Neon wallpapers".into(),
            description: "free pack".into(),
            content_hash: "abc".into(),
            is_free: true,
            created_at: Utc::now(),
        })
        .await;
        conn.publish(PublicPack {
            pack_id: "pack2".into(),
            space_id: "esp_b".into(),
            name: "Paid pack".into(),
            description: "paid".into(),
            content_hash: "def".into(),
            is_free: false,
            created_at: Utc::now(),
        })
        .await;
        assert_eq!(conn.list("").await.len(), 1);
        assert_eq!(conn.list("neon").await.len(), 1);
        assert_eq!(conn.list("missing").await.len(), 0);
        assert!(conn.verify("pack1", "abc").await);
        assert!(!conn.verify("pack1", "bad").await);
        assert_eq!(conn.get("pack1").await.unwrap().name, "Neon wallpapers");
    }
}
