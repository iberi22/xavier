use std::sync::Arc;
use xavier::memory::connection_provider::{ConnectionProvider, InMemoryProvider};
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::memory::store::{MemoryRecord, MemoryStore};

xavier::isolate_test_process!();

#[tokio::test]
async fn test_put_index_atomic_rollback() {
    // Disable embedding logic so it doesn't attempt to connect to network/external providers
    std::env::set_var("XAVIER_EMBEDDER", "disabled");
    std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "disabled");
    std::env::set_var("XAVIER_EMBED_PROVIDER", "disabled");

    let root = tempfile::tempdir().unwrap();
    let config = VecSqliteStoreConfig {
        path: root.path().join("memory.db"),
        embedding_dimensions: 3,
    };

    let provider: Arc<dyn ConnectionProvider> = Arc::new(InMemoryProvider::new());
    let store = VecSqliteMemoryStore::new_with_provider(config, provider.clone())
        .await
        .unwrap();

    let workspace_id = "test_ws";

    // Create a valid record
    let record_valid = MemoryRecord {
        id: "atomic_test_id".to_string(),
        workspace_id: workspace_id.to_string(),
        path: "test/atomic".to_string(),
        content: "valid content that works".to_string(),
        embedding: vec![0.1, 0.2, 0.3], // Valid length 3
        ..Default::default()
    };

    // First put should succeed
    store.put(record_valid.clone()).await.unwrap();

    // Verify it is there
    let found = store
        .search(workspace_id, "valid content", None)
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].content, "valid content that works");

    // Force failure in the embedding insert using a trigger to prove atomic rollback
    let project_id = store.connection_project_id().to_string();
    provider
        .with_conn(&project_id, |conn| {
            conn.execute(
                "CREATE TRIGGER fail_emb BEFORE INSERT ON memory_embeddings BEGIN SELECT RAISE(ABORT, 'forced'); END;",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    // Create an update record
    let record_invalid = MemoryRecord {
        id: "atomic_test_id".to_string(),
        workspace_id: workspace_id.to_string(),
        path: "test/atomic".to_string(),
        content: "new content that fails".to_string(),
        embedding: vec![0.1, 0.2, 0.3],
        ..Default::default()
    };

    // Attempting to overwrite should fail in the vec insertion inside put_index
    let result = store.put(record_invalid.clone()).await;
    assert!(
        result.is_err(),
        "Put should fail due to forced trigger rollback"
    );

    // Verify the FTS index rolled back: searching for new content finds nothing
    let found_new = store.search(workspace_id, "fails", None).await.unwrap();
    assert_eq!(found_new.len(), 0);

    // Verify the FTS index rolled back: searching for old content still finds the original record
    let found_old = store.search(workspace_id, "works", None).await.unwrap();
    assert_eq!(found_old.len(), 1);
    assert_eq!(found_old[0].content, "valid content that works");
}
