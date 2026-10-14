//! Agent memory session deduplication.
//!
//! Provides pure, testable deduplication logic for `agent_memory://` paths
//! with coherent forgetting semantics (soft-deletion via `deleted_at`).

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::info;

use crate::memory::store::{MemoryRecord, MemoryStore};

/// Result of an agent session memory deduplication operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DedupeAgentSessionsResult {
    /// Number of distinct `agent_memory://` paths found.
    pub routes: usize,
    /// Number of duplicate older records marked as forgotten (deleted_at set).
    pub to_forget: usize,
    /// Number of most recent records preserved as active (one per route).
    pub to_keep: usize,
}

/// Checks whether a record has been forgotten (soft-deleted).
pub fn is_record_forgotten(record: &MemoryRecord) -> bool {
    record.deleted_at.is_some()
        || record
            .metadata
            .get("deleted_at")
            .and_then(|v| v.as_str())
            .is_some()
}

/// Pure and testable deduplication function for `agent_memory://` routes.
///
/// For each distinct `agent_memory://` route:
/// - Preserves ONLY the row with the most recent `updated_at` (tie-break: greatest `id`).
/// - Marks all other active rows as forgotten (`deleted_at = now`).
/// - Does NOT perform physical row deletion.
///
/// When `dry_run` is true, computes and returns the counts without mutating the store.
/// When `dry_run` is false, applies the soft-deletion updates to the store.
pub async fn dedupe_agent_sessions(
    store: &dyn MemoryStore,
    workspace_id: &str,
    dry_run: bool,
) -> Result<DedupeAgentSessionsResult> {
    let all_records = store.list(workspace_id).await?;

    let mut routes_map: BTreeMap<String, Vec<MemoryRecord>> = BTreeMap::new();
    for record in all_records {
        if record.path.starts_with("agent_memory://") {
            routes_map
                .entry(record.path.clone())
                .or_default()
                .push(record);
        }
    }

    let routes = routes_map.len();
    let mut to_keep = 0;
    let mut to_forget = 0;
    let mut records_to_mark: Vec<MemoryRecord> = Vec::new();

    let now = Utc::now();

    for (_path, records) in routes_map {
        let mut active: Vec<MemoryRecord> = records
            .into_iter()
            .filter(|r| !is_record_forgotten(r))
            .collect();

        if active.is_empty() {
            // All records for this path were already forgotten
            continue;
        }

        // Sort active records: newest updated_at first, tie-break greatest id first
        active.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| b.id.cmp(&a.id))
        });

        // The most recent record is kept as active (vigente)
        to_keep += 1;

        // All older active records are marked as forgotten
        for mut record in active.into_iter().skip(1) {
            to_forget += 1;
            if !dry_run {
                record.deleted_at = Some(now);
                if let Some(obj) = record.metadata.as_object_mut() {
                    obj.insert(
                        "deleted_at".to_string(),
                        serde_json::json!(now.to_rfc3339()),
                    );
                } else {
                    record.metadata = serde_json::json!({
                        "deleted_at": now.to_rfc3339()
                    });
                }
                records_to_mark.push(record);
            }
        }
    }

    if !dry_run {
        for record in records_to_mark {
            store.update(record).await?;
        }
    }

    info!(
        workspace_id = %workspace_id,
        routes = routes,
        to_keep = to_keep,
        to_forget = to_forget,
        dry_run = dry_run,
        "Completed agent session deduplication"
    );

    Ok(DedupeAgentSessionsResult {
        routes,
        to_forget,
        to_keep,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::InMemoryMemoryStore;
    use chrono::{Duration, TimeZone};
    use std::sync::Arc;

    fn make_test_record(id: &str, path: &str, updated_at: chrono::DateTime<Utc>) -> MemoryRecord {
        MemoryRecord {
            id: id.to_string(),
            workspace_id: "test-ws".to_string(),
            path: path.to_string(),
            content: format!("Content for {id}"),
            metadata: serde_json::json!({ "source": "test" }),
            created_at: updated_at,
            updated_at,
            deleted_at: None,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_dedupe_agent_sessions_dry_run_and_apply() {
        let store = Arc::new(InMemoryMemoryStore::new());
        let t1 = Utc.with_ymd_and_hms(2026, 10, 1, 10, 0, 0).unwrap();
        let t2 = t1 + Duration::hours(1);
        let t3 = t1 + Duration::hours(2);

        // 3 rows for session-1, 1 row for session-2
        store
            .put(make_test_record(
                "s1-old",
                "agent_memory://cursor/session-1",
                t1,
            ))
            .await
            .unwrap();
        store
            .put(make_test_record(
                "s1-mid",
                "agent_memory://cursor/session-1",
                t2,
            ))
            .await
            .unwrap();
        store
            .put(make_test_record(
                "s1-new",
                "agent_memory://cursor/session-1",
                t3,
            ))
            .await
            .unwrap();
        store
            .put(make_test_record(
                "s2-only",
                "agent_memory://cursor/session-2",
                t2,
            ))
            .await
            .unwrap();

        // 1. Dry run
        let dry = dedupe_agent_sessions(&*store, "test-ws", true)
            .await
            .unwrap();
        assert_eq!(
            dry,
            DedupeAgentSessionsResult {
                routes: 2,
                to_forget: 2,
                to_keep: 2,
            }
        );

        // Verify nothing mutated in store during dry run
        let list_after_dry = store.list("test-ws").await.unwrap();
        assert_eq!(list_after_dry.len(), 4);
        for r in &list_after_dry {
            assert!(r.deleted_at.is_none(), "Dry run must not set deleted_at");
        }

        // 2. Apply
        let applied = dedupe_agent_sessions(&*store, "test-ws", false)
            .await
            .unwrap();
        assert_eq!(
            applied,
            DedupeAgentSessionsResult {
                routes: 2,
                to_forget: 2,
                to_keep: 2,
            }
        );

        // Verify store state: exactly 1 active per route
        let list_after_apply = store.list("test-ws").await.unwrap();
        assert_eq!(list_after_apply.len(), 4, "No physical deletions allowed");

        let s1_new = list_after_apply.iter().find(|r| r.id == "s1-new").unwrap();
        assert!(s1_new.deleted_at.is_none(), "Most recent must be kept");

        let s1_mid = list_after_apply.iter().find(|r| r.id == "s1-mid").unwrap();
        assert!(s1_mid.deleted_at.is_some(), "Older row must be forgotten");

        let s1_old = list_after_apply.iter().find(|r| r.id == "s1-old").unwrap();
        assert!(s1_old.deleted_at.is_some(), "Oldest row must be forgotten");

        let s2_only = list_after_apply.iter().find(|r| r.id == "s2-only").unwrap();
        assert!(s2_only.deleted_at.is_none(), "Single row must be kept");

        // 3. Re-run dedupe on clean store: idempotent, 0 to forget
        let rerun = dedupe_agent_sessions(&*store, "test-ws", true)
            .await
            .unwrap();
        assert_eq!(
            rerun,
            DedupeAgentSessionsResult {
                routes: 2,
                to_forget: 0,
                to_keep: 2,
            }
        );
    }

    #[tokio::test]
    async fn test_dedupe_agent_sessions_tiebreak_greatest_id() {
        let store = Arc::new(InMemoryMemoryStore::new());
        let t = Utc.with_ymd_and_hms(2026, 10, 1, 10, 0, 0).unwrap();

        // Two rows with identical timestamp: "alpha" vs "beta"
        store
            .put(make_test_record("alpha", "agent_memory://cursor/tie", t))
            .await
            .unwrap();
        store
            .put(make_test_record("beta", "agent_memory://cursor/tie", t))
            .await
            .unwrap();

        let applied = dedupe_agent_sessions(&*store, "test-ws", false)
            .await
            .unwrap();
        assert_eq!(applied.to_keep, 1);
        assert_eq!(applied.to_forget, 1);

        let list = store.list("test-ws").await.unwrap();
        let beta = list.iter().find(|r| r.id == "beta").unwrap();
        let alpha = list.iter().find(|r| r.id == "alpha").unwrap();

        assert!(beta.deleted_at.is_none(), "Greater ID 'beta' must be kept");
        assert!(
            alpha.deleted_at.is_some(),
            "Lesser ID 'alpha' must be forgotten"
        );
    }
}
