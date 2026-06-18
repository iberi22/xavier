//! E2E test for memory sync between two Xavier nodes.
//!
//! Tests the full memory::sync pipeline: manifest building, diffing,
//! pushing diffs, receiving diffs, and LWW conflict resolution.
//! Uses the known probe paths from build_manifest().

use std::sync::Arc;
use xavier::memory::store::{InMemoryMemoryStore, MemoryRecord, MemoryStore};
use xavier::memory::sync::manifest::build_manifest;
use xavier::memory::sync::diff::diff_manifests;
use xavier::memory::sync::push_pull::entries_as_push_diffs;
use xavier::memory::sync::merge::apply_changes_received;

/// build_manifest probes ["episodic", "semantic", "working", ""]
/// Use these workspace IDs so manifests find the data.
const WS: &str = "episodic"; // one of the known probe paths

fn make_store() -> Arc<InMemoryMemoryStore> {
    Arc::new(InMemoryMemoryStore::new())
}

async fn populate(store: &InMemoryMemoryStore, prefix: &str, count: u64) {
    for i in 0..count {
        store
            .put(MemoryRecord {
                id: format!("{}-{}", prefix, i),
                path: format!("e2e/{}/{}", prefix, i),
                content: format!("Record {} from node {}", i, prefix),
                workspace_id: WS.to_string(),
                metadata: serde_json::json!({"node": prefix, "idx": i}),
                revision: i,
                ..Default::default()
            })
            .await
            .expect(&format!("put {}-{}", prefix, i));
    }
}

#[tokio::test]
async fn test_two_node_sync_cycle() {
    let store_a = make_store();
    let store_b = make_store();

    populate(&store_a, "alpha", 5).await;
    populate(&store_b, "beta", 3).await;

    // Build manifests
    let manifest_a = build_manifest(&*store_a).await.expect("manifest A");
    let manifest_b = build_manifest(&*store_b).await.expect("manifest B");

    assert_eq!(manifest_a.len(), 5, "manifest A should have 5 entries");
    assert_eq!(manifest_b.len(), 3, "manifest B should have 3 entries");

    // Diff: A -> B (push) and B -> A (pull)
    let (to_push, to_pull) =
        diff_manifests(&manifest_a, &manifest_b).expect("diff manifests");

    assert!(!to_push.is_empty(), "A should have entries to push to B");
    assert!(!to_pull.is_empty(), "B should have entries to pull from A");

    // Push: A's entries -> B
    let push_diffs = entries_as_push_diffs(&*store_a, &to_push)
        .await
        .expect("entries as push diffs");

    assert_eq!(push_diffs.len(), to_push.len(), "all push entries should resolve");

    let mut conflicts = 0u64;
    apply_changes_received(&*store_b, &push_diffs, &mut conflicts)
        .await
        .expect("apply to B");

    assert_eq!(conflicts, 0, "no LWW conflicts on first push");

    // Pull: B -> A (B's entries that A doesn't have)
    let pull_diffs = entries_as_push_diffs(&*store_b, &to_pull)
        .await
        .expect("B entries as diffs");

    apply_changes_received(&*store_a, &pull_diffs, &mut conflicts)
        .await
        .expect("apply to A");

    assert_eq!(conflicts, 0, "no LWW conflicts on pull");

    // After bidirectional sync, both stores should converge
    let final_a = build_manifest(&*store_a).await.expect("final manifest A");
    let final_b = build_manifest(&*store_b).await.expect("final manifest B");

    assert_eq!(
        final_a.len(),
        final_b.len(),
        "Both stores should have same manifest entry count after sync: A={} B={}",
        final_a.len(),
        final_b.len()
    );

    // Verify a specific record synced both ways
    let alpha_4 = store_b
        .get(WS, "e2e/alpha/4")
        .await
        .expect("get alpha-4 from B")
        .expect("alpha-4 should exist in B after sync");
    assert_eq!(alpha_4.content, "Record 4 from node alpha");

    let beta_2 = store_a
        .get(WS, "e2e/beta/2")
        .await
        .expect("get beta-2 from A")
        .expect("beta-2 should exist in A after sync");
    assert_eq!(beta_2.content, "Record 2 from node beta");
}

