//! Maloca server modules.

pub mod backlog_route;
pub mod challenge_routes;
pub mod data_node;
pub mod hc_analyzer_bridge;
pub mod hc_cron_bridge;
pub mod live_sync;
pub mod model_service;
pub mod registry_route;
pub mod rewards;

pub use hc_analyzer_bridge::{ChallengeEmbedder, HcAnalyzerBridge};
pub use hc_cron_bridge::{HcCronBridge, HcCronBridgeConfig};
