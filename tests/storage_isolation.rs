use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::RwLock;
use xavier::memory::qmd_memory::QmdMemory;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};

#[tokio::test]
async fn test_workspace_isolation() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("test_isolation.db");

    let config = VecSqliteStoreConfig {
        path: db_path.clone(),
        embedding_dimensions: 384,
    };

    let store = Arc::new(VecSqliteMemoryStore::new(config).await.unwrap());

    // Workspace A
    let ws_a = "workspace_a";
    let memory_a = QmdMemory::new_with_workspace(Arc::new(RwLock::new(Vec::new())), ws_a);
    memory_a.set_store(store.clone()).await;

    memory_a
        .add_document(
            "path/a".to_string(),
            "content for A".to_string(),
            serde_json::json!({}),
        )
        .await
        .unwrap();

    // Workspace B
    let ws_b = "workspace_b";
    let memory_b = QmdMemory::new_with_workspace(Arc::new(RwLock::new(Vec::new())), ws_b);
    memory_b.set_store(store.clone()).await;

    memory_b
        .add_document(
            "path/b".to_string(),
            "content for B".to_string(),
            serde_json::json!({}),
        )
        .await
        .unwrap();

    // Search in A should NOT find B
    let results_a = memory_a.search("content", 10).await.unwrap();
    assert_eq!(results_a.len(), 1);
    assert_eq!(results_a[0].content, "content for A");

    // Search in B should NOT find A
    let results_b = memory_b.search("content", 10).await.unwrap();
    assert_eq!(results_b.len(), 1);
    assert_eq!(results_b[0].content, "content for B");
}
