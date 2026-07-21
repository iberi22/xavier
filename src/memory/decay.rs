// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Memory decay and forgetting curve module.
//!
//! Implements Ebbinghaus forgetting curve algorithms and a DecayManager
//! to periodically lower importance scores or prune old, unaccessed memories.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::memory::manager::priority::MemoryPriority;
use crate::memory::store::{MemoryRecord, MemoryStore};

/// Configuration options for the memory decay algorithm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecayConfig {
    /// Threshold score below which a memory record is pruned (deleted).
    /// If None, pruning is disabled.
    pub prune_threshold: Option<f32>,
    /// Minimum days of inactivity (not accessed) before decay starts applying.
    /// Default is 30 days as per acceptance criteria.
    pub inactivity_days: f32,
    /// Default initial score if record.score is 0.0 or not set.
    pub default_initial_score: f32,
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            prune_threshold: Some(0.1),
            inactivity_days: 30.0,
            default_initial_score: 1.0,
        }
    }
}

/// Results of a decay cycle run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DecayReport {
    pub total_processed: usize,
    pub decayed_count: usize,
    pub pruned_count: usize,
    pub updated_records: Vec<String>,
    pub pruned_records: Vec<String>,
}

/// Manager responsible for executing the decay algorithm on a `MemoryStore`.
pub struct DecayManager {
    config: DecayConfig,
}

impl DecayManager {
    /// Creates a new DecayManager with the specified configuration.
    pub fn new(config: DecayConfig) -> Self {
        Self { config }
    }

    /// Creates a new DecayManager with default configuration.
    pub fn with_defaults() -> Self {
        Self {
            config: DecayConfig::default(),
        }
    }

    /// Executes a decay cycle for all memories in a given workspace.
    ///
    /// It fetches all memories from the store, evaluates their inactive duration,
    /// applies Ebbinghaus decay if inactivity > inactivity_days, and updates or prunes
    /// them in the store accordingly.
    pub async fn decay_workspace_memories(
        &self,
        store: &dyn MemoryStore,
        workspace_id: &str,
    ) -> Result<DecayReport> {
        let now = Utc::now();
        let records = store.list(workspace_id).await?;
        let mut report = DecayReport {
            total_processed: records.len(),
            ..Default::default()
        };

        for mut record in records {
            let (new_score, should_prune) = calculate_decay(&record, now, &self.config);

            if should_prune {
                info!(
                    "Pruning memory record: ID={}, Path={}, Score={:.4} -> expired/decayed",
                    record.id, record.path, new_score
                );
                if let Err(e) = store.delete(workspace_id, &record.id).await {
                    warn!("Failed to delete decayed record {}: {:?}", record.id, e);
                } else {
                    report.pruned_count += 1;
                    report.pruned_records.push(record.id.clone());
                }
            } else if (record.score - new_score).abs() > 0.0001 {
                let old_score = record.score;
                record.score = new_score;

                // Also update score in metadata if present
                if let serde_json::Value::Object(ref mut map) = record.metadata {
                    map.insert("score".to_string(), serde_json::json!(new_score));
                }

                info!(
                    "Decaying memory record: ID={}, Path={}, Score={:.4} -> {:.4}",
                    record.id, record.path, old_score, new_score
                );

                if let Err(e) = store.update(record.clone()).await {
                    warn!("Failed to update decayed record {}: {:?}", record.id, e);
                } else {
                    report.decayed_count += 1;
                    report.updated_records.push(record.id.clone());
                }
            }
        }

        Ok(report)
    }
}

/// Helper function to retrieve the last accessed time of a MemoryRecord.
/// Falls back to updated_at or created_at if last_accessed_at is not present in metadata.
pub fn get_last_accessed(record: &MemoryRecord) -> DateTime<Utc> {
    if let Some(last_accessed_val) = record.metadata.get("last_accessed_at").and_then(|v| v.as_str()) {
        if let Ok(parsed) = DateTime::parse_from_rfc3339(last_accessed_val) {
            return parsed.with_timezone(&Utc);
        }
    }
    // Also check for standard lowercase format or fallback to updated_at
    record.updated_at
}

