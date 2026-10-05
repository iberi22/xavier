//! Textual Gradient Descent (TGD) memory utility pruning and forgetting module.
//!
//! Provides explicit pruning of low-utility memories based on utility scores,
//! inactive age thresholds, and safety retention floors, while preserving
//! pinned and critical facts.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::memory::access::{self, PruneBlockReason, RECORDER};
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
    /// Why the prune gate withheld permission, when it did.
    ///
    /// `None` means the gate cleared the run (window elapsed and evidence
    /// present), whether or not anything was then pruned. `Some(..)` is a
    /// machine-readable reason a maintenance report can show, so a `0 pruned`
    /// line is never silently indistinguishable from "there was nothing to
    /// prune".
    #[serde(default)]
    pub gate_block_reason: Option<String>,
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
    ///
    /// ## The prune gate
    ///
    /// Deletion is irreversible and the evidence for it is weak by construction:
    /// a utility score is a heuristic and "no access recorded" is
    /// indistinguishable from "nobody has asked yet". So the run is gated by
    /// [`crate::memory::access`] before any candidate is deleted:
    ///
    /// 1. **Observation clock** ([`access::prune_block_reason`]) — read *once*
    ///    per workspace, not once per record. Without a
    ///    `observation_started_at` there is no proof the access signal existed
    ///    when the oldest candidate was written, so nothing is eligible. This
    ///    is the fail-safe direction: an unreadable clock can only ever
    ///    *preserve*, never delete.
    /// 2. **Observability** ([`access::record_is_observable`]) — a record
    ///    without an embedding can never enter a vector top-k, so its zero
    ///    access count carries no information about utility. It is not a
    ///    candidate; pruning it would delete exactly the memories that lost
    ///    their embedding rather than the ones that stopped being useful.
    /// 3. **Evidence of use** (durable `memory_access` + the live buffer) — a
    ///    record that has actually been read is demonstrably needed and is
    ///    shielded, whatever its heuristic utility score says.
    /// 4. **Workspace-wide fail-safe** — if nothing has ever been observed for
    ///    this workspace, nothing is deleted at all.
    ///
    /// Every one of these rules can only ever *keep* a memory. There is no
    /// path through this function that deletes on absent evidence: the delete
    /// loop runs only after the clock is proven elapsed and for candidates
    /// that were both observable and un-read.
    ///
    /// The durable table is read directly rather than through the hydrated
    /// `RECORDER` view, so a restart with an empty in-memory view cannot make a
    /// used record look unused. `RECORDER` is consulted in addition, so an
    /// access recorded seconds ago and not yet flushed also shields.
    ///
    /// When the gate blocks, the run is *not* an error: it returns an empty
    /// summary carrying [`TgdPruneSummary::gate_block_reason`]. GC treats a
    /// blocked run exactly like a run with nothing to prune.
    pub async fn prune_memories(
        &self,
        store: &dyn MemoryStore,
        workspace_id: &str,
    ) -> Result<TgdPruneSummary> {
        let now = Utc::now();
        let records = store.list(workspace_id).await?;
        let total_processed = records.len();

        let mut summary = TgdPruneSummary {
            total_processed,
            ..Default::default()
        };

        // ---- Gate, step 1: the observation clock (one read per workspace) ----
        let observation_started_at = match access::observation_started_at(store).await {
            Ok(started) => started,
            Err(error) => {
                // A clock we cannot read is not an elapsed clock. Preserve.
                warn!(
                    workspace = workspace_id,
                    %error,
                    "TGD prune blocked: observation clock unreadable; refusing to delete"
                );
                summary.gate_block_reason = Some(format!("observation_clock_unreadable: {error}"));
                return Ok(summary);
            }
        };
        let gate = access::prune_block_reason(observation_started_at, now);
        if gate != PruneBlockReason::Eligible {
            info!(
                workspace = workspace_id,
                reason = ?gate,
                total_processed,
                "TGD prune blocked by observation gate; nothing deleted"
            );
            summary.gate_block_reason = Some(format!("{gate:?}"));
            return Ok(summary);
        }

        // ---- Gate, step 2+3: observability and evidence, evaluated in memory ----
        //
        // Both are pure predicates over data already in `records` (plus one
        // bulk access-stats load), so neither costs a query per record. The
        // N+1 this would otherwise introduce is avoided by loading the whole
        // workspace's access table in a single `load_access_stats` call.
        let durable_stats = match store.load_access_stats(workspace_id).await {
            Ok(stats) => stats
                .into_iter()
                .collect::<std::collections::HashMap<_, _>>(),
            Err(error) => {
                warn!(
                    workspace = workspace_id,
                    %error,
                    "TGD prune blocked: access evidence unreadable; refusing to delete"
                );
                summary.gate_block_reason = Some(format!("access_evidence_unreadable: {error}"));
                return Ok(summary);
            }
        };
        let evidence_rows = durable_stats.len();

        let mut candidate_indices = Vec::new();
        let mut blocked_not_observable = 0usize;
        let mut blocked_in_use = 0usize;

        for (idx, record) in records.iter().enumerate() {
            if is_pinned_or_critical(record) {
                continue;
            }

            let utility = get_utility_score(record);
            let age_days = get_record_age_days(record, now);

            // Respect min_age_days: records younger than min_age_days are strictly preserved
            if !(utility < self.config.utility_threshold && age_days >= self.config.min_age_days) {
                continue;
            }

            // Gate 2: never observable == never a candidate. A record that can
            // never enter a top-k has no access signal to interpret, so its
            // zero count says nothing about utility. Checked after the cheap
            // utility/age filters so unrelated records cost nothing extra.
            if !access::record_is_observable(record) {
                blocked_not_observable += 1;
                continue;
            }

            // Gate 3: evidence of *use* protects. A record that was actually
            // read is demonstrably needed, whatever its heuristic score says.
            //
            // This reads the durable `memory_access` table directly rather than
            // the hydrated `RECORDER` view, so it is immune to the hydration
            // trap: after a restart the in-memory view may be empty, but a
            // durable row still shields its record. `RECORDER` is consulted
            // additionally so an access recorded seconds ago and not yet
            // flushed also shields.
            let observed_uses = durable_stats
                .get(&record.id)
                .is_some_and(|stats| stats.access_count > 0)
                || RECORDER.access_count(workspace_id, &record.id) > 0;
            if observed_uses {
                blocked_in_use += 1;
                continue;
            }

            candidate_indices.push((idx, utility, age_days));
        }

        if blocked_not_observable > 0 || blocked_in_use > 0 {
            info!(
                workspace = workspace_id,
                blocked_not_observable,
                blocked_in_use,
                evidence_rows,
                "TGD prune gate suppressed candidates"
            );
        }

        // Gate 4: workspace-wide fail-safe.
        //
        // If the durable table holds no rows for this workspace and the live
        // buffer is empty, then *nothing has ever been observed here* — the
        // instrumentation is not running for this workspace at all (unmigrated
        // backend, reads bypassing the HTTP/MCP handlers, or a workspace that
        // has simply never been read). Every candidate would then look "never
        // used", which is exactly the condition the gate exists to refuse:
        // deleting precisely what was never observed. So: no prune.
        if evidence_rows == 0 && RECORDER.pending_count() == 0 {
            warn!(
                workspace = workspace_id,
                total_processed,
                "TGD prune blocked: no access evidence has ever been recorded for this workspace; \
                 deleting unobserved records is not permitted"
            );
            summary.gate_block_reason = Some("no_access_evidence_for_workspace".to_string());
            return Ok(summary);
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

    /// The 10-record threshold matrix, now run *through the gate*.
    ///
    /// `InMemoryMemoryStore` has no `observation_started_at` (the trait default
    /// returns `None`) and no access rows, so this store is exactly the
    /// "nothing was ever observed here" case: the gate must refuse the whole
    /// run. Before the gate was wired in, this same store pruned 4 records —
    /// the behaviour the gate exists to prevent.
    ///
    /// The full matrix with an elapsed clock and real access evidence lives in
    /// `tests/tgd_prune_gate_test.rs`, where the observation window can
    /// actually be reached.
    #[tokio::test]
    async fn test_tgd_pruning_is_blocked_without_observation_evidence() {
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

        assert_eq!(
            summary.total_processed, 10,
            "records are still evaluated; the gate is not a no-op"
        );
        assert_eq!(
            summary.pruned_count, 0,
            "no observation clock and no access evidence means nothing may be deleted"
        );
        assert_eq!(
            summary.gate_block_reason.as_deref(),
            Some("ObservationClockMissing"),
            "the gate must report that the clock, not the utility score, stopped it"
        );
        assert_eq!(summary.pruned_ids.len(), 0);
        assert_eq!(summary.reclaimed_bytes, 0);

        // Every record from the original matrix survives.
        for id in [
            "r1_critical",
            "r2",
            "r3",
            "r4",
            "r5",
            "r6",
            "r7_high_utility",
            "r8_fresh",
            "r9",
            "r10",
        ] {
            assert!(
                store.get(workspace_id, id).await.unwrap().is_some(),
                "{id} must be preserved when nothing has ever been observed"
            );
        }
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
