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
use tracing::{debug, info, warn};
pub mod cache;
pub mod consolidation;

use crate::tgd::cache::TgdCache;
pub use consolidation::TgdConsolidationScheduler;

/// Configuration for the Textual Gradient Descent (TGD) engine.
#[derive(Debug, Clone)]
pub struct TgdConfig {
    /// Confidence threshold (0.0-1.0) below which TGD triggers rule generation.
    /// If average retrieval relevance is below this, the agent considers it a "gap".
    pub confidence_threshold: f32,
    /// Path to the Markdown file where generated improvement rules are persisted.
    pub improvements_path: PathBuf,
    /// Maximum number of rules to maintain in the improvements file.
    pub max_rules_count: usize,
    /// Path to the JSON file used for TGD execution caching.
    pub cache_path: PathBuf,
    /// Minimum interval between successive TGD executions to avoid LLM spam.
    pub min_interval_seconds: i64,
    /// Learning rate for textual refinement (0.1 = subtle, 1.0 = aggressive).
    pub learning_rate: f32,
    /// Number of LLM iterations to perform during memory content refinement.
    pub iterations: usize,
}

impl Default for TgdConfig {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.7,
            improvements_path: PathBuf::from(".xavier/tgd.md"),
            max_rules_count: 100,
            cache_path: PathBuf::from(".xavier/tgd_cache.json"),
            min_interval_seconds: 3600,
            learning_rate: 0.1,
            iterations: 3,
        }
    }
}

use crate::agents::provider::LlmProvider;
use std::sync::Arc;

/// TGD Engine for autonomous rule generation and memory refinement.
///
/// The `TgdEngine` implements "Textual Gradient Descent", a process where an LLM
/// analyzes the delta between conversation history and retrieved context to
/// generate behavioral rules or refine existing memory content.
pub struct TgdEngine {
    provider: Arc<dyn LlmProvider>,
    config: TgdConfig,
    /// Mutex to prevent concurrent read/write to the rules file.
    io_lock: Mutex<()>,
}

impl TgdEngine {
    /// Creates a new TGD engine with the default configuration.
    pub fn new(provider: ModelProviderClient) -> Self {
        Self::with_config(Arc::new(provider), TgdConfig::default())
    }

    /// Creates a new TGD engine with an explicit configuration.
    pub fn with_config(provider: Arc<dyn LlmProvider>, config: TgdConfig) -> Self {
        Self {
            provider,
            config,
            io_lock: Mutex::new(()),
        }
    }

    /// Returns a reference to the engine's configuration.
    pub fn config(&self) -> &TgdConfig {
        &self.config
    }

    /// Generates new behavioral rules by analyzing the gap between conversation history and retrieved context.
    ///
    /// If the average relevance of retrieved documents is below the `confidence_threshold`,
    /// the engine asks an LLM to identify what was missing and generate Markdown rules.
    pub async fn generate_rules(
        &self,
        history: &[ConversationMessage],
        context: &[RetrievedDocument],
    ) -> Result<String> {
        let current_hash = TgdCache::calculate_hash(history, context);
        let mut cache = TgdCache::load(&self.config.cache_path).await;

        if cache.should_skip(&current_hash, self.config.min_interval_seconds) {
            info!("⏭️ TGD: Skipping re-execution (cache hit and interval not elapsed)");
            return Ok(String::new());
        }

        // [G3] Confidence threshold application
        if !context.is_empty() {
            let avg_relevance: f32 =
                context.iter().map(|d| d.relevance_score).sum::<f32>() / context.len() as f32;
            if avg_relevance >= self.config.confidence_threshold {
                info!(
                    "⏭️ TGD: Skipping re-execution (Average relevance {:.2} >= threshold {:.2})",
                    avg_relevance, self.config.confidence_threshold
                );
                // Update and save cache to avoid redundant threshold checks
                cache.last_hash = current_hash;
                cache.last_run = chrono::Utc::now();
                if let Err(e) = cache.save(&self.config.cache_path).await {
                    warn!("⚠️ TGD: Failed to save cache: {}", e);
                }
                return Ok(String::new());
            }
        }

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
            .generate_text(system_prompt, &user_prompt, false)
            .await
        {
            Ok(response) => {
                let rules = response.text.trim().to_string();
                if !rules.is_empty() {
                    info!("✅ TGD: Successfully generated new rules.");
                    self.persist_rules(&rules).await?;
                }

                // Update and save cache after successful run (even if no rules found)
                cache.last_hash = current_hash;
                cache.last_run = chrono::Utc::now();
                if let Err(e) = cache.save(&self.config.cache_path).await {
                    warn!("⚠️ TGD: Failed to save cache: {}", e);
                }

                Ok(rules)
            }
            Err(e) => {
                warn!("❌ TGD: Failed to generate rules: {}", e);
                Err(e)
            }
        }
    }

