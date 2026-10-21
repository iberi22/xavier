//! Core memory store trait and shared types for Xavier.
//!
//! Defines the MemoryStore trait and all shared data structures
//! used by concrete store implementations (SqliteMemoryStore in sqlite_store.rs,
//! VecSqliteMemoryStore in sqlite_vec_store.rs, etc.).
//! Includes references to semantic deduplication (dedup/deduplicate) using
//! cosine similarity or vector_distance metrics during insertion.

use std::{any::Any as StdAny, collections::HashMap, fmt, path::PathBuf, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::{fs, sync::RwLock};

use crate::checkpoint::Checkpoint;
use crate::domain::memory::belief::BeliefEdge;
use crate::memory::access::AccessStats;
use crate::memory::hierarchy::{MemoryHierarchyNode, MemoryTree};
use crate::memory::qmd_memory::MemoryDocument;
use crate::memory::schema::{resolve_metadata, MemoryLevel, MemoryQueryFilters};

// ---------------------------------------------------------------------------
// Backend enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBackend {
    Auto,
    File,
    Memory,
    Sqlite,
    Vec, // SQLite + sqlite-vec vector search
    Supabase,
    Postgres,
    Fallback,
}

impl MemoryBackend {
    /// From env.
    pub fn from_env(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Self::Auto,
            "memory" => Self::Memory,
            "sqlite" => Self::Sqlite,
            "vec" | "sqlite-vec" => Self::Vec,
            "supabase" => Self::Supabase,
            "postgres" | "neon" => Self::Postgres,
            "file" => Self::File,
            "fallback" | "chain" => Self::Fallback,
            _ => Self::File,
        }
    }

    /// As str.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::File => "file",
            Self::Memory => "memory",
            Self::Sqlite => "sqlite",
            Self::Vec => "vec",
            Self::Supabase => "supabase",
            Self::Postgres => "postgres",
            Self::Fallback => "fallback",
        }
    }
}

// ---------------------------------------------------------------------------
// Core data types
// ---------------------------------------------------------------------------

pub use crate::domain::cycle_breaks::w30_12::{stable_key, MemoryRecord, MemoryRevision};