#[tokio::test]
async fn test_lww_conflict_newer_wins() {
    let store_a = make_store();
    let store_b = make_store();

    // Same record on both — A has revision 1, B has revision 100
    store_a
        .put(MemoryRecord {
            id: "conflict-doc-1".to_string(),
            path: "conflicts/shared-1".to_string(),
            content: "Old version from A".to_string(),
            workspace_id: WS.to_string(),
            revision: 1,
            ..Default::default()
        })
        .await
        .unwrap();

    store_b
        .put(MemoryRecord {
            id: "conflict-doc-1".to_string(),
            path: "conflicts/shared-1".to_string(),
            content: "New version from B".to_string(),
            workspace_id: WS.to_string(),
            revision: 100,
            ..Default::default()
        })
        .await
        .unwrap();

    // Sync A -> B
    let manifest_a = build_manifest(&*store_a).await.unwrap();
    let manifest_b = build_manifest(&*store_b).await.unwrap();

    // Since both have the same path+revision combo but different content,
    // they'll have different hashes — so both will appear in each other's
    // "to_push" list.  We need to verify the LWW resolver picks the
    // newer revision (100 over 1) regardless of direction.

    let (to_push_a, _) = diff_manifests(&manifest_a, &manifest_b).unwrap();
    let push_diffs = entries_as_push_diffs(&*store_a, &to_push_a)
        .await
        .unwrap();

    // Only push if there's something A has that B doesn't
    if !push_diffs.is_empty() {
        let mut conflicts = 0u64;
        apply_changes_received(&*store_b, &push_diffs, &mut conflicts)
            .await
            .unwrap();
    }

    // B should still have its own version (revision 100)
    let b_record = store_b
        .get(WS, "conflicts/shared-1")
        .await
        .unwrap()
        .expect("conflict-doc should exist in B");
    assert_eq!(
        b_record.revision, 100,
        "B's revision should remain 100"
    );

    // Now sync B -> A (this is where the newer version should propagate)
    let manifest_a2 = build_manifest(&*store_a).await.unwrap();
    let (_, to_pull_a) = diff_manifests(&manifest_a2, &manifest_b).unwrap();
    let pull_diffs = entries_as_push_diffs(&*store_b, &to_pull_a)
        .await
        .unwrap();

    let mut conflicts = 0u64;
    apply_changes_received(&*store_a, &pull_diffs, &mut conflicts)
        .await
        .unwrap();

    let a_record = store_a
        .get(WS, "conflicts/shared-1")
        .await
        .unwrap()
        .expect("conflict-doc should exist in A after sync");
    assert_eq!(
        a_record.revision, 100,
        "A should be updated to revision 100"
    );
    assert_eq!(
        a_record.content, "New version from B",
        "A should accept B's newer version"
    );
}

#[tokio::test]
async fn test_empty_sync_is_noop() {
    let store_a = make_store();
    let store_b = make_store();

    let manifest_a = build_manifest(&*store_a).await.unwrap();
    let manifest_b = build_manifest(&*store_b).await.unwrap();

    assert!(manifest_a.is_empty(), "empty store -> empty manifest");
    assert!(manifest_b.is_empty(), "empty store -> empty manifest");

    let (to_push, to_pull) = diff_manifests(&manifest_a, &manifest_b).unwrap();
    assert!(to_push.is_empty(), "no push for empty stores");
    assert!(to_pull.is_empty(), "no pull for empty stores");
}

#[tokio::test]
async fn test_identical_stores_no_sync_needed() {
    let store_a = make_store();
    let store_b = make_store();

    populate(&store_a, "same", 3).await;
    populate(&store_b, "same", 3).await;

    let manifest_a = build_manifest(&*store_a).await.unwrap();
    let manifest_b = build_manifest(&*store_b).await.unwrap();

    let (to_push, to_pull) = diff_manifests(&manifest_a, &manifest_b).unwrap();
    assert!(to_push.is_empty(), "identical stores: nothing to push");
    assert!(to_pull.is_empty(), "identical stores: nothing to pull");
}
