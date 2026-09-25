//! HTTP API v1 endpoint definitions
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::embedding;
use crate::server::http::types::*;
use crate::workspace::WorkspaceContext;
use axum::{extract::Json, response::IntoResponse, Extension};
use tracing::info;

/// Memory add.
pub async fn memory_add(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<AddMemoryRequest>,
) -> impl IntoResponse {
    info!(
        path = payload.path.as_deref().unwrap_or("default"),
        "memory_add"
    );
    let path = payload.path.unwrap_or_else(|| "default".to_string());
    let content = payload.content;
    let mut metadata = payload.metadata.unwrap_or(serde_json::json!({}));
    if let Some(object) = metadata.as_object_mut() {
        let agent_id = object
            .get("agent_id")
            .and_then(|v| v.as_str())
            .unwrap_or("http");
        object.insert(
            "_audit".to_string(),
            serde_json::json!({ "agent_id": agent_id, "operation": "memory.add" }),
        );
    }
    let typed = crate::memory::schema::TypedMemoryPayload {
        kind: payload.kind,
        evidence_kind: payload.evidence_kind,
        namespace: payload.namespace,
        provenance: payload.provenance,
        cluster_id: payload.cluster_id,
        level: payload.level,
        zone: None,
        relation: payload.relation,
        clearance: None,
    };
    if let Err(error) = workspace
        .workspace
        .ensure_within_storage_limit(&path, &content, &metadata)
        .await
    {
        return crate::error::ApiError::validation(error.to_string()).into_ok_response();
    }
    #[derive(Debug, serde::Serialize)]
    pub struct AddMemoryResponse {
        pub status: &'static str,
        pub message: &'static str,
        pub workspace_id: String,
        /// `true` when the embedding is being computed asynchronously in the
        /// background (record was persisted immediately via lexical/FTS so a
        /// fresh install with an unreachable/misconfigured embedding endpoint
        /// never blocks or fails this write; `xavier reindex` also backfills
        /// any record left in this state). `false` when no embedder is
        /// configured at all, so no background embedding will ever run.
        pub embedding_pending: bool,
    }

    // Persist the record immediately with an empty vector so the write never
    // blocks on (or fails because of) an unreachable/misconfigured embedding
    // endpoint — the lexical/FTS index is always populated synchronously.
    // The real embedding, when an embedder is configured, is computed and
    // backfilled in the background (mirrors the MCP `create_memory` fast
    // path). Records left without an embedding default to
    // `embedding_status = 'pending'` and are picked up by `xavier reindex`.
    let embedding_pending = embedding::EmbedderConfig::from_env().is_configured();

    if let Err(error) = workspace
        .workspace
        .ingest_typed(
            path.clone(),
            content.clone(),
            metadata.clone(),
            Some(typed.clone()),
            Some(Vec::new()),
            false,
        )
        .await
    {
        return crate::error::ApiError::internal(format!("failed to add memory: {}", error))
            .into_ok_response();
    }

    if embedding_pending {
        let ws_clone = workspace.workspace.clone();
        tokio::spawn(async move {
            if crate::memory::qmd::writer::bus_quota_exhausted(&path) {
                tracing::debug!(
                    path = %path,
                    "memory_add: skipping background embedding: bus quota exhausted"
                );
                return;
            }
            match crate::memory::qmd_memory::reader::generate_embedding(&content).await {
                Ok(vector) if !vector.is_empty() => {
                    if let Err(error) = ws_clone
                        .ingest_typed(path, content, metadata, Some(typed), Some(vector), false)
                        .await
                    {
                        tracing::warn!(
                            error = %error,
                            "memory_add: background embedding backfill failed"
                        );
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "memory_add: background embedding generation failed; record stays pending for xavier reindex"
                    );
                }
            }
        });
    }

    Json(AddMemoryResponse {
        status: "ok",
        message: "Document added to memory",
        workspace_id: workspace.workspace_id,
        embedding_pending,
    })
    .into_response()
}

/// Memory search.
pub async fn memory_search(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<SearchRequest>,
) -> impl IntoResponse {
    match workspace
        .workspace
        .memory
        .search_filtered_with_mode(&payload.query, payload.limit, payload.filters.as_ref())
        .await
    {
        Ok((docs, mode)) => Json(SearchResponse {
            status: "ok".to_string(),
            results: docs
                .into_iter()
                .map(|doc| SearchHit {
                    id: doc.id,
                    path: doc.path,
                    content: doc.content,
                    metadata: doc.metadata,
                })
                .collect(),
            query: payload.query,
            mode: mode.as_str().to_string(),
        })
        .into_response(),
        Err(error) => crate::error::ApiError::internal(format!("memory search failed: {}", error))
            .into_ok_response(),
    }
}

/// Memory hybrid search.
pub async fn memory_hybrid_search(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<HybridSearchRequest>,
) -> impl IntoResponse {
    let mode = payload.search_type.unwrap_or_default();
    match workspace
        .workspace
        .durable_store()
        .hybrid_search(
            &workspace.workspace_id,
            &payload.query,
            mode,
            payload.filters.as_ref(),
            payload.limit,
        )
        .await
    {
        Ok(results) => Json(HybridSearchResponse {
            status: "ok".to_string(),
            results: results
                .into_iter()
                .map(|result| HybridSearchHit {
                    id: result.record.id,
                    path: result.record.path,
                    content: result.record.content,
                    metadata: result.record.metadata,
                    score: result.score,
                    vector_score: result.vector_score,
                    lexical_score: result.lexical_score,
                    kg_score: result.kg_score,
                    bm25: result.bm25,
                })
                .collect(),
            query: payload.query,
            mode,
        })
        .into_response(),
        Err(error) => crate::error::ApiError::internal(error.to_string()).into_response(),
    }
}