impl Default for MemoryRecord {
    fn default() -> Self {
        Self {
            id: String::new(),
            workspace_id: String::new(),
            path: String::new(),
            content: String::new(),
            metadata: serde_json::Value::Null,
            embedding: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            revision: 0,
            primary: false,
            parent_id: None,
            cluster_id: None,
            level: MemoryLevel::Raw,
            relation: None,
            clearance: crate::security::clearance::ClearanceLevel::Unclassified,
            revisions: Vec::new(),
            encrypted_dek: None,
            content_iv: None,
            metadata_iv: None,
            score: 0.0,
            deleted_at: None,
            embedding_status: "pending".to_string(),
            embedding_attempts: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HybridSearchMode {
    Text,
    Vector,
    #[default]
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchResult {
    pub record: MemoryRecord,
    pub score: f32,
    #[serde(default)]
    pub vector_score: f32,
    #[serde(default)]
    pub lexical_score: f32,
    #[serde(default)]
    pub kg_score: f32,
    #[serde(default)]
    pub bm25: Option<f32>,
    #[serde(default)]
    pub vector_similarity: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct FederatedLegs {
    pub store: String,
    pub model_ok: bool,
    pub vector: Vec<HybridSearchResult>,
    pub lexical: Vec<HybridSearchResult>,
    pub kg: Vec<HybridSearchResult>,
}

#[derive(Debug, Clone)]
pub struct FederatedHit {
    pub store: String,
    pub record_id: String,
    pub score: f32,
    pub sources: Vec<String>,
    pub skipped_model_mismatch: bool,
}

pub fn merge_federated(legs: &[FederatedLegs], k: usize, limit: usize) -> Vec<FederatedHit> {
    use std::collections::HashMap;

    // We will accumulate scores keyed by (store, record_id)
    struct HitAccumulator {
        store: String,
        record_id: String,
        score: f32,
        sources: Vec<String>,
        skipped_model_mismatch: bool,
    }

    let mut accumulators: HashMap<(String, String), HitAccumulator> = HashMap::new();

    // 1. Global vector list across all stores where model_ok is true
    let mut global_vector_legs: Vec<(&String, &HybridSearchResult)> = Vec::new();
    for leg in legs {
        if leg.model_ok {
            for res in &leg.vector {
                global_vector_legs.push((&leg.store, res));
            }
        }
    }

    // Sort global vectors by raw vector_similarity descending, then store, then record.id
    global_vector_legs.sort_by(|a, b| {
        let sim_a = a.1.vector_similarity.unwrap_or(0.0);
        let sim_b = b.1.vector_similarity.unwrap_or(0.0);
        sim_b
            .partial_cmp(&sim_a)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(b.0))
            .then_with(|| a.1.record.id.cmp(&b.1.record.id))
    });

    // Apply RRF to global vectors
    for (rank, (store, res)) in global_vector_legs.into_iter().enumerate() {
        let rrf_score = 1.0 / (k as f32 + (rank + 1) as f32);
        let key = (store.clone(), res.record.id.clone());
        let acc = accumulators.entry(key).or_insert_with(|| HitAccumulator {
            store: store.clone(),
            record_id: res.record.id.clone(),
            score: 0.0,
            sources: Vec::new(),
            skipped_model_mismatch: false,
        });
        acc.score += rrf_score;
        if !acc.sources.contains(&"vector".to_string()) {
            acc.sources.push("vector".to_string());
        }
    }

    // 2. Per-store lexical and KG lists
    for leg in legs {
        // Lexical
        for (rank, res) in leg.lexical.iter().enumerate() {
            let rrf_score = 1.0 / (k as f32 + (rank + 1) as f32);
            let key = (leg.store.clone(), res.record.id.clone());
            let acc = accumulators.entry(key).or_insert_with(|| HitAccumulator {
                store: leg.store.clone(),
                record_id: res.record.id.clone(),
                score: 0.0,
                sources: Vec::new(),
                skipped_model_mismatch: !leg.model_ok,
            });
            acc.score += rrf_score;
            if !acc.sources.contains(&"lexical".to_string()) {
                acc.sources.push("lexical".to_string());
            }
        }

        // KG
        for (rank, res) in leg.kg.iter().enumerate() {
            let rrf_score = 1.0 / (k as f32 + (rank + 1) as f32);
            let key = (leg.store.clone(), res.record.id.clone());
            let acc = accumulators.entry(key).or_insert_with(|| HitAccumulator {
                store: leg.store.clone(),
                record_id: res.record.id.clone(),
                score: 0.0,
                sources: Vec::new(),
                skipped_model_mismatch: !leg.model_ok,
            });
            acc.score += rrf_score;
            if !acc.sources.contains(&"kg".to_string()) {
                acc.sources.push("kg".to_string());
            }
        }
    }

    // Prepare final output
    let mut hits: Vec<FederatedHit> = accumulators
        .into_values()
        .map(|acc| {
            // Ensure sources is deterministic
            let mut sources = acc.sources;
            sources.sort();
            FederatedHit {
                store: acc.store,
                record_id: acc.record_id,
                score: acc.score,
                sources,
                skipped_model_mismatch: acc.skipped_model_mismatch,
            }
        })
        .collect();

    // Fix model mismatch flags for any record that might have been initialized before we knew model_mismatch for that store. Actually, we can just look it up.
    let mismatch_map: HashMap<&str, bool> = legs
        .iter()
        .map(|l| (l.store.as_str(), !l.model_ok))
        .collect();
    for hit in &mut hits {
        hit.skipped_model_mismatch = *mismatch_map.get(hit.store.as_str()).unwrap_or(&false);
    }

    // Sort by score desc, then store, then record_id
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.store.cmp(&b.store))
            .then_with(|| a.record_id.cmp(&b.record_id))
    });

    hits.truncate(limit);
    hits
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphHopPath {
    pub entity_id: String,
    pub entity_name: String,
    pub depth: usize,
    pub entity_path: String,
    pub relation_path: String,
    #[serde(default)]
    pub memory_hits: Vec<MemoryRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphHopResult {
    pub source: MemoryRecord,
    pub hops: usize,
    pub query: String,
    pub paths: Vec<GraphHopPath>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SessionTokenRecord {
    pub token: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl fmt::Debug for SessionTokenRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionTokenRecord")
            .field("token", &"[REDACTED]")
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DurableWorkspaceState {
    #[serde(default)]
    pub memories: Vec<MemoryRecord>,
    #[serde(default)]
    pub beliefs: Vec<BeliefEdge>,
    #[serde(default)]
    pub session_tokens: Vec<SessionTokenRecord>,
    #[serde(default)]
    pub checkpoints: Vec<Checkpoint>,
    #[serde(default)]
    pub entity_graph_snapshot: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct DurableStoreFile {
    #[serde(default)]
    pub workspaces: HashMap<String, DurableWorkspaceState>,
}

// ---------------------------------------------------------------------------
// MemoryRecord helpers
// ---------------------------------------------------------------------------

impl MemoryRecord {
    /// From document.
    pub fn from_document(
        workspace_id: &str,
        document: &MemoryDocument,
        primary: bool,
        parent_id: Option<String>,
    ) -> Self {
        let now = Utc::now();
        let revision = document
            .metadata
            .get("revision")
            .and_then(|value| value.as_u64())
            .unwrap_or(1);
        let created_at = document
            .metadata
            .get("created_at")
            .and_then(|value| value.as_str())
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc))
            .unwrap_or(now);
        let updated_at = document
            .metadata
            .get("updated_at")
            .and_then(|value| value.as_str())
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc))
            .unwrap_or(now);

        let id = document
            .id
            .clone()
            .unwrap_or_else(|| stable_key("memory", &[workspace_id, &document.path]));

        let mut metadata = document.metadata.clone();
        if document.score > 0.0 {
            if let Some(obj) = metadata.as_object_mut() {
                obj.insert("score".to_string(), serde_json::json!(document.score));
            }
        }

        Self {
            id,
            workspace_id: workspace_id.to_string(),
            path: document.path.clone(),
            content: document.content.clone(),
            metadata,
            embedding: document
                .content_vector
                .clone()
                .unwrap_or_else(|| document.embedding.clone()),
            created_at,
            updated_at,
            revision,
            primary,
            parent_id,
            cluster_id: document.cluster_id.clone(),
            level: document.level,
            relation: document.relation.clone(),
            clearance: document.clearance,
            encrypted_dek: document.metadata.get("encrypted_dek").and_then(|v| {
                v.as_str()
                    .and_then(|s| crate::utils::crypto::hex_decode(s).ok())
            }),
            content_iv: document.metadata.get("content_iv").and_then(|v| {
                v.as_str()
                    .and_then(|s| crate::utils::crypto::hex_decode(s).ok())
            }),
            metadata_iv: document.metadata.get("metadata_iv").and_then(|v| {
                v.as_str()
                    .and_then(|s| crate::utils::crypto::hex_decode(s).ok())
            }),
            deleted_at: None,
            score: document.score,
            embedding_status: "pending".to_string(),
            embedding_attempts: 0,
            revisions: vec![MemoryRevision {
                revision,
                recorded_at: updated_at,
                path: document.path.clone(),
                content: document.content.clone(),
                metadata: document.metadata.clone(),
            }],
        }
    }

    /// To document.
    pub fn to_document(&self) -> MemoryDocument {
        let mut metadata = self.metadata.clone();
        if let Some(object) = metadata.as_object_mut() {
            object.insert("revision".to_string(), serde_json::json!(self.revision));
            object.insert(
                "created_at".to_string(),
                serde_json::json!(self.created_at.to_rfc3339()),
            );
            object.insert(
                "updated_at".to_string(),
                serde_json::json!(self.updated_at.to_rfc3339()),
            );
            if self.score > 0.0 {
                object.insert("score".to_string(), serde_json::json!(self.score));
            }
            object.insert("primary".to_string(), serde_json::json!(self.primary));
            if let Some(parent_id) = &self.parent_id {
                object.insert("parent_id".to_string(), serde_json::json!(parent_id));
            }
        }

        if let Some(object) = metadata.as_object_mut() {
            if let Some(encrypted_dek) = &self.encrypted_dek {
                object.insert(
                    "encrypted_dek".to_string(),
                    serde_json::json!(crate::utils::crypto::hex_encode(encrypted_dek)),
                );
            }
            if let Some(content_iv) = &self.content_iv {
                object.insert(
                    "content_iv".to_string(),
                    serde_json::json!(crate::utils::crypto::hex_encode(content_iv)),
                );
            }
            if let Some(metadata_iv) = &self.metadata_iv {
                object.insert(
                    "metadata_iv".to_string(),
                    serde_json::json!(crate::utils::crypto::hex_encode(metadata_iv)),
                );
            }
        }

        MemoryDocument {
            id: Some(self.id.clone()),
            path: self.path.clone(),
            content: self.content.clone(),
            metadata,
            content_vector: Some(self.embedding.clone()),
            embedding: self.embedding.clone(),
            cluster_id: self.cluster_id.clone(),
            parent_id: self.parent_id.clone(),
            level: self.level,
            relation: self.relation.clone(),
            clearance: self.clearance,
            minhash: None,
            score: 0.0,
            ..Default::default()
        }
    }

    /// Matches query.
    pub fn matches_query(&self, query: &str) -> bool {
        let lowered = query.trim().to_ascii_lowercase();
        lowered.is_empty()
            || self.path.to_ascii_lowercase().contains(&lowered)
            || self.content.to_ascii_lowercase().contains(&lowered)
            || self
                .metadata
                .to_string()
                .to_ascii_lowercase()
                .contains(&lowered)
    }

    /// Check if the record is authorized for the local embeddings pipeline.
    ///
    /// Authorized records must:
    /// 1. Have user consent.
    /// 2. Not be marked as private or revoked in metadata.
    /// 3. Not exceed the maximum allowed clearance depth.
    pub fn is_authorized_for_embedding(
        &self,
        consent_given: bool,
        max_clearance: crate::security::clearance::ClearanceLevel,
    ) -> bool {
        if !consent_given {
            return false;
        }

        // Refuse revoked records
        if self
            .metadata
            .get("revoked")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return false;
        }

        // Refuse private records
        let is_private = self
            .metadata
            .get("is_private")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let visibility = self
            .metadata
            .get("visibility")
            .and_then(|v| v.as_str())
            .unwrap_or("public");

        if is_private || visibility == "private" {
            return false;
        }

        // Refuse over-depth records
        if self.clearance > max_clearance {
            return false;
        }

        true
    }

    /// Returns the last accessed time, falling back to `updated_at`.
    pub fn last_accessed(&self) -> DateTime<Utc> {
        if let Some(last_accessed_val) = self
            .metadata
            .get("last_accessed_at")
            .and_then(|v| v.as_str())
        {
            if let Ok(parsed) = DateTime::parse_from_rfc3339(last_accessed_val) {
                return parsed.with_timezone(&Utc);
            }
        }
        self.updated_at
    }

    /// Update/touch the last accessed time in metadata.
    pub fn touch(&mut self, now: DateTime<Utc>) {
        if let serde_json::Value::Object(ref mut map) = self.metadata {
            map.insert(
                "last_accessed_at".to_string(),
                serde_json::json!(now.to_rfc3339()),
            );
        } else {
            let mut map = serde_json::Map::new();
            map.insert(
                "last_accessed_at".to_string(),
                serde_json::json!(now.to_rfc3339()),
            );
            self.metadata = serde_json::Value::Object(map);
        }
    }
}

// ---------------------------------------------------------------------------
// MemoryStore trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait MemoryStore: crate::domain::cycle_breaks::w30_12::MemoryConsumerStore {
    fn backend(&self) -> MemoryBackend;
    fn as_any(&self) -> &dyn StdAny;
    async fn health(&self) -> Result<String>;
    #[allow(
        clippy::unused_async,
        reason = "MemoryStore trait requirement for async backends"
    )]
    async fn set_dedup_settings(&self, _settings: crate::settings::types::DedupSettings) {}
    #[allow(
        clippy::unused_async,
        reason = "MemoryStore trait requirement for async backends"
    )]
    async fn compact(&self) -> Result<()> {
        Ok(())
    }
    #[allow(
        clippy::unused_async,
        reason = "MemoryStore trait requirement for async backends"
    )]
    async fn db_size(&self) -> Result<Option<u64>> {
        Ok(None)
    }
    async fn put(&self, record: MemoryRecord) -> Result<()>;
    /// Persist a caller-supplied vector, including an intentionally empty one.
    ///
    /// The default delegates to `put`, which for every store that does not
    /// generate embeddings itself (file, in-memory, sqlite, postgres, supabase)
    /// already stores the record as given. A store whose `put` embeds inline
    /// must override this so the empty vector is kept.
    async fn put_without_embedding(&self, record: MemoryRecord) -> Result<()> {
        self.put(record).await
    }
    /// Attach a computed vector to an existing record without treating it as a
    /// content change. Backends that can should override this to leave the
    /// revision, history and `updated_at` untouched and emit no update event;
    /// the default falls back to a full `update`.
    async fn set_embedding(
        &self,
        workspace_id: &str,
        id_or_path: &str,
        embedding: Vec<f32>,
    ) -> Result<()> {
        let Some(mut record) = self.get(workspace_id, id_or_path).await? else {
            anyhow::bail!("set_embedding: memory not found: {id_or_path}");
        };
        record.embedding = embedding;
        self.update(record).await
    }
    async fn get(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>>;
    async fn update(&self, record: MemoryRecord) -> Result<()>;
    async fn delete(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>>;
    async fn list(&self, workspace_id: &str) -> Result<Vec<MemoryRecord>>;
    /// Count resolved projects without requiring record payloads when supported.
    async fn project_counts(
        &self,
        workspace_id: &str,
    ) -> Result<std::collections::BTreeMap<String, usize>> {
        let mut projects = std::collections::BTreeMap::new();
        for record in self.list(workspace_id).await? {
            if let Ok(resolved) =
                resolve_metadata(&record.path, &record.metadata, workspace_id, None)
            {
                if let Some(project) = resolved.namespace.project {
                    *projects.entry(project).or_insert(0) += 1;
                }
            }
        }
        Ok(projects)
    }
    async fn list_filtered(
        &self,
        workspace_id: &str,
        filters: &MemoryQueryFilters,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>> {
        let all = self.list(workspace_id).await?;
        let mut filtered = filter_records(all, workspace_id, "", Some(filters))?;
        filtered.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        Ok(filtered.into_iter().take(limit).collect())
    }
    async fn search(
        &self,
        workspace_id: &str,
        query: &str,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryRecord>>;
    async fn hybrid_search(
        &self,
        workspace_id: &str,
        query: &str,
        mode: HybridSearchMode,
        filters: Option<&MemoryQueryFilters>,
        limit: usize,
    ) -> Result<Vec<HybridSearchResult>> {
        let _ = (workspace_id, query, mode, filters, limit);
        anyhow::bail!(
            "hybrid search is not supported by the {} backend",
            self.backend().as_str()
        )
    }
    async fn graph_hops(
        &self,
        workspace_id: &str,
        path_or_id: &str,
        hops: usize,
        query: &str,
    ) -> Result<GraphHopResult> {
        let _ = (workspace_id, path_or_id, hops, query);
        anyhow::bail!(
            "graph hop traversal is not supported by the {} backend",
            self.backend().as_str()
        )
    }
    async fn load_workspace_state(&self, workspace_id: &str) -> Result<DurableWorkspaceState>;
    /// Load only workspace metadata (beliefs, session tokens) without memories.
    /// This is used for lazy loading: avoids loading all memory records into RAM at startup.
    async fn load_workspace_metadata(
        &self,
        workspace_id: &str,
    ) -> Result<(
        Vec<crate::domain::memory::belief::BeliefEdge>,
        Vec<SessionTokenRecord>,
    )> {
        let state = self.load_workspace_state(workspace_id).await?;
        Ok((state.beliefs, state.session_tokens))
    }
    async fn save_beliefs(&self, workspace_id: &str, beliefs: Vec<BeliefEdge>) -> Result<()>;
    async fn save_session_token(&self, workspace_id: &str, token: SessionTokenRecord)
        -> Result<()>;
    async fn is_session_token_valid(&self, workspace_id: &str, token: &str) -> Result<bool>;
    async fn save_checkpoint(&self, workspace_id: &str, checkpoint: Checkpoint) -> Result<()>;
    async fn load_checkpoint(
        &self,
        workspace_id: &str,
        task_id: &str,
        name: &str,
    ) -> Result<Option<Checkpoint>>;
    async fn list_checkpoints(&self, workspace_id: &str, task_id: &str) -> Result<Vec<Checkpoint>>;
    async fn delete_checkpoint(&self, workspace_id: &str, task_id: &str, name: &str) -> Result<()>;
    /// List timeline events for a workspace since the given ISO 8601 timestamp.
    async fn list_timeline_events(
        &self,
        workspace_id: &str,
        since: &str,
    ) -> Result<Vec<crate::domain::cycle_breaks::w30_07::RealtimeEvent>> {
        let _ = (workspace_id, since);
        anyhow::bail!(
            "timeline events are not supported by the {} backend",
            self.backend().as_str()
        )
    }

    /// Clean up orphaned vectors or internal resources. Returns the number of cleaned items.
    #[allow(
        clippy::unused_async,
        reason = "MemoryStore trait requirement for async backends"
    )]
    async fn cleanup_orphans(&self) -> Result<usize> {
        Ok(0)
    }

    /// List all workspace IDs that contain at least one record.
    ///
    /// Used by the sync manifest builder to discover every workspace
    /// without guessing.  Backends that cannot enumerate workspaces
    /// return an empty list (the manifest will simply be empty).
    #[allow(
        clippy::unused_async,
        reason = "MemoryStore trait requirement for async backends"
    )]
    async fn list_workspaces(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// List memories and virtual directories at a given path.
    async fn ls(&self, workspace_id: &str, path: &str) -> Result<Vec<MemoryHierarchyNode>> {
        let all = self.list(workspace_id).await?;
        Ok(MemoryTree::build_ls(all, path))
    }

    // -----------------------------------------------------------------------
    // Cloud sync methods (default implementations bail with "not supported")
    // -----------------------------------------------------------------------

    /// Push records newer than last sync to the cloud backend.
    ///
    /// Only meaningful when this store IS the local store and has a paired
    /// cloud store. The default implementation returns Ok with zero counts.
    async fn push_to_cloud(&self, _workspace_id: &str) -> Result<super::cloud_sync::SyncReport> {
        anyhow::bail!(
            "push_to_cloud is not supported by the {} backend",
            self.backend().as_str()
        )
    }

    /// Pull records newer than last sync from the cloud backend.
    async fn pull_from_cloud(&self, _workspace_id: &str) -> Result<super::cloud_sync::SyncReport> {
        anyhow::bail!(
            "pull_from_cloud is not supported by the {} backend",
            self.backend().as_str()
        )
    }

    /// Full bidirectional sync (pull → merge → push).
    async fn sync_all(&self, _workspace_id: &str) -> Result<super::cloud_sync::SyncReport> {
        anyhow::bail!(
            "sync_all is not supported by the {} backend",
            self.backend().as_str()
        )
    }

    #[allow(
        clippy::unused_async,
        reason = "MemoryStore trait requirement for async backends"
    )]
    async fn load_entity_graph_snapshot(&self, _workspace_id: &str) -> Result<Option<String>> {
        Ok(None)
    }

    #[allow(
        clippy::unused_async,
        reason = "MemoryStore trait requirement for async backends"
    )]
    async fn save_entity_graph_snapshot(&self, _workspace_id: &str, _data: &str) -> Result<()> {
        Ok(())
    }

    /// Return code symbols linked to a given memory_id.
    #[allow(
        clippy::unused_async,
        reason = "MemoryStore trait requirement for async backends"
    )]
    async fn symbols_for_memory(&self, memory_id: &str) -> Result<Vec<String>> {
        let _ = memory_id;
        Ok(Vec::new())
    }

    // -----------------------------------------------------------------------
    // Access instrumentation (Phase 1 of utility-prune groundwork)
    //
    // These exist so `TgdUtilityPruner` and `DecayManager` can answer "was this
    // ever needed?" from evidence instead of from the empty `metadata` blob.
    // Default implementations are inert: a backend that does not persist
    // accesses simply reports no observations, which keeps "never accessed"
    // and "never observable" distinguishable instead of silently equal.
    // -----------------------------------------------------------------------

    /// Create the access tables and seed the observation clock.
    #[allow(
        clippy::unused_async,
        reason = "MemoryStore trait requirement for async backends"
    )]
    async fn ensure_access_instrumentation(&self) -> Result<()> {
        Ok(())
    }

    /// Persist buffered accesses as one upsert batch.
    ///
    /// `entries` is `(memory_id, delta)`: a batch, not one row per read.
    #[allow(
        clippy::unused_async,
        reason = "MemoryStore trait requirement for async backends"
    )]
    async fn record_accesses(
        &self,
        _workspace_id: &str,
        _entries: &[(String, u64)],
    ) -> Result<usize> {
        Ok(0)
    }

    /// Load every persisted access statistic for a workspace.
    #[allow(
        clippy::unused_async,
        reason = "MemoryStore trait requirement for async backends"
    )]
    async fn load_access_stats(&self, _workspace_id: &str) -> Result<Vec<(String, AccessStats)>> {
        Ok(Vec::new())
    }

    /// Read one key from `maintenance_meta`.
    #[allow(
        clippy::unused_async,
        reason = "MemoryStore trait requirement for async backends"
    )]
    async fn read_maintenance_meta(
        &self,
        _key: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// FileMemoryStore
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct FileMemoryStore {
    path: PathBuf,
    state: Arc<RwLock<DurableStoreFile>>,
}

impl FileMemoryStore {
    /// New.
    pub async fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let state = if fs::try_exists(&path).await.unwrap_or(false) {
            let payload = fs::read_to_string(&path).await?;
            serde_json::from_str(&payload).unwrap_or_default()
        } else {
            DurableStoreFile::default()
        };

        Ok(Self {
            path,
            state: Arc::new(RwLock::new(state)),
        })
    }

    /// Persist.
    pub async fn persist(&self) -> Result<()> {
        let payload = {
            let state = self.state.read().await;
            serde_json::to_vec_pretty(&*state)?
        };
        fs::write(&self.path, payload).await?;
        Ok(())
    }

    fn workspace_mut<'a>(
        state: &'a mut DurableStoreFile,
        workspace_id: &str,
    ) -> &'a mut DurableWorkspaceState {
        state
            .workspaces
            .entry(workspace_id.to_string())
            .or_default()
    }
}

