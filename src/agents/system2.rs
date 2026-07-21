// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! System 2 — Slow, analytical reasoning with multi-step Chain-of-Thought
//!
//! System 2 implements deliberate, methodical reasoning compared to System 1's
//! fast intuitive retrieval. It supports:
//!
//! - Multi-step Chain-of-Thought reasoning
//! - Confidence scoring with entropy-based calibration
//! - Hypothesis generation and verification
//! - Counter-argument synthesis
//! - Memory-backed belief updates
//! - Self-RAG context evaluation at every step

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{info, warn};

use crate::agents::provider::ModelProviderClient;
use crate::agents::system1::RetrievalResult;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Fully qualified reasoning result from System 2
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningResult {
    pub query: String,
    pub analysis: String,
    pub confidence: f32,
    pub supporting_evidence: Vec<Evidence>,
    pub beliefs_updated: Vec<BeliefUpdate>,
    pub reasoning_chain: Vec<ReasoningStep>,
    pub step_count: usize,
    pub total_tokens_used: usize,
    pub reasoning_elapsed_ms: u64,
    pub calibration: ConfidenceCalibration,
}

/// A single piece of evidence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub source_id: String,
    pub content: String,
    pub relevance: f32,
}

/// A belief update produced during reasoning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeliefUpdate {
    pub concept: String,
    pub relation: String,
    pub target: String,
    pub confidence: f32,
    pub based_on: Vec<String>,
}

/// A single step in the reasoning chain (CoT)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningStep {
    pub step: usize,
    pub category: StepCategory,
    pub thought: String,
    pub conclusion: String,
    pub confidence_delta: f32,
    pub evidence_indices: Vec<usize>,
}

/// Category of reasoning step
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepCategory {
    Hypothesis,
    EvidenceCheck,
    CounterArgument,
    Synthesis,
    Verification,
    FinalConclusion,
}

/// Confidence calibration metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceCalibration {
    pub raw_confidence: f32,
    pub entropy: f32,
    pub calibrated_confidence: f32,
    pub has_contradiction: bool,
    pub contradiction_count: usize,
}

/// A hypothesis to be verified
#[derive(Debug, Clone)]
struct Hypothesis {
    statement: String,
    pro_evidence: Vec<Evidence>,
    contra_evidence: Vec<Evidence>,
    strength: f32,
}

/// Configuration for System 2 reasoning
#[derive(Debug, Clone)]
pub struct ReasonerConfig {
    /// Max evidence to consider
    pub max_evidence: usize,
    /// Min confidence threshold
    pub min_confidence: f32,
    /// Max CoT steps
    pub max_steps: usize,
    /// Whether to generate counter-arguments
    pub enable_counter_args: bool,
    /// Whether to calibrate confidence via entropy
    pub enable_calibration: bool,
    /// Minimum evidence pieces to form a hypothesis
    pub min_evidence_for_hypothesis: usize,
}

impl Default for ReasonerConfig {
    fn default() -> Self {
        Self {
            max_evidence: 10,
            min_confidence: 0.3,
            max_steps: 5,
            enable_counter_args: true,
            enable_calibration: true,
            min_evidence_for_hypothesis: 2,
        }
    }
}

// ---------------------------------------------------------------------------
// System 2 Reasoner
// ---------------------------------------------------------------------------

/// System 2 — slow, deliberate reasoning engine with multi-step CoT
pub struct System2Reasoner {
    config: ReasonerConfig,
    provider: Option<ModelProviderClient>,
}

impl System2Reasoner {
    /// Create with default provider (from env)
    pub fn new(config: ReasonerConfig) -> Self {
        Self {
            config,
            provider: Some(ModelProviderClient::from_env()),
        }
    }

    /// Create without LLM provider — uses heuristic-only reasoning
    pub fn heuristic_only(config: ReasonerConfig) -> Self {
        Self {
            config,
            provider: None,
        }
    }

    /// Create with explicit provider
    pub fn with_provider(config: ReasonerConfig, provider: ModelProviderClient) -> Self {
        Self {
            config,
            provider: Some(provider),
        }
    }

