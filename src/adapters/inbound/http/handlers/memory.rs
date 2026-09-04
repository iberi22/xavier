//! HTTP handler for memory operations
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use super::{error_json, error_response};
use crate::adapters::inbound::http::state::check_auth;
use crate::adapters::inbound::http::AppState;
use crate::domain::memory::{MemoryQueryFilters, MemoryRecord as DomainMemoryRecord};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::memory::sync::{ChunkDiff, DiffAction, ManifestEntry};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GraphViewNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub trust_score: f32,
    pub memory_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GraphViewLink {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub weight: f32,
    pub confidence_score: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GraphViewStats {
    pub entities: usize,
    pub relations: usize,
    pub shown_nodes: usize,
    pub shown_links: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GraphViewResponse {
    pub status: String,
    pub layer: String,
    pub truncated: bool,
    pub nodes: Vec<GraphViewNode>,
    pub links: Vec<GraphViewLink>,
    pub stats: GraphViewStats,
}

/// Sanitize unicode text by removing control characters and invalid surrogates.
/// This prevents JSON serialization errors from invalid UTF-8 sequences.
fn sanitize_unicode(input: &str) -> String {
    input
        .chars()
        .filter(|c| {
            // Allow printable ASCII, extended ASCII, and valid Unicode
            // Remove control chars (except tab, newline, carriage return)
            // Remove surrogate pairs (U+D800-U=DFFF) using numeric comparison
            let code = *c as u32;
            match code {
                0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F | 0xD800..=0xDFFF => false, // control chars & surrogate pairs
                _ => true, // printable ASCII/Unicode including tab (0x09), newline (0x0A), carriage return (0x0D)
            }
        })
        .collect()
}

#[derive(Debug, Deserialize)]
pub struct SearchPayload {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub filters: Option<MemoryQueryFilters>,
    #[serde(default)]
    pub depth: usize,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub limit: Option<usize>,
    pub project: Option<String>,
    pub depth: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct AddPayload {
    pub content: String,
    pub path: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
    pub project: Option<String>,
    #[serde(default)]
    pub dedup: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePayload {
    pub id: String,
    pub content: String,
    pub path: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
    pub project: Option<String>,
    #[serde(default)]
    pub dedup: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeletePayload {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct MemoryQueryPayload {
    pub query: String,
    pub limit: Option<usize>,
    pub filters: Option<serde_json::Value>,
}

fn default_limit() -> usize {
    10
}

/// Search get handler.
pub async fn search_get_handler(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut filters = MemoryQueryFilters::default();
    if let Some(project) = query.project {
        filters.project = Some(project);
    }

    let payload = SearchPayload {
        query: query.q,
        limit: query.limit.unwrap_or(10),
        filters: Some(filters),
        depth: query.depth.unwrap_or(0),
    };

    search_handler(headers, State(state), Json(payload)).await
}

/// Search handler.
pub async fn search_handler(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<SearchPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_auth(&headers, &state)?;
    // Security scan on query before searching
    let sec_result = match state.security.process_input(&payload.query).await {
        Ok(res) => res,
        Err(e) => {
            return Ok(Json(serde_json::json!({
                "results": [],
                "query": payload.query,
                "count": 0,
                "error": format!("Security scan error: {}", e),
                "workspace_id": state.workspace_id,
            })));
        }
    };

    if !sec_result.allowed {
        info!(
            "Search blocked by security: injection detected (confidence={})",
            sec_result.detection_confidence
        );
        return Ok(Json(serde_json::json!({
            "results": <Vec<serde_json::Value>>::new(),
            "query": payload.query,
            "count": 0,
            "blocked": true,
            "reason": "security_policy_violation",
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            },
            "workspace_id": state.workspace_id,
        })));
    }

    let effective_query = sec_result
        .sanitized_input
        .as_deref()
        .unwrap_or(&sec_result.original_input);
    let limit = payload.limit.clamp(1, 100);
    info!("Search request: query={}, limit={}", effective_query, limit);

    match state
        .memory
        .search(effective_query, limit, payload.filters.clone())
        .await
    {
        Ok(mut results) => {
            // Apply depth expansion if requested
            if payload.depth > 0 {
                results = state
                    .memory
                    .expand_depth(&results, payload.depth, payload.filters)
                    .await
                    .unwrap_or(results);
            }

            let documents: Vec<_> = results
                .into_iter()
                .map(|doc| {
                    serde_json::json!({
                        "id": doc.id,
                        "path": doc.path,
                        "content": doc.content,
                        "metadata": doc.metadata,
                        "embedding": doc.embedding,
                    })
                })
                .collect();

            Ok(Json(serde_json::json!({
                "status": "ok",
                "query": payload.query,
                "count": documents.len(),
                "results": documents,
                "workspace_id": state.workspace_id,
            })))
        }
        Err(e) => {
            info!("Search error: {}", e);
            Ok(error_json(e))
        }
    }
}

/// Add handler.
pub async fn add_handler(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<AddPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_auth(&headers, &state)?;
    let now = chrono::Utc::now();
    // FIX A001: Sanitize unicode to prevent JSON serialization errors
    let sanitized_content = sanitize_unicode(&payload.content);
    let sanitized_path = sanitize_unicode(&payload.path);

    let mut metadata = payload.metadata.clone();
    // Inject dedup mode and project into metadata
    if let Some(ref mut obj) = metadata.as_object_mut() {
        if let Some(dedup) = &payload.dedup {
            obj.insert("_dedup_mode".to_string(), serde_json::json!(dedup));
        }
    }
    if let Some(project) = payload.project {
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert("project".to_string(), serde_json::json!(project));
        }
    }

    let record = DomainMemoryRecord {
        id: String::new(),
        workspace_id: state.workspace_id.clone(),
        path: sanitized_path.clone(),
        content: sanitized_content,
        metadata,
        embedding: Vec::new(),
        created_at: now,
        updated_at: now,
        revision: 1,
        primary: true,
        parent_id: None,
        cluster_id: None,
        level: crate::memory::schema::MemoryLevel::Raw,
        relation: None,
        clearance: Default::default(),
        revisions: Vec::new(),
        ..Default::default()
    };
    // Note: domain metadata translation would go here if needed

    // Currently MemoryQueryPort::add doesn't take TypedMemoryPayload directly.
    // QmdMemoryAdapter::add calls record.to_document() which loses TypedMemoryPayload if not in record.metadata.
    // However, record.metadata is used by normalize_metadata in QmdMemory::add_document.

    match state.memory.add(record).await {
        Ok(id) => Ok(Json(serde_json::json!({
            "status": "ok",
            "id": id,
            // FIX: Return sanitized_path (the actual persisted value), not payload.path
            "path": sanitized_path,
            "workspace_id": state.workspace_id,
        }))),
        Err(e) => Ok(error_json(e)),
    }
}

/// Update handler.
pub async fn update_handler(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<UpdatePayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_auth(&headers, &state)?;
    let now = chrono::Utc::now();
    let sanitized_content = sanitize_unicode(&payload.content);
    let sanitized_path = sanitize_unicode(&payload.path);

    let mut metadata = payload.metadata.clone();
    // Inject dedup mode and project into metadata
    if let Some(ref mut obj) = metadata.as_object_mut() {
        if let Some(dedup) = &payload.dedup {
            obj.insert("_dedup_mode".to_string(), serde_json::json!(dedup));
        }
    }
    if let Some(project) = payload.project {
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert("project".to_string(), serde_json::json!(project));
        }
    }

    let record = DomainMemoryRecord {
        id: payload.id.clone(),
        workspace_id: state.workspace_id.clone(),
        path: sanitized_path.clone(),
        content: sanitized_content,
        metadata,
        embedding: Vec::new(),
        created_at: now,
        updated_at: now,
        revision: 1,
        primary: true,
        parent_id: None,
        cluster_id: None,
        level: crate::memory::schema::MemoryLevel::Raw,
        relation: None,
        clearance: Default::default(),
        revisions: Vec::new(),
        ..Default::default()
    };

    match state.memory.update(&payload.id, record).await {
        Ok(updated_record) => Ok(Json(serde_json::json!({
            "status": "ok",
            "id": updated_record.id,
            "path": updated_record.path,
            "workspace_id": state.workspace_id,
        }))),
        Err(e) => Ok(error_json(e)),
    }
}

/// Delete handler.
pub async fn delete_handler(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<DeletePayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_auth(&headers, &state)?;

    match state.memory.delete(&payload.id).await {
        Ok(Some(record)) => Ok(Json(serde_json::json!({
            "status": "ok",
            "deleted": true,
            "id": record.id,
            "path": record.path,
        }))),
        Ok(None) => Ok(Json(serde_json::json!({
            "status": "not_found",
            "deleted": false,
            "id": payload.id,
        }))),
        Err(e) => Ok(error_json(e)),
    }
}

/// Stats handler.
pub async fn stats_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    // Note: MemoryQueryPort doesn't have stats() yet, might need to add it or use storage directly
    // For now returning placeholder or calling list
    Json(serde_json::json!({
        "status": "ok",
        "workspace_id": state.workspace_id,
        "message": "Memory stats not yet implemented in port interface",
    }))
}

/// GET /v1/memories/graph
///
/// Returns full bi-directional memory graph (nodes and links) formatted for
/// GraphCanvas / ForceGraph2D web UI rendering.
pub async fn get_graph_handler(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<GraphViewResponse>, (StatusCode, Json<serde_json::Value>)> {
    check_auth(&headers, &state)?;

    let records = state
        .memory
        .search("", 1000, None)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let mut nodes = Vec::new();
    let mut links = Vec::new();
    let mut seen_nodes = std::collections::HashSet::new();

    for record in &records {
        let node_id = if record.id.is_empty() {
            record.path.clone()
        } else {
            record.id.clone()
        };

        if node_id.is_empty() {
            continue;
        }

        if seen_nodes.insert(node_id.clone()) {
            let label = record
                .metadata
                .get("title")
                .and_then(|v: &serde_json::Value| v.as_str())
                .unwrap_or(&record.path)
                .to_string();

            let kind = record
                .metadata
                .get("kind")
                .and_then(|v: &serde_json::Value| v.as_str())
                .unwrap_or("memory")
                .to_string();

            let description = if !record.content.is_empty() {
                Some(record.content.chars().take(120).collect::<String>())
            } else {
                record
                    .metadata
                    .get("description")
                    .and_then(|v: &serde_json::Value| v.as_str())
                    .map(|s: &str| s.to_string())
            };

            nodes.push(GraphViewNode {
                id: node_id.clone(),
                label,
                kind,
                description,
                trust_score: 1.0,
                memory_count: 1,
            });
        }

        // Link with parent_id if present
        if let Some(parent_id) = &record.parent_id {
            if !parent_id.is_empty() {
                links.push(GraphViewLink {
                    source: parent_id.clone(),
                    target: node_id.clone(),
                    relation: "parent".to_string(),
                    weight: 1.0,
                    confidence_score: 1.0,
                });
            }
        }

        // Parse relations from metadata if array exists
        if let Some(rel_array) = record.metadata.get("relations").and_then(|v: &serde_json::Value| v.as_array()) {
            for rel in rel_array {
                if let (Some(target), Some(rel_type)) = (
                    rel.get("target").and_then(|v: &serde_json::Value| v.as_str()),
                    rel.get("relation").and_then(|v: &serde_json::Value| v.as_str()),
                ) {
                    links.push(GraphViewLink {
                        source: node_id.clone(),
                        target: target.to_string(),
                        relation: rel_type.to_string(),
                        weight: rel
                            .get("weight")
                            .and_then(|v: &serde_json::Value| v.as_f64())
                            .map(|f| f as f32)
                            .unwrap_or(1.0),
                        confidence_score: 1.0,
                    });
                }
            }
        }

        // Link with parent_id if present
        if let Some(ref parent_id) = record.parent_id {
            if !parent_id.is_empty() {
                links.push(GraphViewLink {
                    source: parent_id.clone(),
                    target: node_id.clone(),
                    relation: "parent".to_string(),
                    weight: 1.0,
                    confidence_score: 1.0,
                });
            }
        }

        // Parse relations from metadata if array exists
        if let Some(rel_array) = record.metadata.get("relations").and_then(|v| v.as_array()) {
            for rel in rel_array {
                if let (Some(target), Some(rel_type)) = (
                    rel.get("target").and_then(|v| v.as_str()),
                    rel.get("relation").and_then(|v| v.as_str()),
                ) {
                    links.push(GraphViewLink {
                        source: node_id.clone(),
                        target: target.to_string(),
                        relation: rel_type.to_string(),
                        weight: rel
                            .get("weight")
                            .and_then(|v| v.as_f64())
                            .map(|f| f as f32)
                            .unwrap_or(1.0),
                        confidence_score: 1.0,
                    });
                }
            }
        }
    }

    let stats = GraphViewStats {
        entities: nodes.len(),
        relations: links.len(),
        shown_nodes: nodes.len(),
        shown_links: links.len(),
    };

    Ok(Json(GraphViewResponse {
        status: "ok".to_string(),
        layer: "memory".to_string(),
        truncated: false,
        nodes,
        links,
        stats,
    }))
}

#[derive(Debug, Deserialize)]
pub struct ManifestQuery {
    pub workspace_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ManifestResponse {
    pub status: &'static str,
    pub workspace_id: String,
    pub count: usize,
    pub entries: Vec<ManifestEntry>,
}

/// POST /v1/memories/sync/manifest
///
/// Returns workspace manifest entries for diffing and sync reconciliation.
pub async fn sync_manifest_handler(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ManifestQuery>,
) -> Result<Json<ManifestResponse>, (StatusCode, Json<serde_json::Value>)> {
    check_auth(&headers, &state)?;

    let records = state
        .memory
        .search("", 5000, None)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let workspace_id = query
        .workspace_id
        .filter(|w| !w.is_empty())
        .unwrap_or_else(|| state.workspace_id.clone());

    let mut entries = Vec::new();
    for record in records {
        let path = if record.path.is_empty() {
            record.id.clone()
        } else {
            record.path.clone()
        };

        if path.is_empty() {
            continue;
        }

        let chunk_hash = if let Some(hash) = record.metadata.get("chunk_hash").and_then(|v: &serde_json::Value| v.as_str()) {
            hash.to_string()
        } else {
            let mut hasher = sha2::Sha256::new();
            use sha2::Digest;
            hasher.update(record.content.as_bytes());
            format!("{:x}", hasher.finalize())
        };

        let updated_at = record
            .metadata
            .get("updated_at")
            .and_then(|v: &serde_json::Value| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt: chrono::DateTime<chrono::FixedOffset>| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);

        let size_bytes = record.content.len() as u64;

        entries.push(ManifestEntry {
            chunk_hash,
            namespace: workspace_id.clone(),
            revision: record.revision,
            updated_at,
            size_bytes,
            record_path: Some(path),
        });
    }

    let count = entries.len();

    Ok(Json(ManifestResponse {
        status: "ok",
        workspace_id,
        count,
        entries,
    }))
}

#[derive(Debug, Serialize)]
pub struct SyncPushDeltaResponse {
    pub status: &'static str,
    pub received: usize,
    pub applied: usize,
    pub conflicts: usize,
}

/// POST /v1/memories/sync/push
///
/// Accepts a batch of ChunkDiff deltas from web clients/peers, resolves conflicts
/// using Last-Write-Wins (LWW), and updates the memory store.
pub async fn sync_push_delta_handler(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(diffs): Json<Vec<ChunkDiff>>,
) -> Result<Json<SyncPushDeltaResponse>, (StatusCode, Json<serde_json::Value>)> {
    check_auth(&headers, &state)?;

    let total_received = diffs.len();
    let mut applied = 0;
    let mut conflicts = 0;

    // Fetch existing records for comparison/lookup during LWW
    let existing_records = state
        .memory
        .search("", 5000, None)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    for diff in diffs {
        match diff.action {
            DiffAction::Add | DiffAction::Update => {
                let Some(data) = &diff.data else {
                    continue;
                };

                let incoming: DomainMemoryRecord = match serde_json::from_slice(data) {
                    Ok(r) => r,
                    Err(e) => {
                        info!("sync_push_delta_handler: error deserializing chunk {}: {}", diff.chunk_hash, e);
                        continue;
                    }
                };

                let target_path = if !incoming.path.is_empty() {
                    &incoming.path
                } else {
                    &incoming.id
                };

                let existing = existing_records.iter().find(|r| {
                    (!r.path.is_empty() && &r.path == target_path)
                        || (!r.id.is_empty() && r.id == incoming.id)
                });

                match existing {
                    None => {
                        if state.memory.add(incoming).await.is_ok() {
                            applied += 1;
                        }
                    }
                    Some(local) => {
                        if incoming.updated_at > local.updated_at {
                            conflicts += 1;
                            let local_id = if local.id.is_empty() {
                                &local.path
                            } else {
                                &local.id
                            };
                            if state.memory.update(local_id, incoming).await.is_ok() {
                                applied += 1;
                            }
                        } else if incoming.updated_at == local.updated_at {
                            let local_node = local
                                .metadata
                                .get("node_id")
                                .and_then(|v: &serde_json::Value| v.as_str())
                                .unwrap_or(&local.id);
                            let incoming_node = incoming
                                .metadata
                                .get("node_id")
                                .and_then(|v: &serde_json::Value| v.as_str())
                                .unwrap_or(&incoming.id);

                            if incoming_node >= local_node {
                                conflicts += 1;
                                let local_id = if local.id.is_empty() {
                                    &local.path
                                } else {
                                    &local.id
                                };
                                if state.memory.update(local_id, incoming).await.is_ok() {
                                    applied += 1;
                                }
                            }
                        }
                    }
                }
            }
            DiffAction::Delete => {
                let path_or_id = match &diff.record_path {
                    Some(p) => p.as_str(),
                    None => continue,
                };

                if let Some(local) = existing_records.iter().find(|r| r.path == path_or_id || r.id == path_or_id) {
                    let id = if local.id.is_empty() { &local.path } else { &local.id };
                    if state.memory.delete(id).await.is_ok() {
                        applied += 1;
                    }
                } else if state.memory.delete(path_or_id).await.is_ok() {
                    applied += 1;
                }
            }
        }
    }

    Ok(Json(SyncPushDeltaResponse {
        status: "ok",
        received: total_received,
        applied,
        conflicts,
    }))
}

#[cfg(test)]
mod memory_handler_tests {
    use super::*;
    use crate::domain::memory::MemoryRecord;
    use crate::ports::inbound::agent_lifecycle_port::AgentLifecyclePort;
    use crate::domain::security::ThreatLevel;
    use crate::ports::inbound::health_port::{HealthPort, HealthStatus};
    use crate::ports::inbound::input_security_port::InputSecurityPort;
    use crate::ports::inbound::security_port::SecurityScanPort;
    use crate::ports::inbound::session_port::{SessionEventResult, SessionPort};
    use crate::ports::inbound::session_sync_port::SessionSyncPort;
    use crate::ports::inbound::time_metrics_port::TimeMetricsPort;
    use crate::ports::inbound::verification_port::{VerificationPort, VerificationResult};
    use crate::ports::inbound::MemoryQueryPort;
    use crate::tasks::session_sync_task::SyncCheckResult;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    pub struct TestMemoryQueryPort {
        pub records: Mutex<Vec<MemoryRecord>>,
    }

    #[async_trait]
    impl MemoryQueryPort for TestMemoryQueryPort {
        async fn search(
            &self,
            _query: &str,
            _limit: usize,
            _filters: Option<crate::domain::memory::MemoryQueryFilters>,
        ) -> anyhow::Result<Vec<MemoryRecord>> {
            Ok(self.records.lock().unwrap().clone())
        }

        async fn expand_depth(
            &self,
            _results: &[MemoryRecord],
            _depth: usize,
            _filters: Option<crate::domain::memory::MemoryQueryFilters>,
        ) -> anyhow::Result<Vec<MemoryRecord>> {
            Ok(Vec::new())
        }

        async fn add(&self, record: MemoryRecord) -> anyhow::Result<String> {
            let id = if record.id.is_empty() {
                record.path.clone()
            } else {
                record.id.clone()
            };
            self.records.lock().unwrap().push(record);
            Ok(id)
        }

        async fn update(&self, id: &str, record: MemoryRecord) -> anyhow::Result<MemoryRecord> {
            let mut guard = self.records.lock().unwrap();
            if let Some(existing) = guard.iter_mut().find(|r| r.id == id || r.path == id) {
                *existing = record.clone();
            } else {
                guard.push(record.clone());
            }
            Ok(record)
        }

        async fn delete(&self, id: &str) -> anyhow::Result<Option<MemoryRecord>> {
            let mut guard = self.records.lock().unwrap();
            if let Some(pos) = guard.iter().position(|r| r.id == id || r.path == id) {
                Ok(Some(guard.remove(pos)))
            } else {
                Ok(None)
            }
        }

        async fn get(&self, id: &str) -> anyhow::Result<Option<MemoryRecord>> {
            let guard = self.records.lock().unwrap();
            Ok(guard.iter().find(|r| r.id == id || r.path == id).cloned())
        }

        async fn list(&self, _workspace_id: &str, _limit: usize) -> anyhow::Result<Vec<MemoryRecord>> {
            Ok(self.records.lock().unwrap().clone())
        }

        async fn export(&self, _public_only: bool) -> anyhow::Result<Vec<MemoryRecord>> {
            Ok(self.records.lock().unwrap().clone())
        }

        async fn ls(&self, _path: &str) -> anyhow::Result<Vec<crate::memory::qmd::types::NavEntry>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct MockPort;

    #[async_trait]
    impl InputSecurityPort for MockPort {
        async fn process_input(&self, input: &str) -> anyhow::Result<crate::ports::inbound::input_security_port::SecureInputResult> {
            Ok(crate::ports::inbound::input_security_port::SecureInputResult {
                allowed: true,
                is_injection: false,
                detection_confidence: 0.0,
                attack_type: "none".to_string(),
                sanitized_input: Some(input.to_string()),
                original_input: input.to_string(),
            })
        }
        async fn process_output(&self, output: &str) -> anyhow::Result<String> {
            Ok(output.to_string())
        }
    }

    #[async_trait]
    impl SecurityScanPort for MockPort {
        async fn scan(&self, target: &str, _level: Option<ThreatLevel>) -> anyhow::Result<crate::domain::security::ScanResult> {
            Ok(crate::domain::security::ScanResult {
                id: "scan-1".to_string(),
                scanned_target: target.to_string(),
                scan_duration_ms: 5,
                completed_at: chrono::Utc::now(),
                threats: Vec::new(),
            })
        }
        async fn get_config(&self) -> anyhow::Result<serde_json::Value> {
            Ok(serde_json::json!({}))
        }
    }

    #[async_trait]
    impl TimeMetricsPort for MockPort {
        async fn save_time_metric(
            &self,
            _metric: &crate::domain::memory::TimeMetric,
            _workspace_id: &str,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    #[async_trait]
    impl AgentLifecyclePort for MockPort {
        async fn register(&self, _agent_id: String, _session_id: String, _metadata: crate::domain::agent::AgentMetadata) -> bool {
            true
        }
        async fn unregister(&self, _agent_id: &str) -> bool {
            true
        }
        async fn heartbeat(&self, _agent_id: &str) -> bool {
            true
        }
        async fn get_active_agents(&self) -> Vec<crate::domain::agent::AgentEntry> {
            Vec::new()
        }
        async fn get(&self, _agent_id: &str) -> Option<crate::domain::agent::AgentEntry> {
            None
        }
        async fn on_task_start(&self, _agent_id: &str, _task_id: &str) {}
        async fn on_task_complete(
            &self,
            _agent_id: &str,
            _task_id: &str,
            _result: &Result<crate::agents::runtime::AgentResponse, String>,
        ) {}
    }

    #[async_trait]
    impl HealthPort for MockPort {
        async fn get_health_status(&self) -> HealthStatus {
            HealthStatus {
                status: "healthy".to_string(),
                lag_ms: 0,
                save_ok_rate: 1.0,
                match_score: 1.0,
                active_agents: 1,
                timestamp_ms: 0,
                alerts: Vec::new(),
            }
        }
    }

    #[async_trait]
    impl VerificationPort for MockPort {
        async fn verify_save(
            &self,
            _xavier_url: &str,
            _auth_token: &str,
            path: &str,
            _test_content: &str,
        ) -> Result<VerificationResult, String> {
            Ok(VerificationResult {
                path: path.to_string(),
                save_ok: true,
                retrieve_ok: true,
                match_score: 1.0,
                latency_ms: 10,
            })
        }
    }

    #[async_trait]
    impl SessionSyncPort for MockPort {
        async fn check(&self) -> anyhow::Result<SyncCheckResult> {
            Ok(self.last_result().await)
        }
        async fn last_result(&self) -> SyncCheckResult {
            SyncCheckResult {
                status: "ok".to_string(),
                lag_ms: 0,
                save_ok_rate: 1.0,
                match_score: 1.0,
                active_agents: 1,
                timestamp_ms: 0,
                alerts: Vec::new(),
            }
        }
    }

    #[async_trait]
    impl SessionPort for MockPort {
        async fn handle_event(&self, _event: crate::session::types::SessionEvent) -> bool {
            true
        }
        async fn handle_and_index_event(
            &self,
            _event: crate::session::types::SessionEvent,
        ) -> anyhow::Result<SessionEventResult> {
            Ok(SessionEventResult {
                status: "ok".to_string(),
                session_id: "test".to_string(),
                mapped: true,
                memory_id: Some("mem-1".to_string()),
            })
        }
    }

    fn make_test_app_state(mem: Arc<TestMemoryQueryPort>) -> AppState {
        let mock = Arc::new(MockPort);
        let temp_dir = std::env::temp_dir().join(format!("xavier_test_graph_{}", ulid::Ulid::new()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let db_path = temp_dir.join("code_graph.db");
        let code_db = Arc::new(code_graph::db::CodeGraphDB::new(&db_path).unwrap());
        let code_indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&code_db)));
        let code_query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&code_db)));
        let espacio = Arc::new(crate::espacio::SpaceManager::new(temp_dir.join("spaces")));

        AppState {
            memory: mem,
            security: mock.clone(),
            security_scan: mock.clone(),
            time_metrics: mock.clone(),
            agent_lifecycle: mock.clone(),
            health: mock.clone(),
            verification: mock.clone(),
            session_sync: mock.clone(),
            session: mock.clone(),
            espacio,
            workspace_id: "test-workspace".to_string(),
            auth_token: "test-token".to_string(),
            secrets_engine: None,
            code_db,
            code_indexer,
            code_query,
        }
    }

    #[tokio::test]
    async fn test_get_graph_handler_returns_nodes_and_links() {
        let mem = Arc::new(TestMemoryQueryPort::default());
        let rec1 = MemoryRecord {
            id: "node-1".to_string(),
            workspace_id: "test-workspace".to_string(),
            path: "docs/concept1".to_string(),
            content: "Concept 1 description text".to_string(),
            metadata: serde_json::json!({
                "title": "Concept One",
                "kind": "concept",
                "relations": [
                    {"target": "node-2", "relation": "depends_on", "weight": 0.8}
                ]
            }),
            ..Default::default()
        };
        let rec2 = MemoryRecord {
            id: "node-2".to_string(),
            workspace_id: "test-workspace".to_string(),
            path: "docs/concept2".to_string(),
            content: "Concept 2 description text".to_string(),
            parent_id: Some("node-1".to_string()),
            metadata: serde_json::json!({
                "title": "Concept Two",
                "kind": "concept"
            }),
            ..Default::default()
        };
        mem.records.lock().unwrap().push(rec1);
        mem.records.lock().unwrap().push(rec2);

        let state = make_test_app_state(mem);
        let mut headers = HeaderMap::new();
        headers.insert("X-Xavier-Token", "test-token".parse().unwrap());

        let res = get_graph_handler(headers, State(state)).await.unwrap();
        let body = res.0;

        assert_eq!(body.status, "ok");
        assert_eq!(body.nodes.len(), 2);
        assert!(body.links.len() >= 2);
        assert_eq!(body.stats.entities, 2);
    }

    #[tokio::test]
    async fn test_sync_manifest_handler() {
        let mem = Arc::new(TestMemoryQueryPort::default());
        let rec = MemoryRecord {
            id: "rec-1".to_string(),
            workspace_id: "test-workspace".to_string(),
            path: "memory/item1".to_string(),
            content: "chunk payload content".to_string(),
            revision: 3,
            ..Default::default()
        };
        mem.records.lock().unwrap().push(rec);

        let state = make_test_app_state(mem);
        let mut headers = HeaderMap::new();
        headers.insert("X-Xavier-Token", "test-token".parse().unwrap());

        let res = sync_manifest_handler(headers, State(state), Query(ManifestQuery { workspace_id: None })).await.unwrap();
        let body = res.0;

        assert_eq!(body.status, "ok");
        assert_eq!(body.workspace_id, "test-workspace");
        assert_eq!(body.count, 1);
        assert_eq!(body.entries[0].revision, 3);
        assert_eq!(body.entries[0].record_path.as_deref(), Some("memory/item1"));
    }

    #[tokio::test]
    async fn test_sync_push_delta_handler_lww() {
        let mem = Arc::new(TestMemoryQueryPort::default());
        let now = chrono::Utc::now();
        let old_rec = MemoryRecord {
            id: "rec-1".to_string(),
            workspace_id: "test-workspace".to_string(),
            path: "memory/item1".to_string(),
            content: "old content".to_string(),
            updated_at: now - chrono::Duration::hours(1),
            ..Default::default()
        };
        mem.records.lock().unwrap().push(old_rec);

        let state = make_test_app_state(mem);
        let mut headers = HeaderMap::new();
        headers.insert("X-Xavier-Token", "test-token".parse().unwrap());

        let updated_rec = MemoryRecord {
            id: "rec-1".to_string(),
            workspace_id: "test-workspace".to_string(),
            path: "memory/item1".to_string(),
            content: "updated content via LWW push".to_string(),
            updated_at: now,
            ..Default::default()
        };

        let diff = ChunkDiff {
            chunk_hash: "hash123".to_string(),
            namespace: "test-workspace".to_string(),
            action: DiffAction::Update,
            data: Some(serde_json::to_vec(&updated_rec).unwrap()),
            timestamp: std::time::SystemTime::now(),
            record_path: Some("memory/item1".to_string()),
        };

        let res = sync_push_delta_handler(headers, State(state.clone()), Json(vec![diff])).await.unwrap();
        let body = res.0;

        assert_eq!(body.status, "ok");
        assert_eq!(body.received, 1);
        assert_eq!(body.applied, 1);

        let records = state.memory.search("", 10, None).await.unwrap();
        let target = records.iter().find(|r| r.path == "memory/item1").unwrap();
        assert_eq!(target.content, "updated content via LWW push");
    }
}

/// Memory query handler.
pub async fn memory_query_handler(
    State(state): State<AppState>,
    Json(payload): Json<MemoryQueryPayload>,
) -> Json<serde_json::Value> {
    // Security scan on query
    let sec_result = match state.security.process_input(&payload.query).await {
        Ok(res) => res,
        Err(e) => {
            return Json(serde_json::json!({
                "status": "error",
                "message": format!("Security scan error: {}", e),
            }));
        }
    };

    if !sec_result.allowed {
        return Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let _limit = payload.limit.unwrap_or(10).clamp(1, 100);
    let effective_query = sec_result
        .sanitized_input
        .as_deref()
        .unwrap_or(&sec_result.original_input);

    match state.memory.search(effective_query, _limit, None).await {
        Ok(results) => {
            let documents: Vec<_> = results
                .into_iter()
                .map(|doc| {
                    serde_json::json!({
                        "id": doc.id,
                        "path": doc.path,
                        "content": doc.content,
                        "metadata": doc.metadata,
                        "embedding": doc.embedding,
                    })
                })
                .collect();

            Json(serde_json::json!({
                "status": "ok",
                "query": payload.query,
                "count": documents.len(),
                "results": documents,
                "workspace_id": state.workspace_id,
            }))
        }
        Err(e) => {
            info!("Memory query error: {}", e);
            error_json(e)
        }
    }
}
