//! Textual Gradient Descent (TGD) - Auto-improvement for agents
//!
//! Analyzes the delta between raw conversation history and structured memory (retrieved documents)
//! to generate new behavioral rules in Markdown.

use std::path::PathBuf;
use tokio::sync::Mutex;

use crate::agents::provider::ModelProviderClient;
use crate::agents::runtime::ConversationMessage;
use crate::agents::system1::RetrievedDocument;
use anyhow::Result;
use tracing::{info, warn};

/// Configuration for TGD engine
#[derive(Debug, Clone)]
pub struct TgdConfig {
    /// Confidence threshold below which TGD triggers (default: 0.7)
    pub confidence_threshold: f32,
    /// Path to the improvements rules file
    pub improvements_path: PathBuf,
    /// Maximum number of rules to keep (default: 100)
    pub max_rules_count: usize,
}

impl Default for TgdConfig {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.7,
            improvements_path: PathBuf::from(".xavier/agent_improvements.md"),
            max_rules_count: 100,
        }
    }
}

/// TGD Engine for autonomous rule generation
pub struct TgdEngine {
    provider: ModelProviderClient,
    config: TgdConfig,
    /// Mutex to prevent concurrent read/write to the rules file
    io_lock: Mutex<()>,
}

impl TgdEngine {
    pub fn new(provider: ModelProviderClient) -> Self {
        Self::with_config(provider, TgdConfig::default())
    }

    pub fn with_config(provider: ModelProviderClient, config: TgdConfig) -> Self {
        Self {
            provider,
            config,
            io_lock: Mutex::new(()),
        }
    }

    pub fn config(&self) -> &TgdConfig {
        &self.config
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

        match self
            .provider
            .generate_text(system_prompt, &user_prompt)
            .await
        {
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

    /// Persists rules to the local improvement file using atomic write (tmp + rename).
    async fn persist_rules(&self, rules: &str) -> Result<()> {
        let _lock = self.io_lock.lock().await;

        let path = &self.config.improvements_path;
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

        // Atomic write: write to .tmp file, then rename
        let tmp_path = path.with_extension("md.tmp");
        tokio::fs::write(&tmp_path, combined).await?;
        tokio::fs::rename(&tmp_path, path).await?;

        info!("✅ TGD: Rules persisted to {:?}", path);
        Ok(())
    }
}
