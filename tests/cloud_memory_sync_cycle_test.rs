//! End-to-end integration test suite for cloud memory sync verification.
//!
//! Verifies complete memory sync cycle:
//! local write -> sync manifest -> cloud push -> remote pull -> verification.
//! Ensures 100% data fidelity and manifest hash convergence across nodes.

mod common;

use common::{compute_store_manifest_hash, create_cloud_sync, make_record};
use std::sync::Arc;
use chrono::{TimeDelta, Utc};
use xavier::memory::store::{InMemoryMemoryStore, MemoryStore};

const WORKSPACE: &str = "episodic";

#[tokio::test]
async fn test_full_cloud_memory_sync_cycle_and_manifest_hash_convergence() {
    let local_store = Arc::new(InMemoryMemoryStore::new());
    let cloud_store = Arc::new(InMemoryMemoryStore::new());

    let (sync, _tmp_dir) = create_cloud_sync(cloud_store.clone(), "local-node-1", None).await;

    // 1. Local Write: Write 5 records to local store
    let now = Utc::now();
    for i in 0..5 {
        let rec = make_record(
            &format!("local-rec-{}", i),
            WORKSPACE,
            &format!("memories/local-rec-{}", i),
            &format!("Local memory content {}", i),
            now + TimeDelta::seconds(i as i64),
            1,
            "local-node-1",
            None,
        );
        local_store.put(rec).await.expect("local store put");
    }

    // 2. Cloud Write: Seed 3 records directly in cloud store
    for i in 0..3 {
        let rec = make_record(
            &format!("cloud-rec-{}", i),
            WORKSPACE,
            &format!("memories/cloud-rec-{}", i),
            &format!("Cloud memory content {}", i),
            now + TimeDelta::seconds(i as i64 + 10),
            1,
            "cloud-node-1",
            None,
        );
        cloud_store.put(rec).await.expect("cloud store put");
    }

    // Verify initial manifests diverge
    let local_hash_before = compute_store_manifest_hash(&*local_store)
        .await
        .expect("local hash before");
    let cloud_hash_before = compute_store_manifest_hash(&*cloud_store)
        .await
        .expect("cloud hash before");
    assert_ne!(
        local_hash_before, cloud_hash_before,
        "Manifest hashes should diverge prior to sync cycle"
    );

    // 3. Complete Memory Sync Cycle: Bidirectional sync_all
    let report = sync
        .sync_all(&*local_store, WORKSPACE)
        .await
        .expect("sync_all should succeed");
    assert!(report.success, "sync_all report should indicate success");
    assert_eq!(report.pulled, 3, "should pull 3 cloud records to local");
    assert!(report.pushed >= 5, "should push local records to cloud");

    // 4. Verification: Verify data fidelity across both stores
    let local_records = local_store.list(WORKSPACE).await.expect("list local");
    let cloud_records = cloud_store.list(WORKSPACE).await.expect("list cloud");

    assert_eq!(local_records.len(), 8, "Local store should contain 8 total records");
    assert_eq!(cloud_records.len(), 8, "Cloud store should contain 8 total records");

    for rec in &local_records {
        let cloud_rec = cloud_store
            .get(WORKSPACE, &rec.id)
            .await
            .expect("get cloud record")
            .expect("record must exist in cloud");
        assert_eq!(rec.content, cloud_rec.content, "Content mismatch for record {}", rec.id);
        assert_eq!(rec.revision, cloud_rec.revision, "Revision mismatch for record {}", rec.id);
        assert_eq!(rec.path, cloud_rec.path, "Path mismatch for record {}", rec.id);
    }

    // 5. Manifest Convergence Verification
    let local_hash_after = compute_store_manifest_hash(&*local_store)
        .await
        .expect("local hash after");
    let cloud_hash_after = compute_store_manifest_hash(&*cloud_store)
        .await
        .expect("cloud hash after");

    assert_eq!(
        local_hash_after, cloud_hash_after,
        "Both nodes MUST converge to identical manifest hashes after complete sync cycle"
    );
}

