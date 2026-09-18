//! Textual Gradient Descent (TGD) memory utility pruning and forgetting module.
//!
//! Provides explicit pruning of low-utility memories based on utility scores,
//! inactive age thresholds, and safety retention floors, while preserving
//! pinned and critical facts.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::memory::decay::get_last_accessed;
use crate::memory::manager::priority::MemoryPriority;
use crate::memory::store::{MemoryRecord, MemoryStore};

/// Configuration options for TGD memory utility pruning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TgdPruneConfig {
    /// Utility score threshold below which a record is eligible for pruning.
    pub utility_threshold: f32,
    /// Minimum age in days before low-utility pruning applies.
    pub min_age_days: f32,
    /// Minimum number of memories that must be retained in the workspace (safety floor).
    pub safety_retention_floor: usize,
}

impl Default for TgdPruneConfig {
    fn default() -> Self {
        Self {
            utility_threshold: 0.3,
            min_age_days: 7.0,
            safety_retention_floor: 5,
        }
    }
}

/// Structured summary report of a TGD pruning operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TgdPruneSummary {
    /// Total number of memory records evaluated.
    pub total_processed: usize,
    /// Number of memory records pruned.
    pub pruned_count: usize,
    /// Number of memory records retained.
    pub retained_count: usize,
    /// Estimated reclaimed storage in bytes.
    pub reclaimed_bytes: u64,
    /// Number of consolidated entities processed.
    pub consolidated_entities: usize,
    /// List of IDs of pruned records.
    pub pruned_ids: Vec<String>,
}

/// Helper function to categorize a path for logging and heuristics.
pub fn categorize_path(path: &str) -> &'static str {
    if path.contains("gestalt/bus/executions") {
        "gestalt_bus_execution"
    } else if path.contains("session/cron_") {
        "session_cron"
    } else if path.starts_with("test/") || path.starts_with("xtsp/") {
        "test_artifact"
    } else if path.starts_with("agent_memory://JSON_Agent/") {
        "json_agent"
    } else if path.starts_with("docs/")
        || path.contains("/docs/")
        || path.starts_with("knowledge/")
        || path.contains("/knowledge/")
        || path.starts_with("kb/")
        || path.contains("/kb/")
        || path.contains("docs")
        || path.contains("knowledge")
    {
        "docs_knowledge"
    } else {
        "general"
    }
}

/// Helper function to retrieve the effective utility score of a MemoryRecord.
pub fn get_utility_score(record: &MemoryRecord) -> f32 {
    if let serde_json::Value::Object(ref map) = record.metadata {
        for key in &[
            "utility_score",
            "score",
            "tgd_refinement_score",
            "memory_importance",
        ] {
            if let Some(val) = map.get(*key).and_then(|v| v.as_f64()) {
                return val as f32;
            }
        }
    }

    if record.score > 0.0 {
        return record.score;
    }

    // Path-based heuristics before falling back to 1.0:
    let path = &record.path;
    if path.contains("gestalt/bus/executions") {
        return 0.10;
    }
    if path.contains("session/cron_") {
        return 0.15;
    }
    if path.starts_with("test/") || path.starts_with("xtsp/") {
        return 0.05;
    }
    if path.starts_with("agent_memory://JSON_Agent/") {
        return 0.20;
    }
    if path.starts_with("docs/")
        || path.contains("/docs/")
        || path.starts_with("knowledge/")
        || path.contains("/knowledge/")
        || path.starts_with("kb/")
        || path.contains("/kb/")
        || path.contains("docs")
        || path.contains("knowledge")
    {
        return 0.80;
    }

    1.0
}

/// Helper function to check whether a MemoryRecord is pinned or critical.
pub fn is_pinned_or_critical(record: &MemoryRecord) -> bool {
    let priority = MemoryPriority::from_metadata(&record.metadata);
    if priority == MemoryPriority::Critical {
        return true;
    }

    if let serde_json::Value::Object(ref map) = record.metadata {
        if map.get("pinned").and_then(|v| v.as_bool()).unwrap_or(false)
            || map
                .get("critical")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            || map
                .get("is_pinned")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            || map
                .get("is_critical")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        {
            return true;
        }

        if let Some(p) = map.get("priority").and_then(|v| v.as_str()) {
            if p.eq_ignore_ascii_case("critical") {
                return true;
            }
        }

        if let Some(p) = map.get("memory_priority").and_then(|v| v.as_str()) {
            if p.eq_ignore_ascii_case("critical") {
                return true;
            }
        }
    }

    false
}

/// Calculates the age of a record in days relative to `now`.
pub fn get_record_age_days(record: &MemoryRecord, now: DateTime<Utc>) -> f32 {
    let last_accessed = get_last_accessed(record);
    let duration = now - last_accessed;
    duration.num_seconds() as f32 / 86400.0
}

