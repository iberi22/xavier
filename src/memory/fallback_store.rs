//! Fallback memory store implementation.
//!
//! Provides `FallbackMemoryStore`, a fallback chain wrapper around multiple `MemoryStore`
//! instances (e.g. Vec -> Supabase -> File). Reads iterate through the chain until a store
//! returns a non-empty/successful result, and writes attempt persistence against the chain
//! until one succeeds.

use std::{any::Any as StdAny, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;

use crate::checkpoint::Checkpoint;
use crate::domain::memory::belief::BeliefEdge;
use crate::memory::hierarchy::MemoryHierarchyNode;
use crate::memory::postgres_store::PostgresMemoryStore;
use crate::memory::schema::MemoryQueryFilters;
use crate::memory::sqlite_store::SqliteMemoryStore;
use crate::memory::sqlite_vec_store::VecSqliteMemoryStore;
use crate::memory::store::{
    DurableWorkspaceState, FileMemoryStore, GraphHopResult, HybridSearchMode, HybridSearchResult,
    InMemoryMemoryStore, MemoryBackend, MemoryRecord, MemoryStore, SessionTokenRecord,
};
use crate::memory::supabase_store::SupabaseMemoryStore;

pub struct FallbackMemoryStore {
    stores: Vec<Arc<dyn MemoryStore>>,
}

impl FallbackMemoryStore {
    /// Constructs a new `FallbackMemoryStore` with a list of inner stores.
    pub fn new(stores: Vec<Arc<dyn MemoryStore>>) -> Self {
        Self { stores }
    }

    /// Access the underlying store chain.
    pub fn stores(&self) -> &[Arc<dyn MemoryStore>] {
        &self.stores
    }

    /// Constructs a `FallbackMemoryStore` from environment settings (`XAVIER_MEMORY_FALLBACK`).
    /// Defaults to `"vec,supabase,file"`.
    pub async fn from_env() -> Result<Self> {
        let chain = std::env::var("XAVIER_MEMORY_FALLBACK")
            .unwrap_or_else(|_| "vec,supabase,file".to_string());
        Self::from_chain_str(&chain).await
    }

    /// Parses a comma-separated backend chain string (e.g. `"vec,supabase,file"`) and builds the stores.
    pub async fn from_chain_str(chain: &str) -> Result<Self> {
        let mut stores: Vec<Arc<dyn MemoryStore>> = Vec::new();

        for backend_str in chain.split(',') {
            let backend_str = backend_str.trim();
            if backend_str.is_empty() {
                continue;
            }
            let backend = MemoryBackend::from_env(backend_str);
            match backend {
                MemoryBackend::Vec => match VecSqliteMemoryStore::from_env().await {
                    Ok(store) => stores.push(Arc::new(store)),
                    Err(e) => {
                        tracing::warn!("FallbackMemoryStore: failed to init Vec backend: {}", e)
                    }
                },
                MemoryBackend::Sqlite => match SqliteMemoryStore::from_env().await {
                    Ok(store) => stores.push(Arc::new(store)),
                    Err(e) => {
                        tracing::warn!("FallbackMemoryStore: failed to init Sqlite backend: {}", e)
                    }
                },
                MemoryBackend::Supabase => match SupabaseMemoryStore::from_env().await {
                    Ok(store) => stores.push(Arc::new(store)),
                    Err(e) => tracing::warn!(
                        "FallbackMemoryStore: failed to init Supabase backend: {}",
                        e
                    ),
                },
                MemoryBackend::Postgres => match PostgresMemoryStore::from_env().await {
                    Ok(store) => stores.push(Arc::new(store)),
                    Err(e) => tracing::warn!(
                        "FallbackMemoryStore: failed to init Postgres backend: {}",
                        e
                    ),
                },
                MemoryBackend::File => {
                    let file_path = std::env::var("XAVIER_FILE_STORE_PATH")
                        .unwrap_or_else(|_| "data/file_store.json".to_string());
                    match FileMemoryStore::new(file_path).await {
                        Ok(store) => stores.push(Arc::new(store)),
                        Err(e) => {
                            tracing::warn!("FallbackMemoryStore: failed to init File backend: {}", e)
                        }
                    }
                }
                MemoryBackend::Memory => {
                    stores.push(Arc::new(InMemoryMemoryStore::new()));
                }
                MemoryBackend::Auto | MemoryBackend::Fallback => {
                    tracing::warn!("FallbackMemoryStore: skipping recursive auto/fallback backend in chain");
                }
            }
        }

        if stores.is_empty() {
            tracing::warn!(
                "FallbackMemoryStore: no backends initialized, defaulting to InMemoryMemoryStore"
            );
            stores.push(Arc::new(InMemoryMemoryStore::new()));
        }

        Ok(Self::new(stores))
    }
}

#[async_trait]
impl MemoryStore for FallbackMemoryStore {
    fn backend(&self) -> MemoryBackend {
        MemoryBackend::Fallback
    }

    fn as_any(&self) -> &dyn StdAny {
        self
    }

    async fn health(&self) -> Result<String> {
        let mut statuses = Vec::new();
        for (idx, store) in self.stores.iter().enumerate() {
            match store.health().await {
                Ok(h) => statuses.push(format!("[{}:{}] {}", idx, store.backend().as_str(), h)),
                Err(e) => statuses.push(format!(
                    "[{}:{}] error: {}",
                    idx,
                    store.backend().as_str(),
                    e
                )),
            }
        }
        let op_count = statuses.iter().filter(|s| !s.contains("error:")).count();
        Ok(format!(
            "Fallback chain ({}/{} operational): {}",
            op_count,
            self.stores.len(),
            statuses.join("; ")
        ))
    }

    async fn set_dedup_settings(&self, settings: crate::settings::types::DedupSettings) {
        for store in &self.stores {
            store.set_dedup_settings(settings.clone()).await;
        }
    }

    async fn compact(&self) -> Result<()> {
        for store in &self.stores {
            let _ = store.compact().await;
        }
        Ok(())
    }

    async fn db_size(&self) -> Result<Option<u64>> {
        for store in &self.stores {
            if let Ok(Some(sz)) = store.db_size().await {
                return Ok(Some(sz));
            }
        }
        Ok(None)
    }

    async fn put(&self, record: MemoryRecord) -> Result<()> {
        let mut last_err = None;
        for store in &self.stores {
            match store.put(record.clone()).await {
                Ok(()) => return Ok(()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("FallbackMemoryStore: no stores configured")))
    }

    async fn get(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let mut last_err = None;
        for store in &self.stores {
            match store.get(workspace_id, id_or_path).await {
                Ok(Some(rec)) => return Ok(Some(rec)),
                Ok(None) => continue,
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(None)
        }
    }

    async fn update(&self, record: MemoryRecord) -> Result<()> {
        let mut last_err = None;
        for store in &self.stores {
            match store.update(record.clone()).await {
                Ok(()) => return Ok(()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("FallbackMemoryStore: no stores configured")))
    }

    async fn delete(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let mut last_err = None;
        for store in &self.stores {
            match store.delete(workspace_id, id_or_path).await {
                Ok(Some(rec)) => return Ok(Some(rec)),
                Ok(None) => continue,
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(None)
        }
    }

    async fn list(&self, workspace_id: &str) -> Result<Vec<MemoryRecord>> {
        let mut last_err = None;
        for store in &self.stores {
            match store.list(workspace_id).await {
                Ok(list) if !list.is_empty() => return Ok(list),
                Ok(_) => continue,
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(Vec::new())
        }
    }

    async fn list_filtered(
        &self,
        workspace_id: &str,
        filters: &MemoryQueryFilters,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>> {
        let mut last_err = None;
        for store in &self.stores {
            match store.list_filtered(workspace_id, filters, limit).await {
                Ok(list) if !list.is_empty() => return Ok(list),
                Ok(_) => continue,
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(Vec::new())
        }
    }

    async fn search(
        &self,
        workspace_id: &str,
        query: &str,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryRecord>> {
        let mut last_err = None;
        for store in &self.stores {
            match store.search(workspace_id, query, filters).await {
                Ok(list) if !list.is_empty() => return Ok(list),
                Ok(_) => continue,
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(Vec::new())
        }
    }

    async fn hybrid_search(
        &self,
        workspace_id: &str,
        query: &str,
        mode: HybridSearchMode,
        filters: Option<&MemoryQueryFilters>,
        limit: usize,
    ) -> Result<Vec<HybridSearchResult>> {
        let mut last_err = None;
        for store in &self.stores {
            match store
                .hybrid_search(workspace_id, query, mode, filters, limit)
                .await
            {
                Ok(list) if !list.is_empty() => return Ok(list),
                Ok(_) => continue,
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(Vec::new())
        }
    }

    async fn graph_hops(
        &self,
        workspace_id: &str,
        path_or_id: &str,
        hops: usize,
        query: &str,
    ) -> Result<GraphHopResult> {
        let mut last_err = None;
        for store in &self.stores {
            match store.graph_hops(workspace_id, path_or_id, hops, query).await {
                Ok(res) => return Ok(res),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("FallbackMemoryStore: no stores configured")))
    }

    async fn load_workspace_state(&self, workspace_id: &str) -> Result<DurableWorkspaceState> {
        let mut last_err = None;
        let mut fallback_state = None;
        for store in &self.stores {
            match store.load_workspace_state(workspace_id).await {
                Ok(state) if !state.memories.is_empty() => return Ok(state),
                Ok(state) => {
                    if fallback_state.is_none() {
                        fallback_state = Some(state);
                    }
                }
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(state) = fallback_state {
            Ok(state)
        } else if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(DurableWorkspaceState::default())
        }
    }

    async fn load_workspace_metadata(
        &self,
        workspace_id: &str,
    ) -> Result<(Vec<BeliefEdge>, Vec<SessionTokenRecord>)> {
        let mut last_err = None;
        for store in &self.stores {
            match store.load_workspace_metadata(workspace_id).await {
                Ok((beliefs, tokens)) if !beliefs.is_empty() || !tokens.is_empty() => {
                    return Ok((beliefs, tokens))
                }
                Ok(_) => continue,
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok((Vec::new(), Vec::new()))
        }
    }

    async fn save_beliefs(&self, workspace_id: &str, beliefs: Vec<BeliefEdge>) -> Result<()> {
        let mut last_err = None;
        for store in &self.stores {
            match store.save_beliefs(workspace_id, beliefs.clone()).await {
                Ok(()) => return Ok(()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("FallbackMemoryStore: no stores configured")))
    }

    async fn save_session_token(
        &self,
        workspace_id: &str,
        token: SessionTokenRecord,
    ) -> Result<()> {
        let mut last_err = None;
        for store in &self.stores {
            match store
                .save_session_token(workspace_id, token.clone())
                .await
            {
                Ok(()) => return Ok(()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("FallbackMemoryStore: no stores configured")))
    }

    async fn is_session_token_valid(&self, workspace_id: &str, token: &str) -> Result<bool> {
        let mut last_err = None;
        for store in &self.stores {
            match store.is_session_token_valid(workspace_id, token).await {
                Ok(true) => return Ok(true),
                Ok(false) => continue,
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(false)
        }
    }

    async fn save_checkpoint(&self, workspace_id: &str, checkpoint: Checkpoint) -> Result<()> {
        let mut last_err = None;
        for store in &self.stores {
            match store
                .save_checkpoint(workspace_id, checkpoint.clone())
                .await
            {
                Ok(()) => return Ok(()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("FallbackMemoryStore: no stores configured")))
    }

    async fn load_checkpoint(
        &self,
        workspace_id: &str,
        task_id: &str,
        name: &str,
    ) -> Result<Option<Checkpoint>> {
        let mut last_err = None;
        for store in &self.stores {
            match store.load_checkpoint(workspace_id, task_id, name).await {
                Ok(Some(cp)) => return Ok(Some(cp)),
                Ok(None) => continue,
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(None)
        }
    }

    async fn list_checkpoints(&self, workspace_id: &str, task_id: &str) -> Result<Vec<Checkpoint>> {
        let mut last_err = None;
        for store in &self.stores {
            match store.list_checkpoints(workspace_id, task_id).await {
                Ok(cps) if !cps.is_empty() => return Ok(cps),
                Ok(_) => continue,
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(Vec::new())
        }
    }

    async fn delete_checkpoint(&self, workspace_id: &str, task_id: &str, name: &str) -> Result<()> {
        let mut last_err = None;
        for store in &self.stores {
            match store.delete_checkpoint(workspace_id, task_id, name).await {
                Ok(()) => return Ok(()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("FallbackMemoryStore: no stores configured")))
    }

    async fn list_timeline_events(
        &self,
        workspace_id: &str,
        since: &str,
    ) -> Result<Vec<crate::server::events::RealtimeEvent>> {
        let mut last_err = None;
        for store in &self.stores {
            match store.list_timeline_events(workspace_id, since).await {
                Ok(events) if !events.is_empty() => return Ok(events),
                Ok(_) => continue,
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(Vec::new())
        }
    }

    async fn cleanup_orphans(&self) -> Result<usize> {
        let mut total = 0;
        for store in &self.stores {
            if let Ok(n) = store.cleanup_orphans().await {
                total += n;
            }
        }
        Ok(total)
    }

    async fn list_workspaces(&self) -> Result<Vec<String>> {
        let mut last_err = None;
        for store in &self.stores {
            match store.list_workspaces().await {
                Ok(ws) if !ws.is_empty() => return Ok(ws),
                Ok(_) => continue,
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(Vec::new())
        }
    }

    async fn ls(&self, workspace_id: &str, path: &str) -> Result<Vec<MemoryHierarchyNode>> {
        let mut last_err = None;
        for store in &self.stores {
            match store.ls(workspace_id, path).await {
                Ok(nodes) if !nodes.is_empty() => return Ok(nodes),
                Ok(_) => continue,
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(Vec::new())
        }
    }

    async fn push_to_cloud(&self, workspace_id: &str) -> Result<super::cloud_sync::SyncReport> {
        let mut last_err = None;
        for store in &self.stores {
            match store.push_to_cloud(workspace_id).await {
                Ok(report) => return Ok(report),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("FallbackMemoryStore: no stores configured")))
    }

    async fn pull_from_cloud(&self, workspace_id: &str) -> Result<super::cloud_sync::SyncReport> {
        let mut last_err = None;
        for store in &self.stores {
            match store.pull_from_cloud(workspace_id).await {
                Ok(report) => return Ok(report),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("FallbackMemoryStore: no stores configured")))
    }

    async fn sync_all(&self, workspace_id: &str) -> Result<super::cloud_sync::SyncReport> {
        let mut last_err = None;
        for store in &self.stores {
            match store.sync_all(workspace_id).await {
                Ok(report) => return Ok(report),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("FallbackMemoryStore: no stores configured")))
    }

    async fn load_entity_graph_snapshot(&self, workspace_id: &str) -> Result<Option<String>> {
        let mut last_err = None;
        for store in &self.stores {
            match store.load_entity_graph_snapshot(workspace_id).await {
                Ok(Some(snap)) => return Ok(Some(snap)),
                Ok(None) => continue,
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(None)
        }
    }

    async fn save_entity_graph_snapshot(&self, workspace_id: &str, data: &str) -> Result<()> {
        let mut last_err = None;
        for store in &self.stores {
            match store
                .save_entity_graph_snapshot(workspace_id, data)
                .await
            {
                Ok(()) => return Ok(()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("FallbackMemoryStore: no stores configured")))
    }

    async fn symbols_for_memory(&self, memory_id: &str) -> Result<Vec<String>> {
        let mut last_err = None;
        for store in &self.stores {
            match store.symbols_for_memory(memory_id).await {
                Ok(syms) if !syms.is_empty() => return Ok(syms),
                Ok(_) => continue,
                Err(e) => last_err = Some(e),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ErrorStore;

    #[async_trait]
    impl MemoryStore for ErrorStore {
        fn backend(&self) -> MemoryBackend {
            MemoryBackend::Memory
        }
        fn as_any(&self) -> &dyn StdAny {
            self
        }
        async fn health(&self) -> Result<String> {
            anyhow::bail!("backend offline")
        }
        async fn put(&self, _record: MemoryRecord) -> Result<()> {
            anyhow::bail!("store unavailable")
        }
        async fn get(&self, _workspace_id: &str, _id_or_path: &str) -> Result<Option<MemoryRecord>> {
            anyhow::bail!("store unavailable")
        }
        async fn update(&self, _record: MemoryRecord) -> Result<()> {
            anyhow::bail!("store unavailable")
        }
        async fn delete(
            &self,
            _workspace_id: &str,
            _id_or_path: &str,
        ) -> Result<Option<MemoryRecord>> {
            anyhow::bail!("store unavailable")
        }
        async fn list(&self, _workspace_id: &str) -> Result<Vec<MemoryRecord>> {
            anyhow::bail!("store unavailable")
        }
        async fn search(
            &self,
            _workspace_id: &str,
            _query: &str,
            _filters: Option<&MemoryQueryFilters>,
        ) -> Result<Vec<MemoryRecord>> {
            anyhow::bail!("store unavailable")
        }
        async fn load_workspace_state(&self, _workspace_id: &str) -> Result<DurableWorkspaceState> {
            anyhow::bail!("store unavailable")
        }
        async fn save_beliefs(&self, _workspace_id: &str, _beliefs: Vec<BeliefEdge>) -> Result<()> {
            anyhow::bail!("store unavailable")
        }
        async fn save_session_token(
            &self,
            _workspace_id: &str,
            _token: SessionTokenRecord,
        ) -> Result<()> {
            anyhow::bail!("store unavailable")
        }
        async fn is_session_token_valid(
            &self,
            _workspace_id: &str,
            _token: &str,
        ) -> Result<bool> {
            anyhow::bail!("store unavailable")
        }
        async fn save_checkpoint(&self, _workspace_id: &str, _checkpoint: Checkpoint) -> Result<()> {
            anyhow::bail!("store unavailable")
        }
        async fn load_checkpoint(
            &self,
            _workspace_id: &str,
            _task_id: &str,
            _name: &str,
        ) -> Result<Option<Checkpoint>> {
            anyhow::bail!("store unavailable")
        }
        async fn list_checkpoints(&self, _workspace_id: &str, _task_id: &str) -> Result<Vec<Checkpoint>> {
            anyhow::bail!("store unavailable")
        }
        async fn delete_checkpoint(
            &self,
            _workspace_id: &str,
            _task_id: &str,
            _name: &str,
        ) -> Result<()> {
            anyhow::bail!("store unavailable")
        }
    }

    #[tokio::test]
    async fn test_fallback_memory() {
        let store1: Arc<dyn MemoryStore> = Arc::new(ErrorStore);
        let store2: Arc<dyn MemoryStore> = Arc::new(InMemoryMemoryStore::new());

        let fallback_store = FallbackMemoryStore::new(vec![store1, store2.clone()]);
        assert_eq!(fallback_store.backend(), MemoryBackend::Fallback);
        assert_eq!(fallback_store.stores().len(), 2);

        // Test put: primary store (ErrorStore) fails, falls back to store2
        let record = MemoryRecord {
            id: "mem_123".to_string(),
            workspace_id: "default".to_string(),
            path: "docs/test.md".to_string(),
            content: "Hello fallback memory".to_string(),
            ..Default::default()
        };

        fallback_store
            .put(record.clone())
            .await
            .expect("put should succeed via fallback store");

        // Verify record was inserted into store2
        let found = store2
            .get("default", "mem_123")
            .await
            .expect("store2 should return inserted record")
            .expect("record should be present in store2");
        assert_eq!(found.content, "Hello fallback memory");

        // Test get: primary store fails, falls back to store2
        let fetched = fallback_store
            .get("default", "mem_123")
            .await
            .expect("get should succeed")
            .expect("record should be found via fallback get");
        assert_eq!(fetched.id, "mem_123");

        // Test search: primary store fails, falls back to store2
        let search_results = fallback_store
            .search("default", "Hello", None)
            .await
            .expect("search should succeed");
        assert_eq!(search_results.len(), 1);
        assert_eq!(search_results[0].id, "mem_123");

        // Test list: primary store fails, falls back to store2
        let list_results = fallback_store
            .list("default")
            .await
            .expect("list should succeed");
        assert_eq!(list_results.len(), 1);

        // Test health: reports operational count and details
        let health_str = fallback_store
            .health()
            .await
            .expect("health check should succeed");
        assert!(health_str.contains("Fallback chain (1/2 operational)"));
    }

    #[tokio::test]
    async fn test_fallback_from_chain_str() {
        let fallback = FallbackMemoryStore::from_chain_str("memory,memory")
            .await
            .expect("should build fallback store from chain str");
        assert_eq!(fallback.stores().len(), 2);
    }
}
