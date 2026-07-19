//! Model provider system for Xavier agents.
//!
//! This module provides a unified interface for interacting with different
//! LLM providers, both local and cloud-based.

pub mod anthropic;
pub mod client;
pub mod hardware;
pub mod config;
pub mod gemini;
pub mod local;
pub mod minimax;
pub mod openai;
pub mod rate_limit;
pub mod router;
pub mod traits;
pub mod types;

pub use client::ModelProviderClient;
pub use config::ModelProviderConfig;
pub use rate_limit::{QuotaStatus, RateLimitManager};
pub use traits::LlmProvider;
pub use types::{ApiFlavor, ModelProviderStatus, ProviderMode, LLM_TIMEOUT};

#[cfg(test)]
mod router_tests;
#[cfg(test)]
mod tests;