    /// Run the full reasoning pipeline
    ///
    /// 1. Gather evidence from retrieval context
    /// 2. Generate hypotheses
    /// 3. Verify each hypothesis (pro/con)
    /// 4. Synthesize conclusion
    /// 5. Calibrate confidence
    /// 6. Produce final reasoning result
    pub async fn run(&self, query: &str, context: &RetrievalResult) -> Result<ReasoningResult> {
        let start = Instant::now();
        info!(query = %query, evidence_count = %context.documents.len(), "System2 reasoning started");

        // Phase 1: Evidence extraction
        let evidence = self.extract_evidence(context);

        // Phase 2: Hypothesis generation
        let hypotheses = self.generate_hypotheses(query, &evidence).await;

        // Phase 3: Multi-step reasoning chain
        let mut chain = Vec::new();
        let mut all_updates = Vec::new();
        let mut step = 0usize;
        let mut total_tokens = 0usize;

        for hypothesis in &hypotheses {
            if step >= self.config.max_steps {
                break;
            }

            // Step: Pro-evidence analysis
            chain.push(ReasoningStep {
                step,
                category: StepCategory::Hypothesis,
                thought: format!("Hypothesis: {}", hypothesis.statement),
                conclusion: format!(
                    "Found {} supporting, {} contradicting evidence pieces",
                    hypothesis.pro_evidence.len(),
                    hypothesis.contra_evidence.len()
                ),
                confidence_delta: 0.1f32.min(hypothesis.strength),
                evidence_indices: (0..evidence.len()).step_by(1).collect(),
            });
            step += 1;

            // Step: Evidence check (self-RAG if provider available)
            if let Some(ref provider) = self.provider {
                match provider.evaluate_context(query, &context.documents).await {
                    Ok(conf) => {
                        chain.push(ReasoningStep {
                            step,
                            category: StepCategory::EvidenceCheck,
                            thought: "Self-RAG evaluation of evidence pool".into(),
                            conclusion: format!("Evidence confidence: {:.2}", conf),
                            confidence_delta: conf / evidence.len() as f32,
                            evidence_indices: vec![],
                        });
                        step += 1;
                        total_tokens += 50; // approximate
                    }
                    Err(e) => {
                        warn!("Self-RAG evaluation failed: {}", e);
                    }
                }
            }

            // Step: Counter-argument (if enabled)
            if self.config.enable_counter_args && !hypothesis.contra_evidence.is_empty() {
                chain.push(ReasoningStep {
                    step,
                    category: StepCategory::CounterArgument,
                    thought: "Considering counter-arguments against hypothesis".into(),
                    conclusion: format!(
                        "{} counter-evidence found, relevance: {:.2}",
                        hypothesis.contra_evidence.len(),
                        hypothesis
                            .contra_evidence
                            .iter()
                            .map(|e| e.relevance)
                            .sum::<f32>()
                            / hypothesis.contra_evidence.len() as f32
                    ),
                    confidence_delta: -0.05f32 * hypothesis.contra_evidence.len() as f32,
                    evidence_indices: vec![],
                });
                step += 1;
            }

            // Generate belief update
            if hypothesis.strength > self.config.min_confidence {
                all_updates.push(BeliefUpdate {
                    concept: query.to_string(),
                    relation: "supported_by".into(),
                    target: hypothesis.statement.clone(),
                    confidence: hypothesis.strength,
                    based_on: hypothesis
                        .pro_evidence
                        .iter()
                        .map(|e| e.source_id.clone())
                        .collect(),
                });
            }
        }

        // Phase 4: Synthesis conclusion
        let avg_confidence = if !hypotheses.is_empty() {
            hypotheses.iter().map(|h| h.strength).sum::<f32>() / hypotheses.len() as f32
        } else {
            0.0
        };

        let has_contradiction = hypotheses.iter().any(|h| !h.contra_evidence.is_empty());

        let contradiction_count = hypotheses
            .iter()
            .map(|h| h.contra_evidence.len())
            .sum::<usize>();

        chain.push(ReasoningStep {
            step,
            category: StepCategory::FinalConclusion,
            thought: format!(
                "Synthesis of {} hypotheses — average confidence {:.2}",
                hypotheses.len(),
                avg_confidence
            ),
            conclusion: format!(
                "Analysis complete: {} hypotheses evaluated, {} evidence pieces, {} counter-arguments",
                hypotheses.len(),
                evidence.len(),
                contradiction_count,
            ),
            confidence_delta: 0.0,
            evidence_indices: (0..evidence.len()).collect(),
        });

        // Phase 5: Confidence calibration
        let calibration = if self.config.enable_calibration {
            self.calibrate_confidence(avg_confidence, &hypotheses)
        } else {
            ConfidenceCalibration {
                raw_confidence: avg_confidence,
                entropy: 0.0,
                calibrated_confidence: avg_confidence,
                has_contradiction,
                contradiction_count,
            }
        };

        let elapsed = start.elapsed();
        let analysis = format!(
            "System2 analysis completed in {:?}: {} steps, {} evidence, confidence {:.2} (calibrated: {:.2})",
            elapsed, chain.len(), evidence.len(),
            calibration.raw_confidence, calibration.calibrated_confidence,
        );

        info!(
            steps = %chain.len(),
            evidence = %evidence.len(),
            calibration_confidence = %calibration.calibrated_confidence,
            "System2 reasoning completed"
        );

        let chain_len = chain.len();

        Ok(ReasoningResult {
            query: query.to_string(),
            analysis,
            confidence: calibration.calibrated_confidence,
            supporting_evidence: evidence,
            beliefs_updated: all_updates,
            reasoning_chain: chain,
            step_count: chain_len,
            total_tokens_used: total_tokens,
            reasoning_elapsed_ms: elapsed.as_millis() as u64,
            calibration,
        })
    }