#[async_trait]
impl MemoryStore for FileMemoryStore {
    fn backend(&self) -> MemoryBackend {
        MemoryBackend::File
    }

    fn as_any(&self) -> &dyn StdAny {
        self
    }

    async fn health(&self) -> Result<String> {
        Ok(format!("file store at {}", self.path.display()))
    }

    async fn put(&self, record: MemoryRecord) -> Result<()> {
        let workspace_id = record.workspace_id.clone();
        {
            let mut state = self.state.write().await;
            let workspace = Self::workspace_mut(&mut state, &workspace_id);
            workspace.memories.retain(|item| item.id != record.id);
            workspace.memories.push(record);
        }
        self.persist().await
    }

    async fn get(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let state = self.state.read().await;
        Ok(state
            .workspaces
            .get(workspace_id)
            .and_then(|workspace| {
                workspace
                    .memories
                    .iter()
                    .find(|item| item.id == id_or_path || item.path == id_or_path)
            })
            .cloned())
    }

    async fn update(&self, record: MemoryRecord) -> Result<()> {
        let workspace_id = record.workspace_id.clone();
        {
            let mut state = self.state.write().await;
            let workspace = Self::workspace_mut(&mut state, &workspace_id);
            if let Some(existing) = workspace
                .memories
                .iter_mut()
                .find(|item| item.id == record.id || item.path == record.path)
            {
                *existing = revisioned_record(existing.clone(), record);
            } else {
                workspace.memories.push(record);
            }
        }
        self.persist().await
    }

