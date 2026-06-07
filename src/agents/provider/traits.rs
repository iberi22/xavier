use crate::agents::system1::RetrievedDocument;
use anyhow::Result;
use async_trait::async_trait;

/// Common trait for LLM providers.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Generates a text response from the provider.
    async fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        use_cache: bool,
    ) -> Result<String>;

    /// Generates a response based on query and context documents.
    async fn generate_response(&self, query: &str, context: &[RetrievedDocument])
        -> Result<String>;

    /// Generates a hypothetical document for HyDE-like retrieval.
    async fn generate_hypothetical_document(&self, query: &str) -> Result<String>;

    /// Evaluates if the context is sufficient to answer the query.
    async fn evaluate_context(&self, query: &str, context: &[RetrievedDocument]) -> Result<f32>;
}
