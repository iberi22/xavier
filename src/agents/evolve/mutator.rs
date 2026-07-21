// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Mutator Agent - Generates mutations for experiments

use crate::agents::evolve::experiment::Hypothesis;
use crate::agents::evolve::reflector::Insights;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Mutation types supported by the system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Mutation {
    /// Numeric perturbation (e.g., bias, threshold, weights)
    Numeric {
        name: String,
        old_value: f32,
        new_value: f32,
    },
    /// Boolean toggle flip
    Toggle {
        name: String,
        old_value: bool,
        new_value: bool,
    },
    /// Structural change (e.g., add/remove pipeline steps)
    Structural { operation: String, target: String },
}

/// Mutator - Generates new hypotheses by mutating existing configurations or code
pub struct Mutator {}

impl Mutator {
    pub fn new() -> Self {
        Self {}
    }

    /// Generate mutations based on reflector insights
    pub fn generate_mutations(&self, insights: &Insights) -> Result<Vec<Mutation>> {
        let mut mutations = Vec::new();

        // Use suggestions from insights to guide mutations if available
        if !insights.suggestions.is_empty() {
            for suggestion in &insights.suggestions {
                let suggestion_lc = suggestion.to_lowercase();
                if suggestion_lc.contains("threshold") {
                    mutations.push(Mutation::Numeric {
                        name: "similarity_threshold".to_string(),
                        old_value: 0.7,
                        new_value: 0.75,
                    });
                } else if suggestion_lc.contains("cache") {
                    mutations.push(Mutation::Toggle {
                        name: "use_cache".to_string(),
                        old_value: false,
                        new_value: true,
                    });
                } else if suggestion_lc.contains("simplification") {
                    mutations.push(Mutation::Structural {
                        operation: "remove".to_string(),
                        target: "redundant_check_layer".to_string(),
                    });
                }
            }
        }

        // If we still have no mutations and no best metric, generate a default one
        if mutations.is_empty() && insights.best_metric.is_none() {
            mutations.push(Mutation::Numeric {
                name: "retrieval_threshold".to_string(),
                old_value: 0.5,
                new_value: 0.55,
            });
        }

        // If we have a best metric but no mutations yet, try a generic perturbation
        if mutations.is_empty() && insights.best_metric.is_some() {
            mutations.push(Mutation::Numeric {
                name: "alpha".to_string(),
                old_value: 0.5,
                new_value: 0.51,
            });
        }

        info!(mutations_count = mutations.len(), "Generated mutations");
        Ok(mutations)
    }

    /// Transform a mutation into a testable Hypothesis
    pub fn mutation_to_hypothesis(&self, mutation: &Mutation) -> Hypothesis {
        match mutation {
            Mutation::Numeric {
                name,
                old_value,
                new_value,
            } => Hypothesis::hyperparameter(
                format!("Update {} from {} to {}", name, old_value, new_value),
                vec!["src/config.rs".to_string()],
                format!("{}={}", name, new_value),
            ),
            Mutation::Toggle {
                name,
                old_value,
                new_value,
            } => Hypothesis::hyperparameter(
                format!("Flip {} from {} to {}", name, old_value, new_value),
                vec!["src/config.rs".to_string()],
                format!("{}={}", name, new_value),
            ),
            Mutation::Structural { operation, target } => Hypothesis::architecture(
                format!("{} pipeline step: {}", operation, target),
                vec!["src/agents/pipeline.rs".to_string()],
                format!("{} {}", operation, target),
                10,
            ),
        }
    }
}

impl Default for Mutator {
    fn default() -> Self {
        Self::new()
    }
}
