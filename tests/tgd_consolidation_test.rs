use std::sync::Arc;
use xavier::consolidation::ConsolidationTask;
use xavier::tgd::{TgdEngine, TgdConfig};
use xavier::agents::provider::LlmProvider;
use xavier::agents::provider::types::LlmResponse;
use xavier::workspace::{WorkspaceConfig, WorkspaceContext, WorkspaceState};
use async_trait::async_trait;
use anyhow::Result;
use tempfile::tempdir;

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
    async fn generate_response(&self, _q: &str, _c: &[xavier::agents::system1::RetrievedDocument]) -> Result<LlmResponse> {
        unimplemented!()
    }
    async fn generate_hypothetical_document(&self, _q: &str) -> Result<LlmResponse> {
        unimplemented!()
    }
    async fn evaluate_context(&self, _q: &str, _c: &[xavier::agents::system1::RetrievedDocument]) -> Result<f32> {
        unimplemented!()
    }
}

#[tokio::test]
async fn test_tgd_consolidation_refinement() {
    std::env::set_var("XAVIER_TOKEN", "test-token");
    let temp_dir = tempdir().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    // Create .xavier directory for cache/rules
    tokio::fs::create_dir_all(workspace_dir.join(".xavier")).await.unwrap();

    let config = WorkspaceConfig::from_env();
    let runtime_config = xavier::agents::RuntimeConfig::default();
    let workspace_state = Arc::new(WorkspaceState::new(config, runtime_config, workspace_dir.clone()).await.unwrap());

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

    let stats = task.run_tgd_memory_refinement(&workspace_ctx, Some(&tgd_engine)).await.unwrap();

    // It might refine more than 1 if derivatives are created, so we just check it's at least 1
    assert!(stats.memories_refined >= 1);
    assert!(stats.avg_score_improvement > 0.0);

    // Verify at least one memory was updated with refined content
    let memories = workspace_ctx.workspace.memory_manager.get_all_memories().await.unwrap();
    let refined_exists = memories.iter().any(|m| {
        m.doc.content == "Refined Content" &&
        m.doc.metadata.get("tgd_refined").and_then(|v| v.as_bool()).unwrap_or(false)
    });
    assert!(refined_exists);
}
