use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tempfile::tempdir;
use xavier::agents::provider::types::LlmResponse;
use xavier::agents::provider::LlmProvider;
use xavier::consolidation::ConsolidationTask;
use xavier::tgd::{TgdConfig, TgdEngine};
use xavier::workspace::{WorkspaceConfig, WorkspaceContext, WorkspaceState};

struct MockRefinementProvider;

#[async_trait]
impl LlmProvider for MockRefinementProvider {
    async fn generate_text(&self, system: &str, _user: &str, _cache: bool) -> Result<LlmResponse> {
        if system.contains("evaluator") {
            Ok(LlmResponse {
                text: "0.9".to_string(),
                quota: None,
            })
        } else {
            Ok(LlmResponse {
                text: "Refined Content".to_string(),
                quota: None,
            })
        }
    }
    async fn generate_response(
        &self,
        _q: &str,
        _c: &[xavier::agents::system1::RetrievedDocument],
    ) -> Result<LlmResponse> {
        unimplemented!()
    }
    async fn generate_hypothetical_document(&self, _q: &str) -> Result<LlmResponse> {
        unimplemented!()
    }
    async fn evaluate_context(
        &self,
        _q: &str,
        _c: &[xavier::agents::system1::RetrievedDocument],
    ) -> Result<f32> {
        unimplemented!()
    }
}

#[tokio::test]
async fn test_tgd_consolidation_refinement() {
    std::env::set_var("XAVIER_TOKEN", "test-token");
    let temp_dir = tempdir().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    // Create .xavier directory for cache/rules
    tokio::fs::create_dir_all(workspace_dir.join(".xavier"))
        .await
        .unwrap();

    let config = WorkspaceConfig::from_env();
    let runtime_config = xavier::agents::RuntimeConfig::default();
    let workspace_state = Arc::new(
        WorkspaceState::new(config, runtime_config, workspace_dir.clone())
            .await
            .unwrap(),
    );

    let workspace_ctx = WorkspaceContext {
        workspace_id: "test-ws".to_string(),
        workspace: workspace_state,
    };

    // Use a path that won't trigger locomo derivatives if possible, or just expect them.
    // Actually, any document added might get a 'quality' score assigned.
    workspace_ctx.workspace.memory_manager.memory().add_document(
        "notes/test.md".to_string(),
        "Raw unrefined content that should be long enough to be considered a document and not just a fact.".to_string(),
        serde_json::json!({"memory_importance": 0.5})
    ).await.unwrap();

    let provider = Arc::new(MockRefinementProvider);
    let tgd_config = TgdConfig {
        iterations: 1,
        ..Default::default()
    };
    let tgd_engine = TgdEngine::with_config(provider, tgd_config);

    let task = ConsolidationTask {
        enable_tgd_in_consolidation: true,
        tgd_iterations: 1,
        ..Default::default()
    };

    let stats = task
        .run_tgd_memory_refinement(&workspace_ctx, Some(&tgd_engine))
        .await
        .unwrap();

    // It might refine more than 1 if derivatives are created, so we just check it's at least 1
    assert!(stats.memories_refined >= 1);
    assert!(stats.avg_score_improvement > 0.0);

    // Verify at least one memory was updated with refined content
    let memories = workspace_ctx
        .workspace
        .memory_manager
        .get_all_memories()
        .await
        .unwrap();
    let refined_exists = memories.iter().any(|m| {
        m.doc.content == "Refined Content"
            && m.doc
                .metadata
                .get("tgd_refined")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
    });
    assert!(refined_exists);
}

