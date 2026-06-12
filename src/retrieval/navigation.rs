//! Intelligent Navigation Policy for Memory Retrieval
//!
//! Implements a "HORMER-style" learned navigation policy that weights multiple
//! signals beyond simple cosine similarity (vector match).

use serde::{Deserialize, Serialize};
use crate::search::rrf::ScoredResult;
use crate::memory::qmd_memory::MemoryDocument;
use chrono::{DateTime, Utc};

/// Signals used for scoring a memory during navigation/retrieval
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum NavigationSignal {
    /// Cosine similarity from vector embedding (0.0 to 1.0)
    VectorSimilarity(f32),
    /// Keyword match score (BM25/Lexical, normalized)
    LexicalMatch(f32),
    /// Recency boost factor (1.0 = fresh, decays to 0.0)
    Recency(f32),
    /// Explicit importance/priority score (0.0 to 1.0)
    Importance(f32),
    /// Access frequency/popularity boost
    AccessFrequency(f32),
}

/// Weights for different navigation signals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalWeights {
    pub vector: f32,
    pub lexical: f32,
    pub recency: f32,
    pub importance: f32,
    pub frequency: f32,
}

impl Default for SignalWeights {
    fn default() -> Self {
        Self {
            vector: 0.4,
            lexical: 0.2,
            recency: 0.2,
            importance: 0.1,
            frequency: 0.1,
        }
    }
}

/// A policy that calculates a unified score from multiple navigation signals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationPolicy {
    pub weights: SignalWeights,
}

impl Default for NavigationPolicy {
    fn default() -> Self {
        Self {
            weights: SignalWeights::default(),
        }
    }
}

impl NavigationPolicy {
    pub fn new(weights: SignalWeights) -> Self {
        Self { weights }
    }

    /// Calculate a final unified score from provided signals
    pub fn score(&self, signals: &[NavigationSignal]) -> f32 {
        let mut total_score = 0.0;

        for signal in signals {
            match signal {
                NavigationSignal::VectorSimilarity(s) => total_score += s * self.weights.vector,
                NavigationSignal::LexicalMatch(s) => total_score += s * self.weights.lexical,
                NavigationSignal::Recency(s) => total_score += s * self.weights.recency,
                NavigationSignal::Importance(s) => total_score += s * self.weights.importance,
                NavigationSignal::AccessFrequency(s) => total_score += s * self.weights.frequency,
            }
        }

        total_score
    }

    /// Extract signals from a MemoryDocument and a raw score (usually vector or lexical)
    pub fn extract_signals(&self, doc: &MemoryDocument, base_score: f32, is_vector: bool) -> Vec<NavigationSignal> {
        let mut signals = Vec::with_capacity(5);

        if is_vector {
            signals.push(NavigationSignal::VectorSimilarity(base_score));
        } else {
            signals.push(NavigationSignal::LexicalMatch(base_score));
        }

        // 1. Recency Signal (smooth exponential decay, clamped to [0, 1])
        let updated_at = doc.metadata.get("updated_at")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or(Utc::now());

        let now = Utc::now();
        let age_hours = if updated_at > now {
            0.0_f32 // Future timestamps clamped to zero age
        } else {
            (now - updated_at).num_hours() as f32
        };
        // Smooth sigmoid-like decay: recency = 1 / (1 + age_hours / half_life)
        // This avoids the step function of exp(-x) which drops too fast at low values
        let recency = 1.0 / (1.0 + age_hours / 168.0);
        signals.push(NavigationSignal::Recency(recency));

        // 2. Importance Signal
        let importance = doc.metadata.get("importance")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5) as f32;
        signals.push(NavigationSignal::Importance(importance.clamp(0.0, 1.0)));

        // 3. Access Frequency (if available in metadata)
        let access_count = doc.metadata.get("access_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as f32;
        let frequency = (access_count / 10.0).min(1.0); // Simple normalization
        signals.push(NavigationSignal::AccessFrequency(frequency));

        signals
    }

    /// Apply the policy to a list of ScoredResult, refining their scores
    pub fn apply(&self, results: &mut [ScoredResult], docs_map: &std::collections::HashMap<String, &MemoryDocument>) {
        for res in results.iter_mut() {
            if let Some(doc) = docs_map.get(&res.id) {
                let signals = self.extract_signals(doc, res.score, res.source == "vector");
                res.score = self.score(&signals);
            }
        }

        // Re-sort results after re-scoring
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn test_navigation_scoring() {
        let policy = NavigationPolicy::default();
        let signals = vec![
            NavigationSignal::VectorSimilarity(0.8),
            NavigationSignal::LexicalMatch(0.5),
            NavigationSignal::Recency(1.0),
            NavigationSignal::Importance(0.7),
            NavigationSignal::AccessFrequency(0.2),
        ];

        // Calculation:
        // vector: 0.8 * 0.4 = 0.32
        // lexical: 0.5 * 0.2 = 0.10
        // recency: 1.0 * 0.2 = 0.20
        // importance: 0.7 * 0.1 = 0.07
        // frequency: 0.2 * 0.1 = 0.02
        // Total: 0.71
        let score = policy.score(&signals);
        assert!((score - 0.71).abs() < 0.001);
    }

    #[test]
    fn test_apply_policy_to_results() {
        let policy = NavigationPolicy::default();
        let mut results = vec![
            ScoredResult {
                id: "doc1".to_string(),
                content: "content 1".to_string(),
                score: 0.9, // Higher initial score
                source: "vector".to_string(),
                path: "path1".to_string(),
                updated_at: None,
            },
            ScoredResult {
                id: "doc2".to_string(),
                content: "content 2".to_string(),
                score: 0.5,
                source: "vector".to_string(),
                path: "path2".to_string(),
                updated_at: None,
            },
        ];

        let now = Utc::now().to_rfc3339();
        let doc1 = MemoryDocument {
            id: Some("doc1".to_string()),
            content: "content 1".to_string(),
            metadata: json!({
                "importance": 0.1, // Very low importance
                "updated_at": now,
            }),
            ..Default::default()
        };
        let doc2 = MemoryDocument {
            id: Some("doc2".to_string()),
            content: "content 2".to_string(),
            metadata: json!({
                "importance": 0.9, // High importance
                "updated_at": now,
            }),
            ..Default::default()
        };

        let mut docs_map = HashMap::new();
        docs_map.insert("doc1".to_string(), &doc1);
        docs_map.insert("doc2".to_string(), &doc2);

        policy.apply(&mut results, &docs_map);

        // doc1 score: 0.9*0.4 + 1.0*0.2 + 0.1*0.1 + 0.0*0.1 = 0.36 + 0.2 + 0.01 = 0.57
        // doc2 score: 0.5*0.4 + 1.0*0.2 + 0.9*0.1 + 0.0*0.1 = 0.20 + 0.2 + 0.09 = 0.49

        assert_eq!(results[0].id, "doc1");
        assert!(results[0].score > results[1].score);
    }
}
