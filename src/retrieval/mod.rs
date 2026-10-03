//! Retrieval module - Multi-layer memory retrieval with adaptive gating
//!
//! This module provides adaptive retrieval gating that combines results from
//! Working, Episodic, and Semantic memory layers using weighted RRF fusion.

pub mod config;
pub mod cross_encoder;
pub mod eval;
pub mod gating;
pub mod history;
#[cfg(feature = "pageindex")]
pub mod pageindex_arm;
pub mod policy;
pub mod regeneration;
pub mod tuner;

pub use cross_encoder::{
    CrossEncoderConfig, CrossEncoderError, CrossEncoderReranker, CrossEncoderRerankerBuilder,
    RerankCandidate, RerankResult, TokenMetrics,
};
pub use gating::{
    AdaptiveGating, Event, GatingConfig, LayerSearchResult, LayerStats, LayerWeights,
    SessionSummary,
};
pub mod navigation;
pub use regeneration::{ContextRegenerator, ContextRegeneratorConfig, RegenerationResult};
pub mod scoring;
pub use policy::{NavigationPolicy, TraversalWeights};

use crate::espacio::manager::SpaceManager;
use crate::espacio::public::{espacio_public_search, PublicConnector};
use crate::search::rrf::ScoredResult;

/// Public-espacio retrieval orchestrator arm: merges public-espacio results
/// into the RAG hit set, returning an empty vector (no-op) when no public
/// espacios exist or no manager/connector is configured.
pub async fn retrieve_public_espacios(
    space_manager: Option<&SpaceManager>,
    public_connector: Option<&PublicConnector>,
    query: &str,
    limit: usize,
    namespace_filter: Option<&str>,
) -> Vec<ScoredResult> {
    espacio_public_search(
        space_manager,
        public_connector,
        query,
        limit,
        namespace_filter,
    )
    .await
}

/// Member-only espacio retrieval arm: descriptor hits for the caller's own
/// space plus the spaces with an ACTIVE inbound link to it (WP-13n). The
/// caller's space id is required; there is no global enumeration.
pub async fn retrieve_visible_espacios(
    space_manager: &SpaceManager,
    caller_space: &str,
    query: &str,
    limit: usize,
) -> Vec<ScoredResult> {
    let mut out = Vec::new();
    for id in crate::espacio::visible_space_ids(space_manager, caller_space).await {
        let Ok(space) = space_manager.get(&id).await else {
            continue;
        };
        let score = crate::espacio::search::score_dataset(
            query,
            &space.name,
            &space.description,
            1.0,
            0,
            0,
        );
        if query.is_empty() || score > 0.05 {
            out.push(ScoredResult {
                id: format!("espacio/{}", space.id),
                content: format!("{}: {}", space.name, space.description),
                score: score as f32,
                source: "espacio_visible".to_string(),
                path: space.storage_path.to_string_lossy().to_string(),
                updated_at: Some(space.created_at.timestamp_millis()),
                zone: None,
            });
        }
    }
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if limit > 0 {
        out.truncate(limit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_retrieve_public_espacios_noop_and_merge() {
        // No-op when manager and connector are None
        let res_none = retrieve_public_espacios(None, None, "test", 10, None).await;
        assert!(res_none.is_empty());

        let mgr = SpaceManager::new(std::env::temp_dir().join("retrieval_espacio_test"));
        mgr.create(
            "esp_ret_pub".into(),
            "Retrieval Public Space".into(),
            "Searchable public space".into(),
            "node1".into(),
            true,
        )
        .await
        .unwrap();

        let res = retrieve_public_espacios(Some(&mgr), None, "Retrieval", 10, None).await;
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].source, "espacio_public");
        assert_eq!(res[0].id, "espacio/esp_ret_pub");

        let _ = mgr.delete("esp_ret_pub").await;
    }
}
