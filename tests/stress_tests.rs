use std::sync::Arc;
use tokio::sync::{Barrier, RwLock};
use tokio::task::JoinSet;
use tempfile::TempDir;
use serde_json::json;
use xavier::memory::qmd_memory::QmdMemory;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};

/// Helper to setup a QmdMemory engine with a temporary SQLite-vec store.
async fn setup_stress_test_engine() -> (QmdMemory, TempDir) {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    // Note: VecSqliteMemoryStore requires a physical file path to load the sqlite-vec extension correctly.
    // We cannot use a purely in-memory connection like `:memory:` here without breaking extension loading on some systems.
    let db_path = temp_dir.path().join("stress_test.db");

    let config = VecSqliteStoreConfig {
        path: db_path,
        embedding_dimensions: 768,
    };

    let store = VecSqliteMemoryStore::new(config)
        .await
        .expect("failed to create VecSqliteMemoryStore");

    let memory = QmdMemory::new(Arc::new(RwLock::new(Vec::new())));
    memory.set_store(Arc::new(store)).await;

    (memory, temp_dir)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_qmd_high_concurrency() {
    // Disable embedder to avoid external API calls and focus on DB/Lock contention
    std::env::set_var("XAVIER_EMBEDDER", "disabled");

    let (memory, _temp_dir) = setup_stress_test_engine().await;
    let memory = Arc::new(memory);

    let num_writers = 50;
    let num_readers = 50;
    let total_tasks = num_writers + num_readers;

    let barrier = Arc::new(Barrier::new(total_tasks));
    let mut set = JoinSet::new();

    // Spawn writers
    for i in 0..num_writers {
        let mem = Arc::clone(&memory);
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            for j in 0..5 {
                let path = format!("stress/writer/{}/{}", i, j);
                let content = format!("High-concurrency stress test content from writer {} loop {}", i, j);
                mem.add_document(path, content, json!({ "writer_id": i, "loop": j }))
                    .await?;
            }
            Ok::<(), anyhow::Error>(())
        });
    }

    // Spawn readers
    for _ in 0..num_readers {
        let mem = Arc::clone(&memory);
        let barrier = Arc::clone(&barrier);
        set.spawn(async move {
            barrier.wait().await;
            // Execute multiple searches to increase contention duration
            for _ in 0..5 {
                let _results = mem.search("stress", 10).await?;
            }
            Ok::<(), anyhow::Error>(())
        });
    }

    // Await all tasks and check for errors
    while let Some(res) = set.join_next().await {
        match res {
            Ok(task_res) => {
                if let Err(e) = task_res {
                    panic!("Task failed with error: {:?}", e);
                }
            }
            Err(e) => panic!("Task panicked: {:?}", e),
        }
    }

    // Final check: all documents should be there (at least the ones added by writers)
    let count = memory.count().await.expect("failed to count documents");
    assert!(count >= num_writers * 5, "Expected at least {} documents, found {}", num_writers * 5, count);
}
