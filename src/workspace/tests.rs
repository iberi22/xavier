//! Tests for workspace module
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use super::*;
use crate::agents::RuntimeConfig;
use crate::memory::store::MemoryBackend;
use crate::workspace::config::MB;
use ulid::Ulid;

#[tokio::test]
async fn personal_plan_defaults_to_500mb() {
    let config = WorkspaceConfig {
        id: "ws".to_string(),
        token: "token".to_string(),
        plan: PlanTier::Personal,
        memory_backend: MemoryBackend::File,
        storage_limit_bytes: PlanTier::Personal.default_storage_limit_bytes(),
        request_limit: PlanTier::Personal.default_request_limit(),
        request_unit_limit: Some(100_000),
        embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
        managed_google_embeddings: false,
        sync_policy: SyncPolicy::CloudMirror,
    };

    let workspace = WorkspaceState::new(
        config,
        RuntimeConfig::default(),
        std::env::temp_dir().join(format!("xavier-ws-{}", Ulid::new())),
    )
    .await
    .expect("test assertion");

    assert_eq!(workspace.config.storage_limit_bytes, Some(500 * MB));
    assert_eq!(workspace.config.request_limit, Some(50_000));
}

#[test]
fn usage_event_weights_sync_and_agent_calls_higher() {
    let sync = UsageEvent::from_request("POST", "/sync");
    let agent = UsageEvent::from_request("POST", "/agents/run");
    let read = UsageEvent::from_request("POST", "/memory/search");

    assert_eq!(sync.category, UsageCategory::Sync);
    assert_eq!(sync.units, 5);
    assert_eq!(agent.category, UsageCategory::AgentRun);
    assert_eq!(agent.units, 10);
    assert_eq!(read.category, UsageCategory::Read);
    assert_eq!(read.units, 1);
}

#[tokio::test]
async fn usage_state_persists_between_workspace_reloads() {
    let unique_id = Ulid::new().to_string();
    let root = std::env::temp_dir().join(format!("xavier-usage-{}", unique_id));
    let workspace = WorkspaceState::new(
        WorkspaceConfig {
            id: format!("persist-{}", unique_id),
            token: "token".to_string(),
            plan: PlanTier::Personal,
            memory_backend: MemoryBackend::File,
            storage_limit_bytes: Some(500 * MB),
            request_limit: Some(50_000),
            request_unit_limit: Some(100_000),
            embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: SyncPolicy::CloudMirror,
        },
        RuntimeConfig::default(),
        &root,
    )
    .await
    .expect("test assertion");

    workspace
        .record_request(UsageEvent::from_request("POST", "/sync"))
        .await
        .expect("test assertion");
    workspace
        .record_request(UsageEvent::from_request("POST", "/agents/run"))
        .await
        .expect("test assertion");

    let reloaded = WorkspaceState::new(
        WorkspaceConfig {
            id: format!("persist-{}", unique_id),
            token: "token".to_string(),
            plan: PlanTier::Personal,
            memory_backend: MemoryBackend::File,
            storage_limit_bytes: Some(500 * MB),
            request_limit: Some(50_000),
            request_unit_limit: Some(100_000),
            embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: SyncPolicy::CloudMirror,
        },
        RuntimeConfig::default(),
        &root,
    )
    .await
    .expect("test assertion");

    let usage = reloaded.usage_snapshot().await;
    assert_eq!(usage.requests_used, 2);
    assert_eq!(usage.request_units_used, 15);
}

#[tokio::test]
async fn durable_memory_rehydrates_between_workspace_reloads() {
    let unique_id = Ulid::new().to_string();
    let root = std::env::temp_dir().join(format!("xavier-memory-{}", unique_id));
    let config = WorkspaceConfig {
        id: format!("persist-memory-{}", unique_id),
        token: "token".to_string(),
        plan: PlanTier::Personal,
        memory_backend: MemoryBackend::File,
        storage_limit_bytes: Some(500 * MB),
        request_limit: Some(50_000),
        request_unit_limit: Some(100_000),
        embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
        managed_google_embeddings: false,
        sync_policy: SyncPolicy::CloudMirror,
    };

    let workspace = WorkspaceState::new(config.clone(), RuntimeConfig::default(), &root)
        .await
        .expect("test assertion");
    let doc_id = workspace
        .memory
        .add_document_typed(
            "projects/xavier/core".to_string(),
            "Durable memory survives restarts.".to_string(),
            serde_json::json!({"project":"xavier"}),
            Some(crate::memory::schema::TypedMemoryPayload {
                kind: Some(crate::memory::schema::MemoryKind::Semantic),
                evidence_kind: Some(crate::memory::schema::EvidenceKind::Observation),
                namespace: Some(crate::memory::schema::MemoryNamespace {
                    project: Some("xavier".to_string()),
                    ..crate::memory::schema::MemoryNamespace::default()
                }),
                provenance: None,
                ..crate::memory::schema::TypedMemoryPayload::default()
            }),
        )
        .await
        .expect("test assertion");

    let reloaded = WorkspaceState::new(config, RuntimeConfig::default(), &root)
        .await
        .expect("test assertion");
    let doc = reloaded
        .memory
        .get(&doc_id)
        .await
        .expect("test assertion")
        .expect("test assertion");
    assert_eq!(doc.content, "Durable memory survives restarts.");
    assert_eq!(doc.metadata["kind"].as_str(), Some("semantic"));
}

#[tokio::test]
async fn session_tokens_beliefs_and_checkpoints_persist_between_reloads() {
    let unique_id = Ulid::new().to_string();
    let root = std::env::temp_dir().join(format!("xavier-state-{}", unique_id));
    let config = WorkspaceConfig {
        id: format!("persist-state-{}", unique_id),
        token: "token".to_string(),
        plan: PlanTier::Personal,
        memory_backend: MemoryBackend::File,
        storage_limit_bytes: Some(500 * MB),
        request_limit: Some(50_000),
        request_unit_limit: Some(100_000),
        embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
        managed_google_embeddings: false,
        sync_policy: SyncPolicy::CloudMirror,
    };

    let workspace = WorkspaceState::new(config.clone(), RuntimeConfig::default(), &root)
        .await
        .expect("test assertion");
    let session_token = workspace
        .generate_session_token()
        .await
        .expect("test assertion");
    workspace
        .belief_graph
        .read()
        .await
        .add_edge(
            "xavier".to_string(),
            "memory".to_string(),
            "is_a".to_string(),
        )
        .await;
    workspace.persist_beliefs().await.expect("test assertion");
    workspace
        .checkpoint_manager
        .save(crate::checkpoint::Checkpoint::new(
            "task-1".to_string(),
            "restore".to_string(),
            serde_json::json!({"ok": true}),
        ))
        .await
        .expect("test assertion");

    let reloaded = WorkspaceState::new(config, RuntimeConfig::default(), &root)
        .await
        .expect("test assertion");
    assert!(reloaded.is_session_token_valid(&session_token).await);
    assert!(
        !reloaded.belief_graph.read().await.get_relations().is_empty(),
        "Expected at least 1 relation after persist, got {}",
        reloaded.belief_graph.read().await.get_relations().len()
    );
    let checkpoint = reloaded
        .checkpoint_manager
        .load("task-1".to_string(), "restore".to_string())
        .await
        .expect("test assertion");
    assert!(checkpoint.is_some());
}
