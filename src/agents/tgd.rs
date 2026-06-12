//! Textual Gradient Descent (TGD) - Auto-improvement for agents
//!
//! Analyzes the delta between raw conversation history and structured memory (retrieved documents)
//! to generate new behavioral rules in Markdown.

use anyhow::Result;
use tracing::{info, warn};
use crate::agents::runtime::ConversationMessage;
use crate::agents::system1::RetrievedDocument;
use crate::agents::provider::ModelProviderClient;

/// TGD Engine for autonomous rule generation
pub struct TgdEngine {
    provider: ModelProviderClient,
}

impl TgdEngine {
    pub fn new(provider: ModelProviderClient) -> Self {
        Self { provider }
    }

    /// Generates new rules by analyzing the gap between history and context
    pub async fn generate_rules(
        &self,
        history: &[ConversationMessage],
        context: &[RetrievedDocument],
    ) -> Result<String> {
        info!("🚀 TGD: Analyzing delta for rule generation...");

        let history_text = history
            .iter()
            .map(|m| format!("{:?}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");

        let context_text = context
            .iter()
            .map(|d| format!("Source: {}\nContent: {}", d.path, d.content))
            .collect::<Vec<_>>()
            .join("\n---\n");

        let system_prompt = "You are a Meta-Cognitive Optimizer for the Xavier agent system. \
            Your task is to perform Textual Gradient Descent. \
            Analyze the provided raw conversation history and the retrieved memory context. \
            Identify discrepancies, missing knowledge gaps, or behavioral failures. \
            Generate specific, actionable rules in Markdown format that the agent should follow in the future to avoid these issues. \
            Rules should be concise and start with '- '. \
            Return ONLY the Markdown rules.";

        let user_prompt = format!(
            "### Raw History\n{}\n\n### Structured Memory Context\n{}\n\nGenerate improvement rules:",
            history_text, context_text
        );

        match self.provider.generate_text(system_prompt, &user_prompt).await {
            Ok(response) => {
                let rules = response.text.trim().to_string();
                if !rules.is_empty() {
                    info!("✅ TGD: Successfully generated new rules.");
                    self.persist_rules(&rules).await?;
                }
                Ok(rules)
            }
            Err(e) => {
                warn!("❌ TGD: Failed to generate rules: {}", e);
                Err(e)
            }
        }
    }

    /// Persists rules to the local improvement file
    async fn persist_rules(&self, rules: &str) -> Result<()> {
        let path = std::path::Path::new(".xavier/agent_improvements.md");
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let existing = if path.exists() {
            tokio::fs::read_to_string(path).await?
        } else {
            String::new()
        };

        let combined = if existing.is_empty() {
            rules.to_string()
        } else {
            format!("{}\n{}", existing, rules)
        };

        tokio::fs::write(path, combined).await?;
        Ok(())
    }
}
