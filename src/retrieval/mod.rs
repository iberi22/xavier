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
