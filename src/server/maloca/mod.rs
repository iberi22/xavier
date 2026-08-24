//! Maloca server modules.

pub mod alignment_route;
pub mod backlog_route;
pub mod challenge_routes;
pub mod data_node;
pub mod hc_analyzer_bridge;
pub mod hc_cron_bridge;
pub mod live_sync;
pub mod model_routes;
pub mod model_service;
pub mod registry_route;
pub mod rewards;

pub use hc_analyzer_bridge::{ChallengeEmbedder, HcAnalyzerBridge};
pub use hc_cron_bridge::{HcCronBridge, HcCronBridgeConfig};

use axum::Router;
use std::sync::Arc;
use crate::humanchallenge::HumanChallengeStore;

/// Constructs a unified Axum router aggregating all `/v1/maloca/*` endpoints.
pub fn v1_maloca_router(
    challenge_store: Option<Arc<HumanChallengeStore>>,
    workspace_dir: Option<std::path::PathBuf>,
) -> Router {
    let registry_mgr = registry_route::AppRegistryManager::default();
    let challenge_state = challenge_store
        .map(challenge_routes::ChallengeState::new)
        .unwrap_or_else(challenge_routes::ChallengeState::in_memory);
    let mut backlog_svc = backlog_route::UnifiedBacklogService::new();
    if let Some(dir) = workspace_dir {
        backlog_svc = backlog_svc.with_workspace_dir(dir);
    }
    let model_svc = model_routes::ModelRouterService::new();

    Router::new()
        .merge(registry_route::router(registry_mgr))
        .merge(alignment_route::router())
        .merge(backlog_route::router(backlog_svc))
        .merge(model_routes::router(model_svc))
        .merge(challenge_routes::router(challenge_state))
}