    /// Refines the content of a memory document using iterative Textual Gradient Descent.
    ///
    /// This process repeatedly asks an LLM to improve the clarity, structure, and
    /// density of the provided content, scoring each iteration to ensure improvement.
    pub async fn refine_memory_content(
        &self,
        content: &str,
        iterations: Option<usize>,
    ) -> Result<(String, f32)> {
        let iterations = iterations.unwrap_or(self.config.iterations);
        let mut current_content = content.to_string();
        let mut total_score = 0.0;

        info!(
            "🧠 TGD: Starting memory refinement ({} iterations)...",
            iterations
        );

        let learning_rate = self.config.learning_rate;

        for i in 0..iterations {
            let system_prompt = format!("You are a Textual Gradient Descent (TGD) optimizer. \
                Your goal is to refine the provided memory content to be more accurate, concise, and structured. \
                Analyze the current version and apply 'gradients' by improving clarity and removing redundancy. \
                Apply a learning rate of {} to your refinements (where 0.1 is subtle and 1.0 is aggressive). \
                Return ONLY the refined Markdown content.", learning_rate);

            let user_prompt = format!(
                "### Current Memory Content\n{}\n\nRefine the content:",
                current_content
            );

            let response = self
                .provider
                .generate_text(&system_prompt, &user_prompt, false)
                .await?;
            let refined = response.text.trim().to_string();

            // Evaluation step (simplified for now: calculate a 'gradient' improvement score)
            // In a real TGD loop, we'd use a loss function. Here we use LLM-based evaluation.
            let eval_system = "You are an evaluator for memory quality. \
                Rate the following memory content on a scale from 0.0 to 1.0 based on clarity, structure, and density of information. \
                Return ONLY the numeric score.";

            let eval_response = self
                .provider
                .generate_text(eval_system, &refined, false)
                .await?;
            let score: f32 = eval_response.text.trim().parse().unwrap_or(0.5);

            debug!("TGD iteration {}: score={:.2}", i + 1, score);

            // Only update if it's an improvement or first iteration
            if i == 0 || score > (total_score / i as f32) {
                current_content = refined;
            }
            total_score += score;
        }

        let avg_score = total_score / iterations as f32;
        info!("✅ TGD: Refinement complete. Avg score: {:.2}", avg_score);

        Ok((current_content, avg_score))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::provider::types::LlmResponse;
    use crate::agents::provider::LlmProvider;
    use crate::agents::runtime::MessageRole;
    use async_trait::async_trait;
    use std::sync::Arc;

    struct MockProvider;

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn generate_text(&self, _s: &str, _u: &str, _c: bool) -> Result<LlmResponse> {
            Ok(LlmResponse {
                text: "- Rule 1\n- Rule 2".to_string(),
                quota: None,
            })
        }
        async fn generate_response(
            &self,
            _q: &str,
            _c: &[RetrievedDocument],
        ) -> Result<LlmResponse> {
            unimplemented!()
        }
        async fn generate_hypothetical_document(&self, _q: &str) -> Result<LlmResponse> {
            unimplemented!()
        }
        async fn evaluate_context(&self, _q: &str, _c: &[RetrievedDocument]) -> Result<f32> {
            unimplemented!()
        }
    }

