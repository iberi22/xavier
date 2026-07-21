//! HTTP handler for memory operations
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::adapters::inbound::http::state::check_auth;
use crate::adapters::inbound::http::AppState;
use crate::domain::memory::{MemoryQueryFilters, MemoryRecord as DomainMemoryRecord};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use tracing::info;

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
                0x09 | 0x0A | 0x0D => true, // tab, newline, carriage return
                0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F => false, // control chars
                0xD800..=0xDFFF => false,   // surrogate pairs
                _ => true,
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
}

#[derive(Debug, Deserialize)]
pub struct UpdatePayload {
    pub id: String,
    pub content: String,
    pub path: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
    pub project: Option<String>,
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
                    // FIX A007: Include path and metadata in search results
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
            Ok(Json(serde_json::json!({
                "status": "error",
                "message": e.to_string(),
            })))
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
        Err(e) => Ok(Json(serde_json::json!({
            "status": "error",
            "message": e.to_string(),
        }))),
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
        Err(e) => Ok(Json(serde_json::json!({
            "status": "error",
            "message": e.to_string(),
        }))),
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
        Err(e) => Ok(Json(serde_json::json!({
            "status": "error",
            "message": e.to_string(),
        }))),
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
                    // FIX A007: Include path and metadata in search results
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
            Json(serde_json::json!({
                "status": "error",
                "message": e.to_string(),
            }))
        }
    }
}
