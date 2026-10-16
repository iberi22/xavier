use std::sync::Arc;
use std::time::SystemTime;

use xavier::memory::qmd_memory::QmdMemory;
use xavier::memory::store::{InMemoryMemoryStore, MemoryRecord, MemoryStore};
use xavier::memory::sync::index::index_applied_changes;
use xavier::memory::sync::merge::apply_changes_received;
use xavier::memory::sync::{ChunkDiff, DiffAction, PeerMemorySync};
use xavier::security::clearance::ClearanceLevel;

fn record(workspace_id: &str, path: &str, content: &str) -> MemoryRecord {
    let now = chrono::Utc::now();
    MemoryRecord {
        id: format!("{workspace_id}:{path}"),
        workspace_id: workspace_id.to_string(),
        path: path.to_string(),
        content: content.to_string(),
        metadata: serde_json::json!({"kind": "Context", "namespace": "Global"}),
        created_at: now,
        updated_at: now,
        revision: 1,
        primary: true,
        ..Default::default()
    }
}

fn data_diff(action: DiffAction, record: &MemoryRecord) -> ChunkDiff {
    ChunkDiff {
        chunk_hash: format!("hash-{}-{}", record.id, record.updated_at),
        namespace: record.workspace_id.clone(),
        action,
        data: Some(serde_json::to_vec(record).expect("serialise record")),
        timestamp: SystemTime::now(),
        record_path: Some(record.path.clone()),
    }
}

fn delete_diff(workspace_id: &str, path: &str) -> ChunkDiff {
    ChunkDiff {
        chunk_hash: format!("hash-delete-{path}"),
        namespace: workspace_id.to_string(),
        action: DiffAction::Delete,
        data: None,
        timestamp: SystemTime::now(),
        record_path: Some(path.to_string()),
    }
}

/// Applies a push-style batch to the store, then mirrors it into the index,
/// the way the receiving push handler does.
async fn receive(store: &dyn MemoryStore, qmd: &QmdMemory, diffs: &[ChunkDiff]) {
    let mut conflicts = 0u64;
    apply_changes_received(store, diffs, &mut conflicts)
        .await
        .expect("apply changes");
    index_applied_changes(store, qmd, "default", diffs).await;
}

async fn search_docs(qmd: &QmdMemory, query: &str) -> Vec<(String, String)> {
    qmd.search(query, 10)
        .await
        .expect("search index")
        .into_iter()
        .map(|doc| (doc.path, doc.content))
        .collect()
}

async fn new_index(store: &Arc<dyn MemoryStore>) -> Arc<QmdMemory> {
    let qmd = QmdMemory::new_lazy("default");
    qmd.set_store(Arc::clone(store)).await;
    qmd.init().await.expect("init qmd");
    Arc::new(qmd)
}

#[tokio::test]
async fn pushed_record_is_searchable_and_delete_leaves_index() {
    let store: Arc<dyn MemoryStore> = Arc::new(InMemoryMemoryStore::new());
    let qmd = new_index(&store).await;

    let local = record(
        "default",
        "episodic/tech/blueprints",
        "Secret blueprints of the warp drive",
    );
    let foreign = record(
        "other",
        "episodic/tech/foreign",
        "Foreign warp drive blueprints",
    );

    receive(
        &*store,
        &qmd,
        &[
            data_diff(DiffAction::Add, &local),
            data_diff(DiffAction::Add, &foreign),
        ],
    )
    .await;

    let hits = search_docs(&qmd, "warp drive").await;
    let paths: Vec<&str> = hits.iter().map(|(path, _)| path.as_str()).collect();
    assert!(
        paths.contains(&"episodic/tech/blueprints"),
        "pushed record should be searchable on the receiver, got {paths:?}"
    );
    assert!(
        !paths.contains(&"episodic/tech/foreign"),
        "a record of another workspace must not enter this index, got {paths:?}"
    );

    receive(
        &*store,
        &qmd,
        &[delete_diff("default", "episodic/tech/blueprints")],
    )
    .await;

    let hits = search_docs(&qmd, "warp drive").await;
    assert!(
        hits.iter()
            .all(|(path, _)| path != "episodic/tech/blueprints"),
        "a pushed delete should remove the record from the index, got {hits:?}"
    );
}