#[tokio::test]
async fn test_cloud_memory_sync_lww_conflict_resolution() {
    let local_store = Arc::new(InMemoryMemoryStore::new());
    let cloud_store = Arc::new(InMemoryMemoryStore::new());

    let (sync, _tmp_dir) = create_cloud_sync(cloud_store.clone(), "local-node-lww", None).await;

    let now = Utc::now();

    // Conflict 1: Cloud has newer timestamp -> Cloud wins
    let rec1_local = make_record(
        "rec-1",
        WORKSPACE,
        "memories/rec-1",
        "Old local content",
        now - TimeDelta::seconds(20),
        1,
        "node-a",
        None,
    );
    let rec1_cloud = make_record(
        "rec-1",
        WORKSPACE,
        "memories/rec-1",
        "Newer cloud content",
        now,
        2,
        "node-b",
        None,
    );

    // Conflict 2: Local has newer timestamp -> Local wins
    let rec2_local = make_record(
        "rec-2",
        WORKSPACE,
        "memories/rec-2",
        "Newer local content",
        now,
        2,
        "node-a",
        None,
    );
    let rec2_cloud = make_record(
        "rec-2",
        WORKSPACE,
        "memories/rec-2",
        "Old cloud content",
        now - TimeDelta::seconds(20),
        1,
        "node-b",
        None,
    );

    // Conflict 3: Same timestamp -> Node ID tiebreak ("node-b" > "node-a" -> Cloud wins)
    let rec3_local = make_record(
        "rec-3",
        WORKSPACE,
        "memories/rec-3",
        "Tiebreak local content",
        now,
        1,
        "node-a",
        None,
    );
    let rec3_cloud = make_record(
        "rec-3",
        WORKSPACE,
        "memories/rec-3",
        "Tiebreak cloud content",
        now,
        1,
        "node-b",
        None,
    );

    local_store.put(rec1_local).await.unwrap();
    local_store.put(rec2_local).await.unwrap();
    local_store.put(rec3_local).await.unwrap();

    cloud_store.put(rec1_cloud).await.unwrap();
    cloud_store.put(rec2_cloud).await.unwrap();
    cloud_store.put(rec3_cloud).await.unwrap();

    // Perform bidirectional sync cycle
    let report = sync
        .sync_all(&*local_store, WORKSPACE)
        .await
        .expect("sync_all for conflicts");

    assert!(report.conflicts >= 1, "Should record conflict resolution");

    // Verify rec-1 resolved to Cloud version
    let r1 = local_store.get(WORKSPACE, "rec-1").await.unwrap().unwrap();
    assert_eq!(r1.content, "Newer cloud content", "Newer cloud timestamp must win for rec-1");

    // Verify rec-2 resolved to Local version
    let r2_cloud = cloud_store.get(WORKSPACE, "rec-2").await.unwrap().unwrap();
    assert_eq!(r2_cloud.content, "Newer local content", "Newer local timestamp must win for rec-2");

    // Verify rec-3 resolved to Cloud version due to node_id tiebreak ("node-b" > "node-a")
    let r3 = local_store.get(WORKSPACE, "rec-3").await.unwrap().unwrap();
    assert_eq!(r3.content, "Tiebreak cloud content", "Higher node_id lexicographical tiebreak must win for rec-3");

    // Verify complete manifest hash convergence
    let local_hash = compute_store_manifest_hash(&*local_store).await.unwrap();
    let cloud_hash = compute_store_manifest_hash(&*cloud_store).await.unwrap();
    assert_eq!(local_hash, cloud_hash, "Manifest hashes must converge after LWW resolution");
}

