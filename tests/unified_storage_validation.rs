use chrono::Utc;
use tempfile::tempdir;
use xavier::domain::memory::belief::BeliefEdge;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::memory::store::{HybridSearchMode, MemoryRecord, MemoryStore};

#[tokio::test]
async fn test_unified_storage_persistence() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("unified_test.db");

    let config = VecSqliteStoreConfig {
        path: db_path.clone(),
        embedding_dimensions: 3,
    };

    let store = VecSqliteMemoryStore::new(config).await.unwrap();
    let workspace_id = "test_ws";

    // 1. Test Memory + Vector persistence
    let now = Utc::now();
    let record = MemoryRecord {
        id: "mem1".to_string(),
        workspace_id: workspace_id.to_string(),
        path: "test/path".to_string(),
        content: "This is a test memory with #topic and @mention".to_string(),
        embedding: vec![1.0, 0.0, 0.0],
        created_at: now,
        updated_at: now,
        ..Default::default()
    };

    store.put(record.clone()).await.unwrap();

    // Verify it can be retrieved
    let retrieved = store
        .get(workspace_id, "test/path")
        .await
        .unwrap()
        .expect("Record not found");
    assert_eq!(retrieved.content, record.content);
    assert_eq!(retrieved.embedding, record.embedding);

    // Verify hybrid search works (vector + FTS)
    let search_results = store
        .hybrid_search(workspace_id, "test", HybridSearchMode::Both, None, 10)
        .await
        .unwrap();
    assert!(!search_results.is_empty());
    assert_eq!(search_results[0].record.id, "mem1");

    // 2. Test Graph Data persistence
    let belief = BeliefEdge::new(
        "source_entity".to_string(),
        "target_entity".to_string(),
        "test_relation".to_string(),
        0.9,
        "mem1".to_string(),
    );

    store
        .save_beliefs(workspace_id, vec![belief.clone()])
        .await
        .unwrap();

    // Verify belief is persisted in the unified store
    let state = store.load_workspace_state(workspace_id).await.unwrap();
    assert!(state
        .beliefs
        .iter()
        .any(|b| b.relation_type == "test_relation"));

    // 3. Test Entity extraction persistence
    // After put(), sync_memory_entities should have been called
    // We check if entities were created.
    // In load_workspace_state for VecSqliteMemoryStore, it currently loads beliefs from 'relations' table.
    // Let's check if the entities from the content were extracted.
    let state = store.load_workspace_state(workspace_id).await.unwrap();
    // The extraction logic in graph.rs creates entities for #topic and @mention
    // and relations between the memory node and these entities.
    assert!(state.beliefs.iter().any(|b| b.relation_type == "tags"));
    assert!(state.beliefs.iter().any(|b| b.relation_type == "mentions"));
}

#[tokio::test]
async fn test_unified_graph_state_is_workspace_isolated() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("isolated_graph.db");

    let store = VecSqliteMemoryStore::new(VecSqliteStoreConfig {
        path: db_path,
        embedding_dimensions: 3,
    })
    .await
    .unwrap();

    let alpha = BeliefEdge::new(
        "alpha_source".to_string(),
        "alpha_target".to_string(),
        "alpha_only".to_string(),
        0.9,
        "alpha_mem".to_string(),
    );
    let beta = BeliefEdge::new(
        "beta_source".to_string(),
        "beta_target".to_string(),
        "beta_only".to_string(),
        0.9,
        "beta_mem".to_string(),
    );

    store
        .save_beliefs("workspace_alpha", vec![alpha])
        .await
        .unwrap();
    store
        .save_beliefs("workspace_beta", vec![beta])
        .await
        .unwrap();

    let alpha_state = store.load_workspace_state("workspace_alpha").await.unwrap();
    let beta_state = store.load_workspace_state("workspace_beta").await.unwrap();

    assert!(alpha_state
        .beliefs
        .iter()
        .any(|belief| belief.relation_type == "alpha_only"));
    assert!(!alpha_state
        .beliefs
        .iter()
        .any(|belief| belief.relation_type == "beta_only"));
    assert!(beta_state
        .beliefs
        .iter()
        .any(|belief| belief.relation_type == "beta_only"));
    assert!(!beta_state
        .beliefs
        .iter()
        .any(|belief| belief.relation_type == "alpha_only"));
}