#[tokio::test]
async fn test_nightly_consolidation_process() {
    use tokio::sync::RwLock;
    use xavier::memory::qmd_memory::QmdMemory;
    use xavier::memory::manager::core::MemoryManager;
    use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
    use xavier::memory::store::MemoryStore;

    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("test_consolidation.db");

    let config = VecSqliteStoreConfig {
        path: db_path.clone(),
        embedding_dimensions: 384,
    };

    let store = Arc::new(VecSqliteMemoryStore::new(config).await.unwrap());

    let workspace_id = "test_ws";
    let qmd_memory = Arc::new(QmdMemory::new_with_workspace(
        Arc::new(RwLock::new(Vec::new())),
        workspace_id,
    ));
    qmd_memory.set_store(store.clone()).await;
    qmd_memory.init().await.unwrap();

    let manager = MemoryManager::new(Arc::clone(&qmd_memory), None);

    // 1. Add identical duplicates (relying on MemoryManager consolidation signature)
    qmd_memory
        .add_document(
            "docs/duplicate1".to_string(),
            "This is some repetitive content that will be consolidated.".to_string(),
            serde_json::json!({ "memory_priority": "medium", "kind": "fact" }),
        )
        .await
        .unwrap();

    qmd_memory
        .add_document(
            "docs/duplicate2".to_string(),
            "This is some repetitive content that will be consolidated.".to_string(),
            serde_json::json!({ "memory_priority": "medium", "kind": "fact" }),
        )
        .await
        .unwrap();

    // 2. Add an expired memory with expires_at in the past
    let past_time = chrono::Utc::now() - chrono::Duration::hours(1);
    qmd_memory
        .add_document(
            "docs/expired1".to_string(),
            "This memory has expired in the past.".to_string(),
            serde_json::json!({ "expires_at": past_time.to_rfc3339() }),
        )
        .await
        .unwrap();

    // 3. Add an expired memory with epoch timestamp in the past
    let past_epoch = chrono::Utc::now().timestamp() - 3600;
    qmd_memory
        .add_document(
            "docs/expired2".to_string(),
            "This memory has expired via Unix epoch timestamp.".to_string(),
            serde_json::json!({ "expires_at": past_epoch }),
        )
        .await
        .unwrap();

    // 4. Add an expired memory via short TTL
    qmd_memory
        .add_document(
            "docs/expired_ttl".to_string(),
            "This memory has a very short TTL.".to_string(),
            serde_json::json!({ "ttl": 1 }),
        )
        .await
        .unwrap();

    // 5. Add an active memory with expires_at in the future
    let future_time = chrono::Utc::now() + chrono::Duration::hours(12);
    qmd_memory
        .add_document(
            "docs/active1".to_string(),
            "This memory is still active and should not be purged.".to_string(),
            serde_json::json!({ "expires_at": future_time.to_rfc3339() }),
        )
        .await
        .unwrap();

    // Wait 2 seconds to ensure the TTL memory has expired
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Check count before nightly consolidation
    let list_before = store.list(workspace_id).await.unwrap();
    assert!(
        list_before.iter().any(|m| m.path == "docs/duplicate1"),
        "docs/duplicate1 should exist"
    );
    assert!(
        list_before.iter().any(|m| m.path == "docs/duplicate2"),
        "docs/duplicate2 should exist"
    );
    assert!(
        list_before.iter().any(|m| m.path == "docs/expired1"),
        "docs/expired1 should exist"
    );
    assert!(
        list_before.iter().any(|m| m.path == "docs/expired2"),
        "docs/expired2 should exist"
    );
    assert!(
        list_before.iter().any(|m| m.path == "docs/expired_ttl"),
        "docs/expired_ttl should exist"
    );
    assert!(
        list_before.iter().any(|m| m.path == "docs/active1"),
        "docs/active1 should exist"
    );

    // Run nightly consolidation
    let result = manager.nightly_consolidate().await.unwrap();

    // Ensure documents were affected (at least the duplicates and the expired ones)
    assert!(result.documents_affected >= 1);

    let list_after = store.list(workspace_id).await.unwrap();

    // Verify deduplication: one duplicate should be consolidated/deleted
    let has_dup1 = list_after.iter().any(|m| m.path == "docs/duplicate1");
    let has_dup2 = list_after.iter().any(|m| m.path == "docs/duplicate2");
    assert!(
        !(has_dup1 && has_dup2),
        "At least one of the duplicate documents should have been consolidated and removed."
    );

    // Verify purging: expired memories should be gone
    assert!(
        !list_after.iter().any(|m| m.path == "docs/expired1"),
        "docs/expired1 should have been purged."
    );
    assert!(
        !list_after.iter().any(|m| m.path == "docs/expired2"),
        "docs/expired2 should have been purged."
    );
    assert!(
        !list_after.iter().any(|m| m.path == "docs/expired_ttl"),
        "docs/expired_ttl should have been purged."
    );

    // Verify active memory is preserved
    assert!(
        list_after.iter().any(|m| m.path == "docs/active1"),
        "docs/active1 should still be present."
    );

    // Verify compaction can run and db size helper works
    let size_after = store.db_size().await.unwrap().unwrap_or(0);
    assert!(size_after > 0, "Database size should be greater than zero.");
}
