//! RAG (Retrieval-Augmented Generation) pipeline for Xavier DocBot.
//!
//! Orchestrates: query → hybrid search → rerank → prompt assembly → LLM → cited answer.

pub mod llm_adapter;
pub mod pipeline;
pub mod prompt;

pub use llm_adapter::{LlmAdapter, LlmBackend, LlmConfig};
pub use pipeline::{DocBotPipeline, PipelineConfig};
pub use prompt::{PromptBuilder, PromptLanguage};
