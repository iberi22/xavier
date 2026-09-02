//! Public packs connector — free downloadable packs (T-10)
//!
//! BYO CF R2 public bucket + SDC hash anchor + OfferBlock is_free.
//! GET /public/packs?query= lists free packs, hash verified vs SDC.

use crate::espacio::manager::SpaceManager;
use crate::espacio::search::score_dataset;
use crate::search::rrf::ScoredResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Search public espacios and public connector packs for matching items
pub async fn espacio_public_search(
    manager: Option<&SpaceManager>,
    connector: Option<&PublicConnector>,
    query: &str,
    limit: usize,
    namespace_filter: Option<&str>,
) -> Vec<ScoredResult> {
    let mut results = Vec::new();

    if let Some(mgr) = manager {
        let spaces = mgr.list().await;
        for space in spaces {
            if !space.is_public {
                continue;
            }
            if let Some(ns) = namespace_filter {
                if !space.namespace.contains(ns) && !space.id.contains(ns) {
                    continue;
                }
            }

            let score = score_dataset(query, &space.name, &space.description, 1.0, 0, 0);
            if query.is_empty() || score > 0.05 {
                results.push(ScoredResult {
                    id: format!("espacio/{}", space.id),
                    content: format!("{}: {}", space.name, space.description),
                    score: score as f32,
                    source: "espacio_public".to_string(),
                    path: space.storage_path.to_string_lossy().to_string(),
                    updated_at: Some(space.created_at.timestamp_millis()),
                    zone: None,
                });
            }
        }
    }

    if let Some(conn) = connector {
        let packs = conn.list(query).await;
        for pack in packs {
            if !pack.is_free {
                continue;
            }
            if let Some(ns) = namespace_filter {
                if !pack.space_id.contains(ns) && !pack.pack_id.contains(ns) {
                    continue;
                }
            }

            let score = score_dataset(query, &pack.name, &pack.description, 1.0, 0, 0);
            if query.is_empty() || score > 0.05 {
                results.push(ScoredResult {
                    id: format!("espacio_pack/{}", pack.pack_id),
                    content: format!("{}: {}", pack.name, pack.description),
                    score: score as f32,
                    source: "espacio_public".to_string(),
                    path: format!("data/spaces/{}/packs/{}", pack.space_id, pack.pack_id),
                    updated_at: Some(pack.created_at.timestamp_millis()),
                    zone: None,
                });
            }
        }
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if limit > 0 && results.len() > limit {
        results.truncate(limit);
    }
    results
}

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

    #[tokio::test]
    async fn test_espacio_public_search_filters_and_privacy() {
        let mgr = SpaceManager::new(std::env::temp_dir().join("xavier_espacio_search_test"));

        // Public space
        mgr.create(
            "esp_public_1".into(),
            "Public Knowledge".into(),
            "Open dataset space".into(),
            "node1".into(),
            true,
        )
        .await
        .unwrap();

        // Private space
        mgr.create(
            "esp_private_1".into(),
            "Secret Data".into(),
            "Private space".into(),
            "node1".into(),
            false,
        )
        .await
        .unwrap();

        let conn = PublicConnector::new();
        conn.publish(PublicPack {
            pack_id: "pack_pub_1".into(),
            space_id: "esp_public_1".into(),
            name: "Public Pack".into(),
            description: "Free public pack".into(),
            content_hash: "123".into(),
            is_free: true,
            created_at: Utc::now(),
        })
        .await;

        let res = espacio_public_search(Some(&mgr), Some(&conn), "public", 10, None).await;
        assert_eq!(res.len(), 2);
        assert!(res.iter().all(|r| r.source == "espacio_public"));
        assert!(res.iter().any(|r| r.id == "espacio/esp_public_1"));
        assert!(res.iter().any(|r| r.id == "espacio_pack/pack_pub_1"));
        assert!(!res.iter().any(|r| r.id == "espacio/esp_private_1"));

        // Namespace filter
        let res_ns =
            espacio_public_search(Some(&mgr), Some(&conn), "", 10, Some("esp_public_1")).await;
        assert!(!res_ns.is_empty());
        assert!(!res_ns.iter().any(|r| r.id == "espacio/esp_private_1"));

        let _ = mgr.delete("esp_public_1").await;
        let _ = mgr.delete("esp_private_1").await;
    }
}