/// Helper function to update/touch the last accessed time of a MemoryRecord.
pub fn touch_memory(record: &mut MemoryRecord, now: DateTime<Utc>) {
    if let serde_json::Value::Object(ref mut map) = record.metadata {
        map.insert("last_accessed_at".to_string(), serde_json::json!(now.to_rfc3339()));
    } else {
        let mut map = serde_json::Map::new();
        map.insert("last_accessed_at".to_string(), serde_json::json!(now.to_rfc3339()));
        record.metadata = serde_json::Value::Object(map);
    }
}

/// Calculates the new score and whether the record should be pruned under Ebbinghaus decay model.
pub fn calculate_decay(
    record: &MemoryRecord,
    now: DateTime<Utc>,
    config: &DecayConfig,
) -> (f32, bool) {
    let last_accessed = get_last_accessed(record);
    let duration = now - last_accessed;
    let days_since = duration.num_seconds() as f32 / 86400.0;

    let priority = MemoryPriority::from_metadata(&record.metadata);

    // Critical memories are retained forever and never decay
    if priority == MemoryPriority::Critical {
        return (record.score, false);
    }

    // Hard age limits per priority are checked first
    let max_age = priority.max_age_days() as f32;
    if days_since > max_age {
        return (0.0, true);
    }

    if days_since > config.inactivity_days {
        // Ebbinghaus Forgetting Curve: R = e^(-t / S)
        // Strength (S) is mapped based on Priority (in days)
        let strength = match priority {
            MemoryPriority::High => 365.0,
            MemoryPriority::Medium => 90.0,
            MemoryPriority::Low => 30.0,
            MemoryPriority::Ephemeral => 7.0,
            _ => 90.0,
        };

        let initial_score = if record.score == 0.0 {
            config.default_initial_score
        } else {
            record.score
        };

        // Ebbinghaus decay factor
        let decay_factor = (-days_since / strength).exp();
        let new_score = initial_score * decay_factor;

        // Check if we should prune based on threshold
        let mut should_prune = false;
        if let Some(threshold) = config.prune_threshold {
            if new_score < threshold {
                should_prune = true;
            }
        }

        (new_score, should_prune)
    } else {
        (record.score, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::InMemoryMemoryStore;

    #[test]
    fn test_ebbinghaus_decay_calculation() {
        let record = MemoryRecord {
            score: 1.0,
            metadata: serde_json::json!({
                "memory_priority": "medium",
                "last_accessed_at": (Utc::now() - chrono::Duration::days(45)).to_rfc3339()
            }),
            ..Default::default()
        };

        let config = DecayConfig {
            prune_threshold: Some(0.1),
            inactivity_days: 30.0,
            default_initial_score: 1.0,
        };

        let (new_score, should_prune) = calculate_decay(&record, Utc::now(), &config);

        // After 45 days of inactivity, which is > 30.0 days, decay should be applied.
        // Medium priority strength is 90 days.
        // e^(-45 / 90) = e^(-0.5) ≈ 0.6065
        assert!(new_score < 1.0);
        assert!((new_score - 0.6065).abs() < 0.01);
        assert!(!should_prune);
    }

    #[test]
    fn test_no_decay_within_inactivity_window() {
        let record = MemoryRecord {
            score: 1.0,
            metadata: serde_json::json!({
                "memory_priority": "medium",
                "last_accessed_at": (Utc::now() - chrono::Duration::days(15)).to_rfc3339()
            }),
            ..Default::default()
        };

        let config = DecayConfig {
            prune_threshold: Some(0.1),
            inactivity_days: 30.0,
            default_initial_score: 1.0,
        };

        let (new_score, should_prune) = calculate_decay(&record, Utc::now(), &config);

        // 15 days is < 30 days inactivity limit, so no decay is applied.
        assert_eq!(new_score, 1.0);
        assert!(!should_prune);
    }

    #[test]
    fn test_critical_never_decays() {
        let record = MemoryRecord {
            score: 1.0,
            metadata: serde_json::json!({
                "memory_priority": "critical",
                "last_accessed_at": (Utc::now() - chrono::Duration::days(500)).to_rfc3339()
            }),
            ..Default::default()
        };

        let config = DecayConfig {
            prune_threshold: Some(0.1),
            inactivity_days: 30.0,
            default_initial_score: 1.0,
        };

        let (new_score, should_prune) = calculate_decay(&record, Utc::now(), &config);

        // Critical priority never decays.
        assert_eq!(new_score, 1.0);
        assert!(!should_prune);
    }

    #[test]
    fn test_pruning_below_threshold() {
        let record = MemoryRecord {
            score: 0.2,
            metadata: serde_json::json!({
                "memory_priority": "low",
                "last_accessed_at": (Utc::now() - chrono::Duration::days(40)).to_rfc3339()
            }),
            ..Default::default()
        };

        let config = DecayConfig {
            prune_threshold: Some(0.15),
            inactivity_days: 30.0,
            default_initial_score: 1.0,
        };

        let (new_score, should_prune) = calculate_decay(&record, Utc::now(), &config);

        // Low priority strength is 30 days.
        // e^(-40 / 30) = e^(-1.333) ≈ 0.2636
        // New score = 0.2 * 0.2636 = 0.0527, which is < prune_threshold of 0.15
        assert!(should_prune);
        assert!(new_score < 0.15);
    }

    #[tokio::test]
    async fn test_decay_manager_integration() {
        let store = InMemoryMemoryStore::new();
        let workspace_id = "test-workspace";

        // Create 3 memories:
        // 1. Critical (no decay)
        // 2. Medium inactive for 40 days (decays)
        // 3. Ephemeral inactive for 10 days (pruned because limit for Ephemeral is 1 day max age)

        let record1 = MemoryRecord {
            id: "m1".to_string(),
            workspace_id: workspace_id.to_string(),
            score: 1.0,
            metadata: serde_json::json!({
                "memory_priority": "critical",
                "last_accessed_at": (Utc::now() - chrono::Duration::days(100)).to_rfc3339()
            }),
            ..Default::default()
        };

        let record2 = MemoryRecord {
            id: "m2".to_string(),
            workspace_id: workspace_id.to_string(),
            score: 0.8,
            metadata: serde_json::json!({
                "memory_priority": "medium",
                "last_accessed_at": (Utc::now() - chrono::Duration::days(45)).to_rfc3339()
            }),
            ..Default::default()
        };

        let record3 = MemoryRecord {
            id: "m3".to_string(),
            workspace_id: workspace_id.to_string(),
            score: 0.5,
            metadata: serde_json::json!({
                "memory_priority": "ephemeral",
                "last_accessed_at": (Utc::now() - chrono::Duration::days(10)).to_rfc3339()
            }),
            ..Default::default()
        };

        store.put(record1).await.unwrap();
        store.put(record2).await.unwrap();
        store.put(record3).await.unwrap();

        let manager = DecayManager::new(DecayConfig {
            prune_threshold: Some(0.1),
            inactivity_days: 30.0,
            default_initial_score: 1.0,
        });

        let report = manager.decay_workspace_memories(&store, workspace_id).await.unwrap();

        assert_eq!(report.total_processed, 3);
        assert_eq!(report.decayed_count, 1); // Only record2 decayed
        assert_eq!(report.pruned_count, 1); // Record3 pruned due to max age for Ephemeral

        // Let's verify final states in store
        let m1_after = store.get(workspace_id, "m1").await.unwrap().unwrap();
        assert_eq!(m1_after.score, 1.0);

        let m2_after = store.get(workspace_id, "m2").await.unwrap().unwrap();
        assert!(m2_after.score < 0.8);

        let m3_after = store.get(workspace_id, "m3").await.unwrap();
        assert!(m3_after.is_none());
    }
}