/// TGD Utility Pruner for managing autonomous memory forgetting.
pub struct TgdUtilityPruner {
    config: TgdPruneConfig,
}

impl TgdUtilityPruner {
    /// Creates a new TgdUtilityPruner with the specified configuration.
    pub fn new(config: TgdPruneConfig) -> Self {
        Self { config }
    }

    /// Creates a new TgdUtilityPruner with default configuration.
    pub fn with_defaults() -> Self {
        Self {
            config: TgdPruneConfig::default(),
        }
    }

    /// Executes low-utility memory pruning for a given workspace store.
    pub async fn prune_memories(
        &self,
        store: &dyn MemoryStore,
        workspace_id: &str,
    ) -> Result<TgdPruneSummary> {
        let now = Utc::now();
        let records = store.list(workspace_id).await?;
        let total_processed = records.len();

        let mut candidate_indices = Vec::new();

        for (idx, record) in records.iter().enumerate() {
            if is_pinned_or_critical(record) {
                continue;
            }

            let utility = get_utility_score(record);
            let age_days = get_record_age_days(record, now);

            // Respect min_age_days: records younger than min_age_days are strictly preserved
            if utility < self.config.utility_threshold && age_days >= self.config.min_age_days {
                candidate_indices.push((idx, utility, age_days));
            }
        }

        // Sort candidates by lowest utility score first, then oldest age
        candidate_indices.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal))
        });

        // Respect the safety retention floor
        let max_allowed_prunes = total_processed.saturating_sub(self.config.safety_retention_floor);
        let prunes_to_execute = candidate_indices.len().min(max_allowed_prunes);

        let mut summary = TgdPruneSummary {
            total_processed,
            ..Default::default()
        };

        let mut category_counts = std::collections::HashMap::new();

        for (idx, utility, age_days) in candidate_indices.into_iter().take(prunes_to_execute) {
            let record = &records[idx];
            let category = categorize_path(&record.path);
            let rec_bytes = record.content.len() as u64
                + serde_json::to_string(&record.metadata)
                    .map(|s| s.len() as u64)
                    .unwrap_or(0);

            info!(
                "TGD Pruning low-utility memory: ID={}, Path={}, Category={}, Utility={:.4}, AgeDays={:.1}, SizeBytes={}",
                record.id, record.path, category, utility, age_days, rec_bytes
            );

            if let Err(e) = store.delete(workspace_id, &record.id).await {
                warn!("Failed to delete low-utility record {}: {:?}", record.id, e);
            } else {
                summary.pruned_count += 1;
                summary.reclaimed_bytes += rec_bytes;
                summary.pruned_ids.push(record.id.clone());
                *category_counts.entry(category).or_insert(0usize) += 1;
            }
        }

        if !category_counts.is_empty() {
            info!("TGD Pruning summary by category: {:?}", category_counts);
        }

        summary.retained_count = total_processed.saturating_sub(summary.pruned_count);
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::InMemoryMemoryStore;

    #[tokio::test]
    async fn test_tgd_pruning_thresholds_and_safety_floor() {
        let store = InMemoryMemoryStore::new();
        let workspace_id = "test-ws";

        // Create 10 records:
        // 1. Critical fact (pinned via metadata) -> MUST NOT be pruned
        // 2-6. Low utility (< 0.3) and old (> 7 days)
        // 7. High utility (0.8) and old (> 7 days) -> Retained
        // 8. Low utility (0.1) but fresh (1 day) -> Retained
        // 9-10. Normal records

        let now = Utc::now();

        // Record 1: Critical / pinned
        store
            .put(MemoryRecord {
                id: "r1_critical".to_string(),
                workspace_id: workspace_id.to_string(),
                content: "Critical core system instruction".to_string(),
                metadata: serde_json::json!({
                    "pinned": true,
                    "utility_score": 0.05,
                    "last_accessed_at": (now - chrono::Duration::days(30)).to_rfc3339()
                }),
                ..Default::default()
            })
            .await
            .unwrap();

        // Records 2..=6: Low utility (0.1) & old (15 days)
        for i in 2..=6 {
            store
                .put(MemoryRecord {
                    id: format!("r{}", i),
                    workspace_id: workspace_id.to_string(),
                    content: format!("Temporary low quality test log {}", i),
                    metadata: serde_json::json!({
                        "utility_score": 0.1,
                        "last_accessed_at": (now - chrono::Duration::days(15)).to_rfc3339()
                    }),
                    ..Default::default()
                })
                .await
                .unwrap();
        }

        // Record 7: High utility (0.8), old (20 days)
        store
            .put(MemoryRecord {
                id: "r7_high_utility".to_string(),
                workspace_id: workspace_id.to_string(),
                content: "High value memory".to_string(),
                metadata: serde_json::json!({
                    "utility_score": 0.8,
                    "last_accessed_at": (now - chrono::Duration::days(20)).to_rfc3339()
                }),
                ..Default::default()
            })
            .await
            .unwrap();

        // Record 8: Low utility (0.1), fresh (1 day)
        store
            .put(MemoryRecord {
                id: "r8_fresh".to_string(),
                workspace_id: workspace_id.to_string(),
                content: "Fresh low utility memory".to_string(),
                metadata: serde_json::json!({
                    "utility_score": 0.1,
                    "last_accessed_at": (now - chrono::Duration::days(1)).to_rfc3339()
                }),
                ..Default::default()
            })
            .await
            .unwrap();

        // Records 9..=10: Normal
        for i in 9..=10 {
            store
                .put(MemoryRecord {
                    id: format!("r{}", i),
                    workspace_id: workspace_id.to_string(),
                    content: format!("Normal memory content {}", i),
                    metadata: serde_json::json!({
                        "utility_score": 0.5,
                        "last_accessed_at": (now - chrono::Duration::days(3)).to_rfc3339()
                    }),
                    ..Default::default()
                })
                .await
                .unwrap();
        }

        // Config with safety retention floor of 6 records
        let pruner = TgdUtilityPruner::new(TgdPruneConfig {
            utility_threshold: 0.3,
            min_age_days: 7.0,
            safety_retention_floor: 6,
        });

        let summary = pruner.prune_memories(&store, workspace_id).await.unwrap();

        assert_eq!(summary.total_processed, 10);
        // Out of 5 candidates (r2, r3, r4, r5, r6), safety retention floor of 6 allows max 4 prunes (10 - 6 = 4)
        assert_eq!(summary.pruned_count, 4);
        assert_eq!(summary.retained_count, 6);
        assert!(summary.reclaimed_bytes > 0);

        // Verify r1_critical was NOT pruned
        let r1 = store.get(workspace_id, "r1_critical").await.unwrap();
        assert!(r1.is_some(), "Pinned/critical fact must be preserved");

        // Verify r7_high_utility was NOT pruned
        let r7 = store.get(workspace_id, "r7_high_utility").await.unwrap();
        assert!(r7.is_some(), "High utility record must be preserved");

        // Verify r8_fresh was NOT pruned
        let r8 = store.get(workspace_id, "r8_fresh").await.unwrap();
        assert!(r8.is_some(), "Fresh record must be preserved");
    }

    #[test]
    fn test_tgd_path_heuristics_gestalt_bus() {
        let bus_record = MemoryRecord {
            id: "bus_1".to_string(),
            path: "gestalt/bus/executions/task_123.json".to_string(),
            ..Default::default()
        };
        let bus_score = get_utility_score(&bus_record);
        assert!(
            (bus_score - 0.10).abs() < 1e-4,
            "gestalt/bus/executions should receive utility 0.10, got {}",
            bus_score
        );
        assert!(
            bus_score < 0.30,
            "gestalt/bus/executions utility {:.2} must be strictly below 0.30 threshold",
            bus_score
        );

        let cron_record = MemoryRecord {
            id: "cron_1".to_string(),
            path: "session/cron_daily_backup".to_string(),
            ..Default::default()
        };
        assert!((get_utility_score(&cron_record) - 0.15).abs() < 1e-4);

        let test_record = MemoryRecord {
            id: "test_1".to_string(),
            path: "test/temp_run.json".to_string(),
            ..Default::default()
        };
        assert!((get_utility_score(&test_record) - 0.05).abs() < 1e-4);

        let xtsp_record = MemoryRecord {
            id: "xtsp_1".to_string(),
            path: "xtsp/benchmark_fixture.json".to_string(),
            ..Default::default()
        };
        assert!((get_utility_score(&xtsp_record) - 0.05).abs() < 1e-4);

        let json_agent_record = MemoryRecord {
            id: "agent_1".to_string(),
            path: "agent_memory://JSON_Agent/memory.json".to_string(),
            ..Default::default()
        };
        assert!((get_utility_score(&json_agent_record) - 0.20).abs() < 1e-4);

        let docs_record = MemoryRecord {
            id: "docs_1".to_string(),
            path: "docs/manual.md".to_string(),
            ..Default::default()
        };
        assert!((get_utility_score(&docs_record) - 0.80).abs() < 1e-4);

        let general_record = MemoryRecord {
            id: "gen_1".to_string(),
            path: "general/user_notes.md".to_string(),
            ..Default::default()
        };
        assert!((get_utility_score(&general_record) - 1.0).abs() < 1e-4);
    }
}
