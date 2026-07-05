//! Researcher Agent - Scans for new memory techniques and generates hypotheses

use crate::agents::evolve::experiment::{Hypothesis, HypothesisType};
use crate::agents::evolve::gap_analyzer::GapReport;
use crate::agents::runtime::AgentRuntime;
use anyhow::{anyhow, Result};
use tracing::info;

/// Researcher - Generates hypotheses based on literature review, code analysis and gap analysis
pub struct Researcher {
    runtime: Option<AgentRuntime>,
}

impl Researcher {
    pub fn new() -> Self {
        Self { runtime: None }
    }

    pub fn with_runtime(mut self, runtime: AgentRuntime) -> Self {
        self.runtime = Some(runtime);
        self
    }

    /// Generate a hypothesis for the next experiment based on a GapReport
    pub async fn generate_hypothesis(&self, gap_report: &GapReport) -> Result<Hypothesis> {
        if let Some(ref runtime) = self.runtime {
            return self.generate_dynamic_hypothesis(runtime, gap_report).await;
        }

        // Fallback to random hardcoded hypothesis if no runtime is available
        self.generate_fallback_hypothesis().await
    }

    async fn generate_dynamic_hypothesis(
        &self,
        runtime: &AgentRuntime,
        gap_report: &GapReport,
    ) -> Result<Hypothesis> {
        info!("Generating dynamic hypothesis via LLM...");

        let prompt = format!(
            r#"You are a senior system architect for Xavier, a high-performance vector memory system.
Based on the following Gap Report, suggest a single, concrete improvement hypothesis.

GAP REPORT:
- Avg Latency: {}ms
- P95 Latency: {}ms
- Error Rate: {:.2}%
- High Latency Endpoints: {:?}
- Recall Indicators: {:?}
- Critical Modules: {:?}

SECURITY RULES:
- You MUST only suggest changes to files within 'src/'.
- Do NOT suggest changes to security-critical files like 'src/crypto/' or 'src/auth/'.
- Your 'files' list must contain valid paths relative to the repository root.

Output format (JSON):
{{
  "description": "Short description of the change",
  "type": "optimization | hyperparameter | architecture | simplification",
  "files": ["src/list/of/files.rs"],
  "patch": "A diff or specific code change instruction in SEARCH/REPLACE format:
<<<<<<< SEARCH
[existing code]
=======
[new code]
>>>>>>> REPLACE",
  "complexity_cost": 10
}}
"#,
            gap_report.avg_latency_ms,
            gap_report.p95_latency_ms,
            gap_report.error_rate * 100.0,
            gap_report.high_latency_endpoints,
            gap_report.recall_indicators,
            gap_report.critical_modules
        );

        let response = runtime
            .run(&prompt, None, Some("evolve".to_string()))
            .await?;

        // Extract JSON from response (robustly)
        let json_str = self.extract_json(&response.response)?;
        let parsed: serde_json::Value = serde_json::from_str(&json_str)?;

        let h_type = match parsed["type"].as_str().unwrap_or("optimization") {
            "hyperparameter" => HypothesisType::Hyperparameter,
            "architecture" => HypothesisType::Architecture,
            "simplification" => HypothesisType::Simplification,
            _ => HypothesisType::Optimization,
        };

        let mut h = Hypothesis::new(
            parsed["description"]
                .as_str()
                .unwrap_or("Dynamic improvement")
                .to_string(),
            h_type,
        );

        h.files = parsed["files"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        h.patch = parsed["patch"].as_str().unwrap_or_default().to_string();
        h.complexity_cost = parsed["complexity_cost"].as_u64().unwrap_or(0) as usize;

        Ok(h)
    }

    fn extract_json(&self, text: &str) -> Result<String> {
        let json_start = text
            .find('{')
            .ok_or_else(|| anyhow!("No JSON start found in LLM response"))?;
        let json_end = text
            .rfind('}')
            .ok_or_else(|| anyhow!("No JSON end found in LLM response"))?
            + 1;
        Ok(text[json_start..json_end].to_string())
    }

    async fn generate_fallback_hypothesis(&self) -> Result<Hypothesis> {
        let hypotheses = [
            Hypothesis::optimization(
                "add retrieval cache layer".to_string(),
                vec!["src/memory/mod.rs".to_string()],
                r#"
<<<<<<< SEARCH
pub mod results;
=======
pub mod results;
pub mod cache;
>>>>>>> REPLACE
"#
                .to_string(),
                15,
            ),
            Hypothesis::simplification(
                "remove unnecessary cloning in hot path".to_string(),
                vec!["src/memory/qmd_memory.rs".to_string()],
                5,
            ),
            Hypothesis::optimization(
                "use more efficient hash for deduplication".to_string(),
                vec!["src/memory/embedder.rs".to_string()],
                r#"
<<<<<<< SEARCH
use sha2::{Digest, Sha256};

pub fn hash_embedding(embedding: &[f32]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(embedding.as_bytes());
    crate::crypto::hex_encode(&hasher.finalize())
}
=======
use xxhash_rust::xxhash64;

pub fn hash_embedding(embedding: &[f32]) -> u64 {
    xxhash64(embedding.as_bytes(), 0)
}
>>>>>>> REPLACE
"#
                .to_string(),
                3,
            ),
        ];

        let idx =
            (chrono::Utc::now().timestamp() % hypotheses.len() as i64).unsigned_abs() as usize;
        let hypothesis = hypotheses[idx].clone();

        info!(
            hypothesis_id = %hypothesis.id,
            hypothesis_type = %hypothesis.hypothesis_type,
            "🔬 Generated fallback hypothesis"
        );

        Ok(hypothesis)
    }
}

impl Default for Researcher {
    fn default() -> Self {
        Self::new()
    }
}