    async fn delete(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let removed = {
            let mut state = self.state.write().await;
            let workspace = Self::workspace_mut(&mut state, workspace_id);
            let removed = workspace
                .memories
                .iter()
                .position(|item| item.id == id_or_path || item.path == id_or_path)
                .map(|index| workspace.memories.remove(index));

            if let Some(removed_record) = removed.as_ref() {
                workspace.memories.retain(|item| {
                    item.parent_id.as_deref() != Some(&removed_record.id)
                        && item.parent_id.as_deref() != Some(&removed_record.path)
                });
            }

            removed
        };

        if removed.is_some() {
            self.persist().await?;
        }

        Ok(removed)
    }

    async fn list(&self, workspace_id: &str) -> Result<Vec<MemoryRecord>> {
        let state = self.state.read().await;
        Ok(state
            .workspaces
            .get(workspace_id)
            .map(|workspace| workspace.memories.clone())
            .unwrap_or_default())
    }

    async fn search(
        &self,
        workspace_id: &str,
        query: &str,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryRecord>> {
        filter_records(self.list(workspace_id).await?, workspace_id, query, filters)
    }

    async fn load_workspace_state(&self, workspace_id: &str) -> Result<DurableWorkspaceState> {
        let state = self.state.read().await;
        Ok(state
            .workspaces
            .get(workspace_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn save_beliefs(&self, workspace_id: &str, beliefs: Vec<BeliefEdge>) -> Result<()> {
        {
            let mut state = self.state.write().await;
            let workspace = Self::workspace_mut(&mut state, workspace_id);
            workspace.beliefs = beliefs;
        }
        self.persist().await
    }

    async fn save_session_token(
        &self,
        workspace_id: &str,
        token: SessionTokenRecord,
    ) -> Result<()> {
        {
            let mut state = self.state.write().await;
            let workspace = Self::workspace_mut(&mut state, workspace_id);
            workspace
                .session_tokens
                .retain(|item| item.expires_at > Utc::now() && item.token != token.token);
            workspace.session_tokens.push(token);
        }
        self.persist().await
    }

    async fn is_session_token_valid(&self, workspace_id: &str, token: &str) -> Result<bool> {
        let state = self.state.read().await;
        Ok(state
            .workspaces
            .get(workspace_id)
            .map(|workspace| {
                workspace.session_tokens.iter().any(|item| {
                    xavier::server::http::api::constant_time_compare(&item.token, token)
                        && item.expires_at > Utc::now()
                })
            })
            .unwrap_or(false))
    }

    async fn save_checkpoint(&self, workspace_id: &str, checkpoint: Checkpoint) -> Result<()> {
        {
            let mut state = self.state.write().await;
            let workspace = Self::workspace_mut(&mut state, workspace_id);
            workspace.checkpoints.retain(|item| {
                !(item.task_id == checkpoint.task_id && item.name == checkpoint.name)
            });
            workspace.checkpoints.push(checkpoint);
        }
        self.persist().await
    }

    async fn load_checkpoint(
        &self,
        workspace_id: &str,
        task_id: &str,
        name: &str,
    ) -> Result<Option<Checkpoint>> {
        let state = self.state.read().await;
        Ok(state
            .workspaces
            .get(workspace_id)
            .and_then(|workspace| {
                workspace
                    .checkpoints
                    .iter()
                    .find(|item| item.task_id == task_id && item.name == name)
            })
            .cloned())
    }

    async fn list_checkpoints(&self, workspace_id: &str, task_id: &str) -> Result<Vec<Checkpoint>> {
        let state = self.state.read().await;
        Ok(state
            .workspaces
            .get(workspace_id)
            .map(|workspace| {
                workspace
                    .checkpoints
                    .iter()
                    .filter(|item| item.task_id == task_id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn delete_checkpoint(&self, workspace_id: &str, task_id: &str, name: &str) -> Result<()> {
        {
            let mut state = self.state.write().await;
            let workspace = Self::workspace_mut(&mut state, workspace_id);
            workspace
                .checkpoints
                .retain(|item| !(item.task_id == task_id && item.name == name));
        }
        self.persist().await
    }

    async fn ls(&self, workspace_id: &str, path: &str) -> Result<Vec<MemoryHierarchyNode>> {
        let all = self.list(workspace_id).await?;
        Ok(MemoryTree::build_ls(all, path))
    }

    async fn list_workspaces(&self) -> Result<Vec<String>> {
        let state = self.state.read().await;
        Ok(state.workspaces.keys().cloned().collect())
    }

    async fn load_entity_graph_snapshot(&self, workspace_id: &str) -> Result<Option<String>> {
        let state = self.state.read().await;
        Ok(state
            .workspaces
            .get(workspace_id)
            .and_then(|w| w.entity_graph_snapshot.clone()))
    }

    async fn save_entity_graph_snapshot(&self, workspace_id: &str, data: &str) -> Result<()> {
        {
            let mut state = self.state.write().await;
            let workspace = Self::workspace_mut(&mut state, workspace_id);
            workspace.entity_graph_snapshot = Some(data.to_string());
        }
        self.persist().await
    }
}

// ---------------------------------------------------------------------------
// InMemoryMemoryStore
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct InMemoryMemoryStore {
    state: Arc<RwLock<DurableStoreFile>>,
}

impl InMemoryMemoryStore {
    /// New.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl MemoryStore for InMemoryMemoryStore {
    fn backend(&self) -> MemoryBackend {
        MemoryBackend::Memory
    }

    fn as_any(&self) -> &dyn StdAny {
        self
    }

    async fn health(&self) -> Result<String> {
        Ok("in-memory store".to_string())
    }

    async fn put(&self, record: MemoryRecord) -> Result<()> {
        let workspace_id = record.workspace_id.clone();
        let mut state = self.state.write().await;
        let workspace = state.workspaces.entry(workspace_id).or_default();
        workspace.memories.retain(|item| item.id != record.id);
        workspace.memories.push(record);
        Ok(())
    }

    async fn get(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let state = self.state.read().await;
        Ok(state.workspaces.get(workspace_id).and_then(|workspace| {
            workspace
                .memories
                .iter()
                .find(|item| item.id == id_or_path || item.path == id_or_path)
                .cloned()
        }))
    }

    async fn update(&self, record: MemoryRecord) -> Result<()> {
        self.put(record).await
    }

    async fn delete(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let mut state = self.state.write().await;
        let Some(workspace) = state.workspaces.get_mut(workspace_id) else {
            return Ok(None);
        };
        let previous_len = workspace.memories.len();
        workspace
            .memories
            .retain(|item| item.id != id_or_path && item.path != id_or_path);
        if workspace.memories.len() == previous_len {
            return Ok(None);
        }
        Ok(None)
    }

    async fn list(&self, workspace_id: &str) -> Result<Vec<MemoryRecord>> {
        let state = self.state.read().await;
        Ok(state
            .workspaces
            .get(workspace_id)
            .map(|workspace| workspace.memories.clone())
            .unwrap_or_default())
    }

    async fn search(
        &self,
        workspace_id: &str,
        query: &str,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryRecord>> {
        let records = self.list(workspace_id).await?;
        filter_records(records, workspace_id, query, filters)
    }

    async fn load_workspace_state(&self, workspace_id: &str) -> Result<DurableWorkspaceState> {
        let state = self.state.read().await;
        Ok(state
            .workspaces
            .get(workspace_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn save_beliefs(&self, workspace_id: &str, beliefs: Vec<BeliefEdge>) -> Result<()> {
        let mut state = self.state.write().await;
        let workspace = state
            .workspaces
            .entry(workspace_id.to_string())
            .or_default();
        workspace.beliefs = beliefs;
        Ok(())
    }

    async fn save_session_token(
        &self,
        workspace_id: &str,
        token: SessionTokenRecord,
    ) -> Result<()> {
        let mut state = self.state.write().await;
        let workspace = state
            .workspaces
            .entry(workspace_id.to_string())
            .or_default();
        workspace
            .session_tokens
            .retain(|existing| existing.token != token.token);
        workspace.session_tokens.push(token);
        Ok(())
    }

    async fn is_session_token_valid(&self, workspace_id: &str, token: &str) -> Result<bool> {
        let now = Utc::now();
        let state = self.state.read().await;
        Ok(state
            .workspaces
            .get(workspace_id)
            .map(|workspace| {
                workspace.session_tokens.iter().any(|item| {
                    xavier::server::http::api::constant_time_compare(&item.token, token)
                        && item.expires_at > now
                })
            })
            .unwrap_or(false))
    }

    async fn save_checkpoint(&self, workspace_id: &str, checkpoint: Checkpoint) -> Result<()> {
        let mut state = self.state.write().await;
        let workspace = state
            .workspaces
            .entry(workspace_id.to_string())
            .or_default();
        workspace
            .checkpoints
            .retain(|item| !(item.task_id == checkpoint.task_id && item.name == checkpoint.name));
        workspace.checkpoints.push(checkpoint);
        Ok(())
    }

    async fn load_checkpoint(
        &self,
        workspace_id: &str,
        task_id: &str,
        name: &str,
    ) -> Result<Option<Checkpoint>> {
        let state = self.state.read().await;
        Ok(state.workspaces.get(workspace_id).and_then(|workspace| {
            workspace
                .checkpoints
                .iter()
                .find(|item| item.task_id == task_id && item.name == name)
                .cloned()
        }))
    }

    async fn list_checkpoints(&self, workspace_id: &str, task_id: &str) -> Result<Vec<Checkpoint>> {
        let state = self.state.read().await;
        Ok(state
            .workspaces
            .get(workspace_id)
            .map(|workspace| {
                workspace
                    .checkpoints
                    .iter()
                    .filter(|item| item.task_id == task_id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn delete_checkpoint(&self, workspace_id: &str, task_id: &str, name: &str) -> Result<()> {
        let mut state = self.state.write().await;
        if let Some(workspace) = state.workspaces.get_mut(workspace_id) {
            workspace
                .checkpoints
                .retain(|item| !(item.task_id == task_id && item.name == name));
        }
        Ok(())
    }

    async fn list_workspaces(&self) -> Result<Vec<String>> {
        let state = self.state.read().await;
        Ok(state.workspaces.keys().cloned().collect())
    }

    async fn load_entity_graph_snapshot(&self, workspace_id: &str) -> Result<Option<String>> {
        let state = self.state.read().await;
        Ok(state
            .workspaces
            .get(workspace_id)
            .and_then(|w| w.entity_graph_snapshot.clone()))
    }

    async fn save_entity_graph_snapshot(&self, workspace_id: &str, data: &str) -> Result<()> {
        let mut state = self.state.write().await;
        let workspace = state
            .workspaces
            .entry(workspace_id.to_string())
            .or_default();
        workspace.entity_graph_snapshot = Some(data.to_string());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Shared helper functions
// ---------------------------------------------------------------------------

/// Revisioned record.
pub(crate) fn revisioned_record(existing: MemoryRecord, mut next: MemoryRecord) -> MemoryRecord {
    next.id = existing.id;
    next.created_at = existing.created_at;
    next.updated_at = Utc::now();
    next.revision = existing.revision + 1;
    next.revisions = existing.revisions.clone();
    next.revisions.push(MemoryRevision {
        revision: next.revision,
        recorded_at: next.updated_at,
        path: next.path.clone(),
        content: next.content.clone(),
        metadata: next.metadata.clone(),
    });
    next
}

/// Filter records.
/// Hierarchy + resolved-namespace predicate for a single record. Used by
/// [`filter_records`] and by store implementations that must evaluate filters
/// AFTER decrypting each row (see `VecSqliteMemoryStore::list_filtered`).
pub(crate) fn record_matches_filters(
    record: &MemoryRecord,
    workspace_id: &str,
    filters: Option<&MemoryQueryFilters>,
) -> bool {
    filters.is_none_or(|filters| {
        let matches_hierarchical = filters.cluster_ids.as_ref().is_none_or(|ids| {
            record
                .cluster_id
                .as_ref()
                .map(|id| ids.contains(id))
                .unwrap_or(false)
        }) && filters
            .levels
            .as_ref()
            .is_none_or(|levels| levels.contains(&record.level));

        if !matches_hierarchical {
            return false;
        }

        resolve_metadata(&record.path, &record.metadata, workspace_id, None)
            .map(|resolved| {
                filters
                    .org_id
                    .as_deref()
                    .is_none_or(|value| resolved.namespace.org_id.as_deref() == Some(value))
                    && filters
                        .user_id
                        .as_deref()
                        .is_none_or(|value| resolved.namespace.user_id.as_deref() == Some(value))
                    && filters
                        .agent_id
                        .as_deref()
                        .is_none_or(|value| resolved.namespace.agent_id.as_deref() == Some(value))
                    && filters.workspace_id.as_deref().is_none_or(|value| {
                        resolved.namespace.workspace_id.as_deref() == Some(value)
                    })
                    && filters
                        .project
                        .as_deref()
                        .is_none_or(|value| resolved.namespace.project.as_deref() == Some(value))
                    && filters
                        .scope
                        .as_deref()
                        .is_none_or(|value| resolved.namespace.scope.as_deref() == Some(value))
                    && filters
                        .session_id
                        .as_deref()
                        .is_none_or(|value| resolved.namespace.session_id.as_deref() == Some(value))
            })
            .unwrap_or(false)
    })
}

pub(crate) fn filter_records(
    records: Vec<MemoryRecord>,
    workspace_id: &str,
    query: &str,
    filters: Option<&MemoryQueryFilters>,
) -> Result<Vec<MemoryRecord>> {
    Ok(records
        .into_iter()
        .filter(|record| {
            record.matches_query(query) && record_matches_filters(record, workspace_id, filters)
        })
        .collect())
}

/// True when `filters` carries any predicate that cannot be fully enforced at
/// SQL level for at-rest-encrypted rows: their metadata is an opaque blob, so
/// `json_extract` yields NULL and `build_filtered_query` admits them through
/// the `encrypted_dek IS NOT NULL OR ...` bypass. The effective match then
/// happens in Rust, after decryption. Callers MUST NOT push `LIMIT` into SQL
/// for such filters — otherwise `ORDER BY created_at DESC LIMIT n` fills the
/// candidate window with the newest rows of the table regardless of the
/// filter and real matches deeper in the table are silently dropped (the
/// `get_project_context` -> 0 records production bug, 2026-09-11).
pub(crate) fn filters_require_post_decrypt_match(filters: &MemoryQueryFilters) -> bool {
    filters.kinds.is_some()
        || filters.evidence_kinds.is_some()
        || filters.org_id.is_some()
        || filters.user_id.is_some()
        || filters.agent_id.is_some()
        || filters.session_id.is_some()
        || filters.project.is_some()
        || filters.scope.is_some()
        || filters.retrieval_scope.is_some()
        || filters.source_app.is_some()
        || filters.source_type.is_some()
        || filters.repo_url.is_some()
        || filters.file_path.is_some()
        || filters.symbol.is_some()
        || filters.url.is_some()
        || filters.message_id.is_some()
        || filters.topic_key.is_some()
        || filters.observed_after.is_some()
        || filters.observed_before.is_some()
        || filters.recorded_after.is_some()
        || filters.recorded_before.is_some()
        || filters.zones.is_some()
        || filters.clearances.is_some()
}

#[async_trait]
impl<T: MemoryStore> crate::domain::cycle_breaks::w30_12::MemoryConsumerStore for T {
    fn into_consumer_store(
        self: Arc<Self>,
    ) -> Arc<dyn crate::domain::cycle_breaks::w30_12::MemoryConsumerStore>
    where
        Self: 'static,
    {
        self
    }

    async fn put_record(&self, record: MemoryRecord) -> Result<()> {
        self.put(record).await
    }

    async fn update_record(&self, record: MemoryRecord) -> Result<()> {
        self.update(record).await
    }

    async fn list_records(&self, workspace_id: &str) -> Result<Vec<MemoryRecord>> {
        self.list(workspace_id).await
    }

    async fn save_checkpoint_record(
        &self,
        workspace_id: &str,
        checkpoint: Checkpoint,
    ) -> Result<()> {
        self.save_checkpoint(workspace_id, checkpoint).await
    }

    async fn load_checkpoint_record(
        &self,
        workspace_id: &str,
        task_id: &str,
        name: &str,
    ) -> Result<Option<Checkpoint>> {
        self.load_checkpoint(workspace_id, task_id, name).await
    }

    async fn list_checkpoint_records(
        &self,
        workspace_id: &str,
        task_id: &str,
    ) -> Result<Vec<Checkpoint>> {
        self.list_checkpoints(workspace_id, task_id).await
    }

    async fn delete_checkpoint_record(
        &self,
        workspace_id: &str,
        task_id: &str,
        name: &str,
    ) -> Result<()> {
        self.delete_checkpoint(workspace_id, task_id, name).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn non_vec_store_keeps_empty_vector_and_default_set_embedding_fills_it() {
        let store = InMemoryMemoryStore::new();
        let mut record = dummy_record("m1");
        record.workspace_id = "ws".to_string();
        record.revision = 1;
        store.put_without_embedding(record).await.unwrap();
        assert!(store
            .get("ws", "m1")
            .await
            .unwrap()
            .unwrap()
            .embedding
            .is_empty());

        store
            .set_embedding("ws", "m1", vec![1.0, 2.0])
            .await
            .unwrap();
        assert_eq!(
            store.get("ws", "m1").await.unwrap().unwrap().embedding,
            vec![1.0, 2.0]
        );
        assert!(store.set_embedding("ws", "nope", vec![1.0]).await.is_err());
    }

    fn dummy_record(id: &str) -> MemoryRecord {
        MemoryRecord {
            id: id.to_string(),
            ..Default::default()
        }
    }

    fn dummy_hit(id: &str, vector_similarity: Option<f32>) -> HybridSearchResult {
        HybridSearchResult {
            record: dummy_record(id),
            score: 0.0,
            vector_score: 0.0,
            lexical_score: 0.0,
            kg_score: 0.0,
            bm25: None,
            vector_similarity,
        }
    }

    #[test]
    fn merge_is_rank_based_and_deterministic() {
        let leg1 = FederatedLegs {
            store: "store1".to_string(),
            model_ok: true,
            vector: vec![
                dummy_hit("a", Some(0.9)), // globally rank 1 if it's the only one
                dummy_hit("b", Some(0.8)), // globally rank 2
            ],
            lexical: vec![
                dummy_hit("b", None), // rank 1 locally
                dummy_hit("c", None), // rank 2 locally
            ],
            kg: vec![],
        };

        let leg2 = FederatedLegs {
            store: "store2".to_string(),
            model_ok: true,
            vector: vec![
                dummy_hit("a", Some(0.9)), // same similarity, tie break by store ("store1" < "store2") -> globally rank 2, store1's 'a' is rank 1
                dummy_hit("d", Some(0.85)), // globally rank 3
            ],
            lexical: vec![],
            kg: vec![
                dummy_hit("e", None), // rank 1 locally
            ],
        };

        let hits = merge_federated(&[leg1, leg2], 60, 10);
        assert_eq!(hits.len(), 6); // store1:a, store2:a, store2:d, store1:b, store1:c, store2:e

        assert_eq!(hits[0].store, "store1");
        assert_eq!(hits[0].record_id, "b");
    }

    #[test]
    fn vector_merge_uses_raw_similarity_across_stores() {
        let leg1 = FederatedLegs {
            store: "store1".to_string(),
            model_ok: true,
            vector: vec![dummy_hit("a", Some(0.1))],
            lexical: vec![],
            kg: vec![],
        };
        let leg2 = FederatedLegs {
            store: "store2".to_string(),
            model_ok: true,
            vector: vec![dummy_hit("b", Some(0.9))],
            lexical: vec![],
            kg: vec![],
        };

        let hits = merge_federated(&[leg1, leg2], 60, 10);
        // store2:b should be rank 1 in vector pool, store1:a rank 2
        assert_eq!(hits[0].store, "store2");
        assert_eq!(hits[0].record_id, "b");
        assert_eq!(hits[1].store, "store1");
        assert_eq!(hits[1].record_id, "a");
    }

    #[test]
    fn model_mismatch_skips_vector_leg_and_flags_store() {
        let leg1 = FederatedLegs {
            store: "store1".to_string(),
            model_ok: false,
            vector: vec![
                dummy_hit("a", Some(0.99)), // High sim, but should be ignored
            ],
            lexical: vec![dummy_hit("b", None)],
            kg: vec![],
        };

        let hits = merge_federated(&[leg1], 60, 10);
        // Only lexical should survive
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record_id, "b");
        assert!(hits[0].skipped_model_mismatch);
        assert_eq!(hits[0].sources, vec!["lexical".to_string()]);
    }

    #[test]
    fn dedup_by_store_and_record_id() {
        let leg1 = FederatedLegs {
            store: "store1".to_string(),
            model_ok: true,
            vector: vec![dummy_hit("a", Some(0.9))],
            lexical: vec![dummy_hit("a", None)],
            kg: vec![dummy_hit("a", None)],
        };

        let hits = merge_federated(&[leg1], 60, 10);
        assert_eq!(hits.len(), 1);
        let hit = &hits[0];
        assert_eq!(hit.record_id, "a");
        assert_eq!(hit.sources.len(), 3);
        assert!(hit.sources.contains(&"vector".to_string()));
        assert!(hit.sources.contains(&"lexical".to_string()));
        assert!(hit.sources.contains(&"kg".to_string()));
    }

    fn namespaced_record(id: &str, agent: Option<&str>) -> MemoryRecord {
        let mut metadata = serde_json::Map::new();
        metadata.insert("org_id".into(), "org-1".into());
        metadata.insert("user_id".into(), "user-1".into());
        if let Some(agent) = agent {
            metadata.insert("agent_id".into(), agent.into());
        }
        MemoryRecord {
            id: id.to_string(),
            workspace_id: "ws".to_string(),
            path: format!("notes/{id}"),
            content: format!("shared note {id}"),
            metadata: serde_json::Value::Object(metadata),
            ..Default::default()
        }
    }

    #[test]
    fn record_filter_agent_id_excludes_other_agents() {
        let filters = MemoryQueryFilters {
            agent_id: Some("agent-a".to_string()),
            ..Default::default()
        };
        let own = namespaced_record("a", Some("agent-a"));
        let other = namespaced_record("b", Some("agent-b"));
        let missing = namespaced_record("c", None);
        assert!(record_matches_filters(&own, "ws", Some(&filters)));
        assert!(!record_matches_filters(&other, "ws", Some(&filters)));
        assert!(!record_matches_filters(&missing, "ws", Some(&filters)));

        let org = MemoryQueryFilters {
            org_id: Some("org-2".to_string()),
            ..Default::default()
        };
        assert!(!record_matches_filters(&own, "ws", Some(&org)));
        let user = MemoryQueryFilters {
            user_id: Some("user-2".to_string()),
            ..Default::default()
        };
        assert!(!record_matches_filters(&own, "ws", Some(&user)));
        let all = MemoryQueryFilters {
            org_id: Some("org-1".to_string()),
            user_id: Some("user-1".to_string()),
            agent_id: Some("agent-a".to_string()),
            ..Default::default()
        };
        assert!(record_matches_filters(&own, "ws", Some(&all)));
    }

    #[test]
    fn record_filter_unset_ids_match_everything() {
        let filters = MemoryQueryFilters::default();
        for record in [
            namespaced_record("a", Some("agent-a")),
            namespaced_record("b", Some("agent-b")),
            namespaced_record("c", None),
            dummy_record("d"),
        ] {
            assert!(record_matches_filters(&record, "ws", Some(&filters)));
            assert!(record_matches_filters(&record, "ws", None));
        }
    }

    #[tokio::test]
    async fn in_memory_store_agent_id_filter_returns_only_that_agent() {
        let store = InMemoryMemoryStore::new();
        store
            .put(namespaced_record("a", Some("agent-a")))
            .await
            .unwrap();
        store
            .put(namespaced_record("b", Some("agent-b")))
            .await
            .unwrap();
        store.put(namespaced_record("c", None)).await.unwrap();
        let filters = MemoryQueryFilters {
            agent_id: Some("agent-a".to_string()),
            ..Default::default()
        };

        let listed = store.list_filtered("ws", &filters, 10).await.unwrap();
        assert_eq!(
            listed.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["a"]
        );
        let found = store
            .search("ws", "shared note", Some(&filters))
            .await
            .unwrap();
        assert_eq!(
            found.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["a"]
        );
        assert_eq!(
            store.search("ws", "shared note", None).await.unwrap().len(),
            3
        );
    }
}
