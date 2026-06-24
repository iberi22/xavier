//! MaturityEngine - orchestrator for MCP/fallback maturity analysis.
//!
//! Automatically decides whether to use an external MCP server (codegraph)
//! or the internal fallback based on regex/grep.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::codebase::maturity::scorer;
use crate::ports::code_graph::CodeGraphPort;

/// Result of analyzing a feature's maturity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureMaturityResult {
    pub feature_id: String,
    pub score: f64,
    pub code_score: f64,
    pub memory_score: f64,
    pub conversation_score: f64,
    pub test_score: f64,
    pub doc_score: f64,
    pub evidence: Vec<String>,
    pub gaps: Vec<String>,
}

/// Orchestrator for maturity analysis
pub struct MaturityEngine {
    code_graph: Option<Box<dyn CodeGraphPort + Send + Sync>>,
    anchors: HashMap<String, Vec<String>>,
}

impl MaturityEngine {
    /// Create engine with internal fallback only
    pub fn new_fallback() -> Self {
        Self {
            code_graph: None,
            anchors: HashMap::new(),
        }
    }

    /// Create engine with an external MCP adapter
    pub fn new_with_adapter(adapter: Box<dyn CodeGraphPort + Send + Sync>) -> Self {
        Self {
            code_graph: Some(adapter),
            anchors: HashMap::new(),
        }
    }

    /// Analyze maturity of a feature using the best available source
    pub async fn analyze(
        &self,
        feature_id: &str,
        features_path: &Path,
        codebase_root: &Path,
    ) -> Result<FeatureMaturityResult> {
        if let Some(ref cg) = self.code_graph {
            if let Ok(result) = cg.verify_design(features_path, codebase_root, Some(feature_id)).await {
                return Ok(FeatureMaturityResult {
                    feature_id: feature_id.to_string(),
                    score: result.verified as f64 / result.total_symbols.max(1) as f64 * 100.0,
                    code_score: 0.0,
                    memory_score: 0.0,
                    conversation_score: 0.0,
                    test_score: 0.0,
                    doc_score: 0.0,
                    evidence: result.gaps.clone(),
                    gaps: result.gaps,
                });
            }
        }

        // Fallback: internal scorer
        let evidence = self.gather_evidence(feature_id);
        let score = scorer::score_feature(feature_id, &evidence, 0.5, 0.5);

        Ok(FeatureMaturityResult {
            feature_id: feature_id.to_string(),
            score: score.overall,
            code_score: score.code_coverage,
            memory_score: score.memory_coverage,
            conversation_score: score.conversation_coverage,
            test_score: score.test_coverage,
            doc_score: score.doc_coverage,
            evidence,
            gaps: Vec::new(),
        })
    }

    fn gather_evidence(&self, _feature_id: &str) -> Vec<String> {
        vec![]
    }

    pub fn set_anchors(&mut self, anchors: HashMap<String, Vec<String>>) {
        self.anchors = anchors;
    }
}
