use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::memory::qmd_memory::MemoryDocument;
use super::priority::MemoryPriority;

/// Memory Quality Score - composite score for retention decisions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryQuality {
    /// 0-1 based on access frequency and priority
    pub relevance_score: f32,
    /// 0-1 based on belief graph verifications
    pub accuracy_score: f32,
    /// 0-1 based on time since last access
    pub freshness_score: f32,
    /// 0-1 based on metadata completeness
    pub completeness_score: f32,
    /// Weighted composite score
    pub overall: f32,
}

impl MemoryQuality {
    /// Weights for composite score
    const RELEVANCE_WEIGHT: f32 = 0.40;
    const ACCURACY_WEIGHT: f32 = 0.25;
    const FRESHNESS_WEIGHT: f32 = 0.20;
    const COMPLETENESS_WEIGHT: f32 = 0.15;

    pub fn calculate(
        doc: &MemoryDocument,
        priority: MemoryPriority,
        access_count: usize,
        last_access: Option<DateTime<Utc>>,
        verified: bool,
    ) -> Self {
        // Relevance: access frequency + priority boost
        let base_relevance = (access_count as f32 * 0.1).min(1.0);
        let priority_boost = match priority {
            MemoryPriority::Critical => 1.0,
            MemoryPriority::High => 0.8,
            MemoryPriority::Medium => 0.6,
            MemoryPriority::Low => 0.4,
            MemoryPriority::Ephemeral => 0.2,
        };
        let relevance_score = (base_relevance * 0.6 + priority_boost * 0.4).min(1.0);

        // Accuracy: based on verification in belief graph and memory level
        let level_accuracy = match doc.level {
            crate::memory::schema::MemoryLevel::Belief => 1.0,
            crate::memory::schema::MemoryLevel::Extracted => 0.8,
            crate::memory::schema::MemoryLevel::Processed => 0.7,
            crate::memory::schema::MemoryLevel::Raw => 0.5,
        };
        let accuracy_score = if verified { 1.0 } else { level_accuracy };

        // Freshness: based on days since last access
        let freshness_score = if let Some(last) = last_access {
            let days_since = (Utc::now() - last).num_days() as f32;
            let max_days = priority.max_age_days() as f32;
            (1.0 - days_since / max_days).clamp(0.0, 1.0)
        } else {
            // No access record = assume fresh
            0.8
        };

        // Completeness: based on metadata fields
        let completeness_score = {
            let meta = &doc.metadata;
            let mut score = 0.0;
            let mut count = 0;
            for key in ["kind", "namespace", "provenance", "source_path"] {
                if meta.get(key).is_some() {
                    score += 1.0;
                }
                count += 1;
            }
            if count > 0 {
                score / count as f32
            } else {
                0.5
            }
        };

        let overall = Self::RELEVANCE_WEIGHT * relevance_score
            + Self::ACCURACY_WEIGHT * accuracy_score
            + Self::FRESHNESS_WEIGHT * freshness_score
            + Self::COMPLETENESS_WEIGHT * completeness_score;

        Self {
            relevance_score,
            accuracy_score,
            freshness_score,
            completeness_score,
            overall: overall.clamp(0.0, 1.0),
        }
    }
}

/// Memory entry with metadata for management decisions
#[derive(Debug, Clone)]
pub struct ManagedMemory {
    pub doc: MemoryDocument,
    pub priority: MemoryPriority,
    pub quality: MemoryQuality,
    pub access_count: usize,
    pub last_access: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub size_bytes: u64,
}

/// Memory statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub total_documents: usize,
    pub total_size_bytes: u64,
    pub by_priority: HashMap<String, usize>,
    pub by_quality_bucket: HashMap<String, usize>,
    pub low_quality_count: usize,
    pub ephemeral_count: usize,
    pub decayed_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_calculation() {
        let doc = MemoryDocument {
            id: Some("test".to_string()),
            path: "test/path".to_string(),
            content: "Test content".to_string(),
            metadata: serde_json::json!({"kind": "fact"}),
            content_vector: Some(vec![0.0; 384]),
            embedding: vec![0.0; 384],
            ..Default::default()
        };

        let quality = MemoryQuality::calculate(
            &doc,
            MemoryPriority::Medium,
            5,
            Some(chrono::Utc::now()),
            true,
        );

        assert!(quality.overall >= 0.0 && quality.overall <= 1.0);
        assert!(quality.accuracy_score == 1.0); // verified = true
    }
}