#[tokio::test]
async fn test_cloud_memory_sync_deletions_and_tombstones() {
    let local_store = Arc::new(InMemoryMemoryStore::new());
    let cloud_store = Arc::new(InMemoryMemoryStore::new());

    let (sync, _tmp_dir) = create_cloud_sync(cloud_store.clone(), "local-node-del", None).await;

    let now = Utc::now();

    // 1. Initial sync of active records
    let rec1 = make_record("del-1", WORKSPACE, "memories/del-1", "Active content 1", now, 1, "node-a", None);
    let rec2 = make_record("del-2", WORKSPACE, "memories/del-2", "Active content 2", now, 1, "node-a", None);

    local_store.put(rec1).await.unwrap();
    local_store.put(rec2).await.unwrap();

    sync.sync_all(&*local_store, WORKSPACE).await.unwrap();

    let initial_local_hash = compute_store_manifest_hash(&*local_store).await.unwrap();
    let initial_cloud_hash = compute_store_manifest_hash(&*cloud_store).await.unwrap();
    assert_eq!(initial_local_hash, initial_cloud_hash);

    // 2. Mark record 1 as deleted (tombstone) locally with newer timestamp
    let tombstone_time = now + TimeDelta::seconds(10);
    let rec1_tombstone = make_record(
        "del-1",
        WORKSPACE,
        "memories/del-1",
        "Active content 1",
        tombstone_time,
        2,
        "node-a",
        Some(tombstone_time),
    );
    local_store.put(rec1_tombstone).await.unwrap();

    // 3. Mark record 2 as deleted (tombstone) on cloud store with newer timestamp
    let rec2_tombstone = make_record(
        "del-2",
        WORKSPACE,
        "memories/del-2",
        "Active content 2",
        tombstone_time,
        2,
        "node-b",
        Some(tombstone_time),
    );
    cloud_store.put(rec2_tombstone).await.unwrap();

    // 4. Sync cycle
    sync.sync_all(&*local_store, WORKSPACE).await.unwrap();

    // Verify tombstone record 1 propagated to cloud store
    let cloud_r1 = cloud_store.get(WORKSPACE, "del-1").await.unwrap().unwrap();
    assert!(cloud_r1.deleted_at.is_some(), "Deleted tombstone timestamp for del-1 must sync to cloud store");

    // Verify tombstone record 2 propagated to local store
    let local_r2 = local_store.get(WORKSPACE, "del-2").await.unwrap().unwrap();
    assert!(local_r2.deleted_at.is_some(), "Deleted tombstone timestamp for del-2 must sync to local store");

    // Both stores converge to identical manifest hashes
    let local_hash = compute_store_manifest_hash(&*local_store).await.unwrap();
    let cloud_hash = compute_store_manifest_hash(&*cloud_store).await.unwrap();
    assert_eq!(local_hash, cloud_hash, "Manifest hashes must converge after deletion/tombstone sync");
}

#[tokio::test]
async fn test_cloud_memory_sync_pagination_over_batch_size_limit() {
    let local_store = Arc::new(InMemoryMemoryStore::new());
    let cloud_store = Arc::new(InMemoryMemoryStore::new());

    // Configure small batch size of 25 to force 5 batch chunks for 120 records
    let batch_size = 25;
    let total_records = 120;
    let (sync, _tmp_dir) = create_cloud_sync(cloud_store.clone(), "local-node-batch", Some(batch_size)).await;

    let now = Utc::now();
    for i in 0..total_records {
        let rec = make_record(
            &format!("batch-rec-{}", i),
            WORKSPACE,
            &format!("memories/batch-rec-{}", i),
            &format!("Batch item content {}", i),
            now + TimeDelta::milliseconds(i as i64),
            1,
            "node-a",
            None,
        );
        local_store.put(rec).await.unwrap();
    }

    // Execute push to cloud across batch chunks
    let report = sync.push_to_cloud(&*local_store, WORKSPACE).await.expect("push across batches");
    assert_eq!(report.pushed, total_records, "All 120 records should be pushed across batches");

    // Verify cloud received all 120 records
    let cloud_list = cloud_store.list(WORKSPACE).await.unwrap();
    assert_eq!(cloud_list.len(), total_records, "Cloud store must contain all 120 records");

    // Verify pulling from cloud into a fresh local node also paginates properly
    let fresh_local = Arc::new(InMemoryMemoryStore::new());
    let (sync_fresh, _tmp_fresh) = create_cloud_sync(cloud_store.clone(), "fresh-node-batch", Some(batch_size)).await;
    let pull_report = sync_fresh.pull_from_cloud(&*fresh_local, WORKSPACE).await.expect("pull across batches");
    assert_eq!(pull_report.pulled, total_records, "Fresh store must pull all 120 records");

    // Verify manifest hash convergence across all stores
    let original_hash = compute_store_manifest_hash(&*local_store).await.unwrap();
    let cloud_hash = compute_store_manifest_hash(&*cloud_store).await.unwrap();
    let fresh_hash = compute_store_manifest_hash(&*fresh_local).await.unwrap();

    assert_eq!(original_hash, cloud_hash, "Original local and cloud manifests must converge");
    assert_eq!(cloud_hash, fresh_hash, "Cloud and fresh local manifests must converge");
}
