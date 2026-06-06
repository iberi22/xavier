//! Model provider system for Xavier agents.
//!
//! This module provides a unified interface for interacting with different
//! LLM providers, both local and cloud-based.

pub mod client;
pub mod config;
pub mod types;
pub mod traits;
pub mod openai;
pub mod anthropic;
pub mod gemini;
pub mod minimax;
pub mod local;
pub mod rate_limit;

pub use client::ModelProviderClient;
pub use config::ModelProviderConfig;
pub use types::{ApiFlavor, ModelProviderStatus, ProviderMode, LLM_TIMEOUT};
pub use traits::LlmProvider;
pub use rate_limit::{RateLimitManager, QuotaStatus};