#[tokio::test]
async fn updated_record_replaces_its_index_entry() {
    let store: Arc<dyn MemoryStore> = Arc::new(InMemoryMemoryStore::new());
    let qmd = new_index(&store).await;

    let first = record(
        "default",
        "episodic/tech/fusion",
        "Nuclear energy (A version)",
    );
    receive(&*store, &qmd, &[data_diff(DiffAction::Add, &first)]).await;

    let mut newer = record(
        "default",
        "episodic/tech/fusion",
        "Clean nuclear fusion (B version)",
    );
    newer.updated_at = first.updated_at + chrono::Duration::seconds(1);
    receive(&*store, &qmd, &[data_diff(DiffAction::Update, &newer)]).await;

    let hits = search_docs(&qmd, "nuclear").await;
    assert_eq!(
        hits.len(),
        1,
        "the update must replace, not duplicate: {hits:?}"
    );
    assert!(hits[0].1.contains("Clean nuclear fusion (B version)"));
}

#[tokio::test]
async fn pulled_records_are_indexed_through_peer_sync() {
    let store: Arc<dyn MemoryStore> = Arc::new(InMemoryMemoryStore::new());
    let qmd = new_index(&store).await;

    let mut sync = PeerMemorySync::new(Arc::clone(&store), "node-a".to_string())
        .with_search_index(Arc::clone(&qmd));
    sync.clearance_ceiling = ClearanceLevel::TopSecret;

    let pulled = record(
        "default",
        "episodic/tech/blueprints",
        "Secret blueprints of the warp drive",
    );
    let mut conflicts = 0u64;
    sync.apply_received_diffs(&[data_diff(DiffAction::Add, &pulled)], &mut conflicts)
        .await
        .expect("apply pulled diffs");

    let hits = search_docs(&qmd, "warp drive").await;
    assert!(
        hits.iter()
            .any(|(path, _)| path == "episodic/tech/blueprints"),
        "a pulled record should be searchable on this node, got {hits:?}"
    );
}

xavier::isolate_test_process!();

/// Indexing a received record must not write it back to the store: a store
/// write bumps revision/updated_at, changes the chunk hash and makes the next
/// sync echo the record back to the sender.
#[tokio::test]
async fn indexing_received_record_does_not_rewrite_store() {
    let store: Arc<dyn MemoryStore> = Arc::new(InMemoryMemoryStore::new());
    let qmd = new_index(&store).await;
    let local = record("default", "episodic/tech/stable", "Stable revision record");
    for action in [DiffAction::Add, DiffAction::Update] {
        let diffs = [data_diff(action, &local)];
        let mut conflicts = 0u64;
        apply_changes_received(&*store, &diffs, &mut conflicts)
            .await
            .expect("apply changes");
        let applied = store
            .get("default", &local.path)
            .await
            .expect("get")
            .expect("record applied");
        index_applied_changes(&*store, &qmd, "default", &diffs).await;
        let after = store
            .get("default", &local.path)
            .await
            .expect("get")
            .expect("record still present");
        assert_eq!(
            after.revision, applied.revision,
            "indexing bumped the revision"
        );
        assert_eq!(
            after.updated_at, applied.updated_at,
            "indexing moved updated_at"
        );
    }
    let hits = search_docs(&qmd, "Stable revision").await;
    assert_eq!(
        hits.iter()
            .filter(|(p, _)| p == "episodic/tech/stable")
            .count(),
        1,
        "record should be indexed exactly once, got {hits:?}"
    );
}