    fn mock_message(content: &str) -> ConversationMessage {
        ConversationMessage {
            id: "msg".to_string(),
            role: MessageRole::User,
            content: content.to_string(),
            timestamp: chrono::Utc::now(),
        }
    }

    fn mock_document(content: &str, score: f32) -> RetrievedDocument {
        RetrievedDocument {
            id: "doc".to_string(),
            path: "test.md".to_string(),
            content: content.to_string(),
            relevance_score: score,
            token_count: 10,
            metadata: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn test_tgd_confidence_threshold() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cache_path = temp_dir.path().join("cache.json");
        let rules_path = temp_dir.path().join("rules.md");

        let tgd_config = TgdConfig {
            confidence_threshold: 0.8,
            cache_path,
            improvements_path: rules_path,
            ..Default::default()
        };

        let provider = Arc::new(MockProvider);
        let tgd = TgdEngine::with_config(provider, tgd_config);

        let history = vec![mock_message("hello")];

        // 1. High confidence -> Skip
        let context_high = vec![mock_document("world1", 0.9)];
        let rules = tgd.generate_rules(&history, &context_high).await.unwrap();
        assert!(
            rules.is_empty(),
            "Should skip generation when confidence is high"
        );

        // 2. Low confidence -> Execute
        // Use a DIFFERENT document to avoid cache hit from step 1
        let context_low = vec![mock_document("world2", 0.5)];
        let rules = tgd.generate_rules(&history, &context_low).await.unwrap();
        assert!(
            !rules.is_empty(),
            "Should generate rules when confidence is low"
        );
        assert!(rules.contains("- Rule 1"));
    }

    #[tokio::test]
    async fn test_tgd_incremental_cache() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cache_path = temp_dir.path().join("cache.json");
        let rules_path = temp_dir.path().join("rules.md");

        let tgd_config = TgdConfig {
            confidence_threshold: 1.0, // Force execution
            cache_path,
            improvements_path: rules_path,
            min_interval_seconds: 3600,
            ..Default::default()
        };

        let provider = Arc::new(MockProvider);
        let tgd = TgdEngine::with_config(provider, tgd_config);

        let history = vec![mock_message("hello")];
        let context = vec![mock_document("world", 0.5)];

        // First run
        let rules1 = tgd.generate_rules(&history, &context).await.unwrap();
        assert!(!rules1.is_empty());

        // Second run with same content -> Skip due to cache
        let rules2 = tgd.generate_rules(&history, &context).await.unwrap();
        assert!(rules2.is_empty(), "Should skip due to cache hit");

        // Third run with different content -> Execute
        let history2 = vec![mock_message("hello there")];
        let rules3 = tgd.generate_rules(&history2, &context).await.unwrap();
        assert!(!rules3.is_empty(), "Should execute when content changes");
    }

    #[tokio::test]
    async fn test_tgd_edge_cases() {
        let temp_dir = tempfile::tempdir().unwrap();
        let tgd_config = TgdConfig {
            confidence_threshold: 1.0,
            cache_path: temp_dir.path().join("cache.json"),
            improvements_path: temp_dir.path().join("rules.md"),
            ..Default::default()
        };

        let provider = Arc::new(MockProvider);
        let tgd = TgdEngine::with_config(provider, tgd_config);

        // Empty history/context
        let rules = tgd.generate_rules(&[], &[]).await.unwrap();
        assert!(!rules.is_empty()); // Still calls provider but with empty prompts

        // Duplicate content in history
        let history = vec![mock_message("hello"), mock_message("hello")];
        let rules = tgd.generate_rules(&history, &[]).await.unwrap();
        assert!(!rules.is_empty());
    }
}