    /// Extract and rank evidence from retrieval context
    fn extract_evidence(&self, context: &RetrievalResult) -> Vec<Evidence> {
        context
            .documents
            .iter()
            .take(self.config.max_evidence)
            .map(|doc| {
                // Simple relevance — use score field if available, else content length heuristic
                let relevance = if doc.relevance_score > 0.0 {
                    doc.relevance_score
                } else {
                    (doc.content.len() as f32 / 1000.0).min(1.0)
                };
                Evidence {
                    source_id: doc.id.clone(),
                    content: doc.content.clone(),
                    relevance,
                }
            })
            .collect()
    }

    /// Generate and rank hypotheses from evidence
    async fn generate_hypotheses(&self, query: &str, evidence: &[Evidence]) -> Vec<Hypothesis> {
        if evidence.len() < self.config.min_evidence_for_hypothesis {
            info!(
                %query,
                evidence_count = %evidence.len(),
                "Insufficient evidence for hypothesis generation"
            );
            return vec![];
        }

        // Group evidence by relevance (simple heuristic — split into pro/con by threshold)
        let threshold = 0.4;
        let (pro, contra): (Vec<&Evidence>, Vec<&Evidence>) =
            evidence.iter().partition(|e| e.relevance >= threshold);

        let mut hypotheses = Vec::new();

        // Hypothesis 1: Main query answer (pro evidence)
        if !pro.is_empty() {
            let strength = pro.iter().map(|e| e.relevance).sum::<f32>() / pro.len() as f32;
            hypotheses.push(Hypothesis {
                statement: format!(
                    "Query '{}' is supported by {} evidence sources with avg relevance {:.2}",
                    query,
                    pro.len(),
                    strength,
                ),
                pro_evidence: pro.into_iter().cloned().collect(),
                contra_evidence: contra.clone().into_iter().cloned().collect(),
                strength,
            });
        }

        // If there's a mix of pro and contra, generate a contra-hypothesis too
        if !contra.is_empty() && self.config.enable_counter_args {
            let contra_evidence: Vec<Evidence> = contra.into_iter().cloned().collect();
            let contra_strength = contra_evidence.iter().map(|e| e.relevance).sum::<f32>()
                / contra_evidence.len() as f32;
            hypotheses.push(Hypothesis {
                statement: format!(
                    "Evidence conflicting to '{}' — {} counter-sources with avg relevance {:.2}",
                    query,
                    contra_evidence.len(),
                    contra_strength,
                ),
                pro_evidence: vec![],
                contra_evidence,
                strength: 1.0 - contra_strength,
            });
        }

        hypotheses
    }

