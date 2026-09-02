//! Retrieval module - Multi-layer memory retrieval with adaptive gating
//!
//! This module provides adaptive retrieval gating that combines results from
//! Working, Episodic, and Semantic memory layers using weighted RRF fusion.

pub mod config;
pub mod cross_encoder;
pub mod eval;
pub mod gating;
pub mod history;
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
