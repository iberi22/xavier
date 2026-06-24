use crate::codebase::maturity::config::{FeatureAnchor, SubcomponentAnchor};
use crate::ports::outbound::code_graph::CodeGraphPort;
use anyhow::Result;
use std::sync::Arc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredSubcomponent {
    pub name: String,
    pub weight: u32,
    pub maturity: u8,
    pub static_pass_rate: u8,
    pub test_pass_rate: u8,
    pub gate_check: bool,
    pub memory_usage: u8,
    pub issue_health: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredFeature {
    pub id: String,
    pub name: String,
    pub subcomponents: Vec<ScoredSubcomponent>,
    pub overall: f64,
}

pub struct MaturityScorer {
    adapter: Arc<dyn CodeGraphPort>,
}

const STATIC_WEIGHT: f64 = 0.35;
const TEST_WEIGHT: f64 = 0.35;
const GATE_WEIGHT: f64 = 0.10;
const MEMORY_WEIGHT: f64 = 0.10;
const ISSUE_WEIGHT: f64 = 0.10;

impl MaturityScorer {
    pub fn new(adapter: Arc<dyn CodeGraphPort>) -> Self {
        Self { adapter }
    }

    pub async fn score_feature(
        &self,
        feature: &FeatureAnchor,
        memory_evidence: f64,
        issue_evidence: f64
    ) -> Result<ScoredFeature> {
        let mut scored_subs = Vec::new();

        for sub in &feature.subcomponents {
            let scored_sub = self.score_subcomponent(sub, memory_evidence, issue_evidence).await?;
            scored_subs.push(scored_sub);
        }

        let total_weight: u32 = scored_subs.iter().map(|s| s.weight).sum();
        let weighted_sum: f64 = scored_subs.iter().map(|s| s.maturity as f64 * s.weight as f64).sum();
        let overall = if total_weight == 0 { 0.0 } else { (weighted_sum / total_weight as f64).round() };

        Ok(ScoredFeature {
            id: feature.id.clone(),
            name: feature.name.clone(),
            subcomponents: scored_subs,
            overall,
        })
    }

    async fn score_subcomponent(
        &self,
        sub: &SubcomponentAnchor,
        memory_evidence: f64,
        issue_evidence: f64
    ) -> Result<ScoredSubcomponent> {
        // 1. Static Checks
        let static_found = if sub.static_checks.is_empty() {
            1.0
        } else {
            let mut found = 0;
            for check in &sub.static_checks {
                if self.adapter.find_symbol(&check.symbol).await?.is_some() {
                    found += 1;
                }
            }
            found as f64 / sub.static_checks.len() as f64
        };

        // 2. Feature Gates
        let gate_ok = if let Some(ref gate) = sub.required_feature {
            self.adapter.check_feature_gate(gate).await?
        } else {
            true
        };

        // 3. Tests
        let test_pass_rate = if sub.test_anchors.is_empty() {
            1.0
        } else {
            let mut found = 0;
            for test in &sub.test_anchors {
                if self.adapter.find_symbol(&format!("fn {}", test)).await?.is_some() {
                    found += 1;
                }
            }
            found as f64 / sub.test_anchors.len() as f64
        };

        // Weighted Calculation
        let score = (static_found * STATIC_WEIGHT +
                    test_pass_rate * TEST_WEIGHT +
                    (if gate_ok { 1.0 } else { 0.0 }) * GATE_WEIGHT +
                    memory_evidence * MEMORY_WEIGHT +
                    issue_evidence * ISSUE_WEIGHT) * 100.0;

        Ok(ScoredSubcomponent {
            name: sub.name.clone(),
            weight: sub.weight,
            maturity: score.round() as u8,
            static_pass_rate: (static_found * 100.0) as u8,
            test_pass_rate: (test_pass_rate * 100.0) as u8,
            gate_check: gate_ok,
            memory_usage: (memory_evidence * 100.0) as u8,
            issue_health: (issue_evidence * 100.0) as u8,
        })
    }
}