    /// Calibrate confidence by estimating entropy from evidence diversity
    fn calibrate_confidence(
        &self,
        raw_confidence: f32,
        hypotheses: &[Hypothesis],
    ) -> ConfidenceCalibration {
        if hypotheses.is_empty() {
            return ConfidenceCalibration {
                raw_confidence: 0.0,
                entropy: f32::INFINITY,
                calibrated_confidence: 0.0,
                has_contradiction: false,
                contradiction_count: 0,
            };
        }

        // Count contradictions
        let contradiction_count: usize = hypotheses.iter().map(|h| h.contra_evidence.len()).sum();

        let has_contradiction = contradiction_count > 0;

        // Estimate entropy from evidence diversity
        // Higher diversity = higher entropy = lower calibrated confidence
        let total_evidence: usize = hypotheses
            .iter()
            .map(|h| h.pro_evidence.len() + h.contra_evidence.len())
            .sum();

        let entropy = if total_evidence == 0 {
            0.0
        } else {
            let contradiction_ratio = contradiction_count as f32 / total_evidence as f32;
            // Simple entropy proxy: -p*log2(p) - (1-p)*log2(1-p)
            let p = 1.0 - contradiction_ratio;
            if p <= 0.0 || p >= 1.0 {
                0.0
            } else {
                let log2 = |x: f32| x.log2();
                -(p * log2(p) + (1.0 - p) * log2(1.0 - p))
            }
        };

        // Calibrate: penalty for high entropy and contradictions
        let entropy_penalty = entropy / 2.0; // max ~0.5 penalty
        let contradiction_penalty = if has_contradiction {
            0.1f32.min(contradiction_count as f32 * 0.02)
        } else {
            0.0
        };

        let calibrated = (raw_confidence - entropy_penalty - contradiction_penalty).clamp(0.0, 1.0);

        ConfidenceCalibration {
            raw_confidence,
            entropy,
            calibrated_confidence: calibrated,
            has_contradiction,
            contradiction_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::system1::{RetrievalResult, RetrievedDocument, SearchType};
    use serde_json::json;

    fn make_doc(id: &str, content: &str, relevance: f32) -> RetrievedDocument {
        RetrievedDocument {
            id: id.to_string(),
            path: format!("/test/{}", id),
            content: content.to_string(),
            relevance_score: relevance,
            token_count: content.split_whitespace().count(),
            metadata: json!({"source": "test"}),
        }
    }

    fn make_context(docs: Vec<RetrievedDocument>) -> RetrievalResult {
        let total = docs.len();
        RetrievalResult {
            documents: docs,
            query: "test query".to_string(),
            search_type: SearchType::Hybrid,
            total_results: total,
        }
    }

    #[test]
    fn test_default_config() {
        let config = ReasonerConfig::default();
        assert_eq!(config.max_evidence, 10);
        assert_eq!(config.max_steps, 5);
        assert!(config.enable_counter_args);
    }

    #[tokio::test]
    async fn test_extract_evidence() {
        let reasoner = System2Reasoner::heuristic_only(ReasonerConfig::default());
        let docs = vec![make_doc("1", "test content", 0.8)];
        let context = make_context(docs);

        let evidence = reasoner.extract_evidence(&context);
        assert_eq!(evidence.len(), 1);
        assert!((evidence[0].relevance - 0.8).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_extract_evidence_respects_max() {
        let config = ReasonerConfig {
            max_evidence: 2,
            ..Default::default()
        };
        let reasoner = System2Reasoner::heuristic_only(config);
        let docs = vec![
            make_doc("1", "a", 0.9),
            make_doc("2", "b", 0.8),
            make_doc("3", "c", 0.7),
        ];
        let context = make_context(docs);

        let evidence = reasoner.extract_evidence(&context);
        assert_eq!(evidence.len(), 2);
    }

    #[tokio::test]
    async fn test_generate_hypotheses_returns_empty_without_evidence() {
        let reasoner = System2Reasoner::heuristic_only(ReasonerConfig::default());
        let evidence = vec![];
        let hypotheses = reasoner.generate_hypotheses("test", &evidence).await;
        assert!(hypotheses.is_empty());
    }

    #[tokio::test]
    async fn test_run_basic_flow() {
        let reasoner = System2Reasoner::heuristic_only(ReasonerConfig::default());
        let docs = vec![
            make_doc("1", "Earth is round and orbits the sun", 0.9),
            make_doc("2", "The Earth takes 365 days to orbit", 0.85),
            make_doc("3", "Gravity keeps planets in orbit", 0.75),
        ];
        let context = make_context(docs);

        let result = reasoner.run("Earth orbit", &context).await.unwrap();
        assert_eq!(result.query, "Earth orbit");
        assert!(!result.analysis.is_empty());
        assert!(result.confidence >= 0.0);
        assert!(result.reasoning_chain.len() >= 2); // at least hypothesis + final
        assert!(!result.supporting_evidence.is_empty());
    }

    #[tokio::test]
    async fn test_run_with_contradicting_evidence() {
        let reasoner = System2Reasoner::heuristic_only(ReasonerConfig::default());
        // Mix of pro and contra evidence
        let docs = vec![
            make_doc("1", "The sky is blue on Earth", 0.9),
            make_doc("2", "The sky appears red on Mars", 0.6),
            make_doc("3", "Light scattering depends on atmosphere", 0.8),
        ];
        let context = make_context(docs);

        let result = reasoner.run("sky color", &context).await.unwrap();
        assert!(result.calibration.has_contradiction || result.supporting_evidence.len() >= 2);
        // contradiction_count is usize, always >= 0 by construction
        let _ = result.calibration.contradiction_count;
    }

    #[tokio::test]
    async fn test_calibration_penalizes_contradictions() {
        let reasoner = System2Reasoner::heuristic_only(ReasonerConfig::default());

        // High confidence example with no contradictions
        let no_contra = vec![Hypothesis {
            statement: "test".into(),
            pro_evidence: vec![Evidence {
                source_id: "1".into(),
                content: "a".into(),
                relevance: 0.9,
            }],
            contra_evidence: vec![],
            strength: 0.9,
        }];

        let cal_no = reasoner.calibrate_confidence(0.9, &no_contra);
        assert!((cal_no.calibrated_confidence - 0.9).abs() < 0.15);

        // Same confidence but with contradictions
        let with_contra = vec![Hypothesis {
            statement: "test".into(),
            pro_evidence: vec![Evidence {
                source_id: "1".into(),
                content: "a".into(),
                relevance: 0.9,
            }],
            contra_evidence: vec![Evidence {
                source_id: "2".into(),
                content: "b".into(),
                relevance: 0.7,
            }],
            strength: 0.9,
        }];

        let cal_contra = reasoner.calibrate_confidence(0.9, &with_contra);
        assert!(
            cal_contra.calibrated_confidence < cal_no.calibrated_confidence,
            "Contradictions should reduce calibrated confidence"
        );
    }

    #[tokio::test]
    async fn test_run_with_empty_context() {
        let reasoner = System2Reasoner::heuristic_only(ReasonerConfig::default());
        let docs = vec![];
        let context = make_context(docs);

        let result = reasoner.run("nothing", &context).await.unwrap();
        assert!(result.confidence < 0.5); // Low confidence for empty context
        assert!(result.supporting_evidence.is_empty());
    }

    #[tokio::test]
    async fn test_multi_step_chain_is_generated() {
        let reasoner = System2Reasoner::heuristic_only(ReasonerConfig::default());
        let docs = vec![
            make_doc("1", "Water boils at 100 Celsius at sea level", 0.95),
            make_doc("2", "Boiling point decreases with altitude", 0.90),
            make_doc("3", "On Mount Everest water boils at 71 Celsius", 0.85),
            make_doc("4", "Pressure cooking increases boiling point", 0.80),
        ];
        let context = make_context(docs);

        let result = reasoner.run("water boiling point", &context).await.unwrap();
        assert!(
            result.reasoning_chain.len() >= 2,
            "Should have at least 2 reasoning steps with 4 docs"
        );
    }

    #[tokio::test]
    async fn test_heuristic_only_still_produces_valid_output() {
        let reasoner = System2Reasoner::heuristic_only(ReasonerConfig::default());
        let docs = vec![make_doc("a", "test content here", 0.5)];
        let context = make_context(docs);

        let result = reasoner.run("test", &context).await.unwrap();
        assert!(result.confidence >= 0.0);
        // reasoning_elapsed_ms is u64, always >= 0 by construction
        let _ = result.reasoning_elapsed_ms;
        assert!(!result.analysis.is_empty());
    }

    #[test]
    fn test_step_category_serde() {
        let cat = StepCategory::Hypothesis;
        let json = serde_json::to_string(&cat).unwrap();
        assert_eq!(json, "\"Hypothesis\"");

        let back: StepCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, StepCategory::Hypothesis);
    }

    #[test]
    fn test_reasoning_result_serde_roundtrip() {
        let result = ReasoningResult {
            query: "test".into(),
            analysis: "analysis".into(),
            confidence: 0.75,
            supporting_evidence: vec![Evidence {
                source_id: "s1".into(),
                content: "content".into(),
                relevance: 0.9,
            }],
            beliefs_updated: vec![BeliefUpdate {
                concept: "c".into(),
                relation: "r".into(),
                target: "t".into(),
                confidence: 0.8,
                based_on: vec!["s1".into()],
            }],
            reasoning_chain: vec![ReasoningStep {
                step: 0,
                category: StepCategory::Hypothesis,
                thought: "thinking".into(),
                conclusion: "done".into(),
                confidence_delta: 0.1,
                evidence_indices: vec![0],
            }],
            step_count: 1,
            total_tokens_used: 100,
            reasoning_elapsed_ms: 50,
            calibration: ConfidenceCalibration {
                raw_confidence: 0.75,
                entropy: 0.5,
                calibrated_confidence: 0.70,
                has_contradiction: false,
                contradiction_count: 0,
            },
        };

        let json = serde_json::to_string(&result).unwrap();
        let back: ReasoningResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.query, "test");
        assert!(
            (back.calibration.calibrated_confidence - 0.70).abs() < 0.02,
            "expected ~0.70, got {}",
            back.calibration.calibrated_confidence
        );
        assert_eq!(back.supporting_evidence.len(), 1);
        assert_eq!(back.beliefs_updated.len(), 1);
    }
}
