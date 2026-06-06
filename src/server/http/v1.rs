//! HTTP API v1 endpoint definitions
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use axum::{extract::Json, response::IntoResponse, Extension};
use tracing::info;
use crate::workspace::WorkspaceContext;
use crate::server::http::types::*;
use crate::embedding;

pub async fn memory_add(Extension(workspace): Extension<WorkspaceContext>, Json(payload): Json<AddMemoryRequest>) -> impl IntoResponse {
    info!(path = payload.path.as_deref().unwrap_or("default"), "memory_add");
    let path = payload.path.unwrap_or_else(|| "default".to_string());
    let content = payload.content;
    let mut metadata = payload.metadata.unwrap_or(serde_json::json!({}));
    if let Some(object) = metadata.as_object_mut() {
        let agent_id = object.get("agent_id").and_then(|v| v.as_str()).unwrap_or("http");
        object.insert("_audit".to_string(), serde_json::json!({ "agent_id": agent_id, "operation": "memory.add" }));
    }
    let typed = crate::memory::schema::TypedMemoryPayload { kind: payload.kind, evidence_kind: payload.evidence_kind, namespace: payload.namespace, provenance: payload.provenance, cluster_id: payload.cluster_id, level: payload.level, zone: None, relation: payload.relation };
    let content_vector = match embedding::build_embedder_from_env().await {
        Ok(embedder) => match embedder.encode(&content).await {
            Ok(vector) if !vector.is_empty() => Some(vector),
            _ => None,
        },
        _ => None,
    };
    if let Err(error) = workspace.workspace.ensure_within_storage_limit(&path, &content, &metadata).await {
        return Json(serde_json::json!({ "status": "error", "message": error.to_string(), "workspace_id": workspace.workspace_id }));
    }
    match workspace.workspace.ingest_typed(path, content, metadata, Some(typed), content_vector, false).await {
        Ok(_) => Json(serde_json::json!({ "status": "ok", "message": "Document added to memory", "workspace_id": workspace.workspace_id })),
        Err(error) => Json(serde_json::json!({ "status": "error", "message": format!("failed to add memory: {}", error), "workspace_id": workspace.workspace_id })),
    }
}

pub async fn memory_search(Extension(workspace): Extension<WorkspaceContext>, Json(payload): Json<SearchRequest>) -> Json<serde_json::Value> {
    match workspace.workspace.memory.search_filtered(&payload.query, payload.limit, payload.filters.as_ref()).await {
        Ok(docs) => Json(serde_json::json!(SearchResponse {
            status: "ok".to_string(),
            results: docs.into_iter().map(|doc| serde_json::json!({ "id": doc.id, "path": doc.path, "content": doc.content, "metadata": doc.metadata })).collect(),
            query: payload.query,
        })),
        Err(error) => Json(serde_json::json!({ "status": "error", "message": format!("memory search failed: {}", error), "workspace_id": workspace.workspace_id })),
    }
}

pub async fn memory_hybrid_search(Extension(workspace): Extension<WorkspaceContext>, Json(payload): Json<HybridSearchRequest>) -> impl IntoResponse {
    let mode = payload.search_type.unwrap_or_default();
    match workspace.workspace.durable_store().hybrid_search(&workspace.workspace_id, &payload.query, mode, payload.filters.as_ref(), payload.limit).await {
        Ok(results) => Json(HybridSearchResponse {
            status: "ok".to_string(),
            results: results.into_iter().map(|result| serde_json::json!({ "id": result.record.id, "path": result.record.path, "content": result.record.content, "metadata": result.record.metadata, "score": result.score, "vector_score": result.vector_score, "lexical_score": result.lexical_score, "kg_score": result.kg_score, "bm25": result.bm25 })).collect(),
            query: payload.query,
            mode,
        }).into_response(),
        Err(error) => Json(serde_json::json!({ "status": "error", "message": error.to_string(), "query": payload.query, "mode": mode })).into_response(),
    }
}
