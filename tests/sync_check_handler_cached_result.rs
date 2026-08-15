use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use xavier::adapters::inbound::http::routes::sync_check_handler;
use xavier::adapters::outbound::http_health_adapter::HttpHealthAdapter;
use xavier::memory::schema::MemoryQueryFilters;
use xavier::memory::store::{DurableWorkspaceState, MemoryRecord, MemoryStore, SessionTokenRecord};
use xavier::tasks::session_sync_task::SessionSyncTask;

struct MockMemoryStore {
    records: Vec<MemoryRecord>,
}

#[async_trait]
impl MemoryStore for MockMemoryStore {
    async fn put(&self, _record: MemoryRecord) -> anyhow::Result<()> {
        Ok(())
    }

    async fn get(
        &self,
        _workspace_id: &str,
        _id_or_path: &str,
    ) -> anyhow::Result<Option<MemoryRecord>> {
        Ok(None)
    }

    async fn list(&self, _workspace_id: &str) -> anyhow::Result<Vec<MemoryRecord>> {
        Ok(self.records.clone())
    }

    async fn update(&self, _record: MemoryRecord) -> anyhow::Result<()> {
        Ok(())
    }

    async fn list_filtered(
        &self,
        _workspace_id: &str,
        _filters: &MemoryQueryFilters,
        _limit: usize,
    ) -> anyhow::Result<Vec<MemoryRecord>> {
        Ok(self.records.clone())
    }

    async fn search(
        &self,
        _workspace_id: &str,
        _query: &str,
        _filters: Option<&MemoryQueryFilters>,
    ) -> anyhow::Result<Vec<MemoryRecord>> {
        Ok(Vec::new())
    }

    async fn delete(
        &self,
        _workspace_id: &str,
        _id_or_path: &str,
    ) -> anyhow::Result<Option<MemoryRecord>> {
        Ok(None)
    }

    async fn load_workspace_state(
        &self,
        _workspace_id: &str,
    ) -> anyhow::Result<DurableWorkspaceState> {
        Ok(DurableWorkspaceState {
            memories: self.records.clone(),
            beliefs: Vec::new(),
            session_tokens: Vec::new(),
            checkpoints: Vec::new(),
            entity_graph_snapshot: None,
        })
    }

    async fn save_beliefs(
        &self,
        _workspace_id: &str,
        _beliefs: Vec<xavier::domain::memory::belief::BeliefEdge>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn save_session_token(
        &self,
        _workspace_id: &str,
        _token: SessionTokenRecord,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn is_session_token_valid(
        &self,
        _workspace_id: &str,
        _token: &str,
    ) -> anyhow::Result<bool> {
        Ok(true)
    }

    async fn save_checkpoint(
        &self,
        _workspace_id: &str,
        _checkpoint: xavier::checkpoint::Checkpoint,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn load_checkpoint(
        &self,
        _workspace_id: &str,
        _task_id: &str,
        _name: &str,
    ) -> anyhow::Result<Option<xavier::checkpoint::Checkpoint>> {
        Ok(None)
    }

    async fn list_checkpoints(
        &self,
        _workspace_id: &str,
        _task_id: &str,
    ) -> anyhow::Result<Vec<xavier::checkpoint::Checkpoint>> {
        Ok(Vec::new())
    }

    async fn delete_checkpoint(
        &self,
        _workspace_id: &str,
        _task_id: &str,
        _name: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn backend(&self) -> xavier::memory::store::MemoryBackend {
        xavier::memory::store::MemoryBackend::Memory
    }

    async fn health(&self) -> anyhow::Result<String> {
        Ok("ok".to_string())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn make_session_record(seconds_ago: i64) -> MemoryRecord {
    let event_at = Utc::now() - chrono::Duration::seconds(seconds_ago);
    let indexed_at = Utc::now();
    MemoryRecord {
        id: "session-1".to_string(),
        workspace_id: "default".to_string(),
        path: "test".to_string(),
        content: serde_json::json!({
            "session_id": "session-1",
            "event_type": "message",
            "timestamp": event_at,
            "content": "test",
        })
        .to_string(),
        metadata: serde_json::json!({
            "kind": "session",
        }),
        embedding: vec![],
        created_at: event_at,
        updated_at: indexed_at,
        revision: 1,
        primary: true,
        parent_id: None,
        revisions: vec![],
        cluster_id: None,
        level: xavier::memory::schema::MemoryLevel::Raw,
        relation: None,
        clearance: Default::default(),
        encrypted_dek: None,
        content_iv: None,
        metadata_iv: None,
        score: 0.0,
        deleted_at: None,
        ..Default::default()
    }
}

#[tokio::test]
async fn sync_check_handler_returns_cached_result_from_session_sync_task() {
    // Reset or ensure state is fresh
    SessionSyncTask::update_metrics(1.0, 1.0, 0);

    let storage = Arc::new(MockMemoryStore {
        records: vec![make_session_record(45)],
    }) as Arc<dyn MemoryStore>;

    let health = Arc::new(HttpHealthAdapter::new(
        "http://127.0.0.1:1".to_string(),
        reqwest::Client::new(),
    ));

    let task = SessionSyncTask::with_storage(health, Some(storage));

    let sync_result = task.run_sync_check().await;
    let axum::Json(response) = sync_check_handler().await;

    assert_eq!(response.active_agents, 0);
    assert_eq!(response.save_ok_rate, 1.0); // Default if check fails or not updated
    assert_eq!(response.match_score, 1.0);
    assert_eq!(response.timestamp_ms, sync_result.timestamp_ms);
    assert!(!response.alerts.is_empty());
    assert!(response.alerts.iter().any(|a| a.contains("unreachable")));
}
