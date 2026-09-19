//! DocBot REST API routes.
//!
//! Mounts at `/api/docbot/v1/` (separate from the main Xavier REST API to
//! avoid coupling). All routes share the `DocBotState` extension.
//!
//! ## Endpoints
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | POST | `/collections` | Create a collection |
//! | GET  | `/collections` | List all collections |
//! | GET  | `/collections/{id}` | Get a specific collection |
//! | DELETE | `/collections/{id}` | Delete a collection |
//! | POST | `/collections/{id}/ingest` | Upload and ingest a document |
//! | GET  | `/collections/{id}/documents` | List documents in a collection |
//! | POST | `/ask` | Ask a RAG question (all collections) |
//! | POST | `/collections/{id}/ask` | Ask a RAG question (specific collection) |
//! | GET  | `/health` | Gateway health check |

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::Path,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use tracing::{error, info};

use crate::collections::indexer::{ingest_document, ChunkConfig};
use crate::collections::schema::{AskRequest, CreateCollectionRequest, DocCollection};
use crate::collections::store::CollectionStore;
use crate::rag::pipeline::DocBotPipeline;

// ── Shared state ───────────────────────────────────────────────────────────────

/// Shared state injected into all DocBot routes.
#[derive(Clone)]
pub struct DocBotState {
    pub store: Arc<CollectionStore>,
    pub pipeline: Arc<DocBotPipeline>,
    pub chunk_config: ChunkConfig,
}

// ── Router ─────────────────────────────────────────────────────────────────────

/// Build the DocBot Axum router.
pub fn docbot_router(state: DocBotState) -> Router {
    Router::new()
        // Collections CRUD
        .route("/collections", post(create_collection))
        .route("/collections", get(list_collections))
        .route("/collections/{id}", get(get_collection))
        .route("/collections/{id}", delete(delete_collection))
        // Document ingestion
        .route("/collections/{id}/ingest", post(ingest_doc))
        .route("/collections/{id}/documents", get(list_documents))
        // RAG ask
        .route("/ask", post(ask_all))
        .route("/collections/{id}/ask", post(ask_collection))
        // Health
        .route("/health", get(health))
        // Inject state as Extension
        .layer(Extension(Arc::new(state)))
}

// ── Handlers ───────────────────────────────────────────────────────────────────

/// POST /collections — Create a new document collection.
async fn create_collection(
    Extension(state): Extension<Arc<DocBotState>>,
    Json(req): Json<CreateCollectionRequest>,
) -> impl IntoResponse {
    let mut col = DocCollection::new(&req.name);
    if let Some(desc) = req.description {
        col = col.with_description(desc);
    }
    if let Some(tags) = req.tags {
        col = col.with_tags(tags);
    }
    if let Some(lang) = req.language {
        col = col.with_language(lang);
    }
    if let Some(access) = req.access {
        col = col.with_access(access);
    }

    match state.store.create_collection(&col) {
        Ok(()) => {
            info!(id = %col.id, name = %col.name, "collection created");
            (StatusCode::CREATED, Json(col)).into_response()
        }
        Err(e) => {
            error!(error = %e, "create_collection failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    }
}

/// GET /collections — List all collections.
async fn list_collections(Extension(state): Extension<Arc<DocBotState>>) -> impl IntoResponse {
    match state.store.list_collections(None) {
        Ok(cols) => Json(serde_json::json!({
            "collections": cols,
            "count": cols.len(),
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /collections/:id — Get a specific collection.
async fn get_collection(
    Extension(state): Extension<Arc<DocBotState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.store.get_collection(&id) {
        Ok(Some(col)) => Json(col).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "collection not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// DELETE /collections/:id — Delete a collection and all its documents.
async fn delete_collection(
    Extension(state): Extension<Arc<DocBotState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.store.delete_collection(&id) {
        Ok(0) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "collection not found" })),
        )
            .into_response(),
        Ok(n) => Json(serde_json::json!({ "deleted": n })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /collections/:id/ingest — Upload a document (raw bytes).
///
/// Send the file as raw body. Use these headers:
/// - `Content-Type`: MIME type of the document (e.g. `application/pdf`)
/// - `X-Document-Title`: display title (optional, falls back to filename)
/// - `X-Document-Filename`: original file name (optional)
async fn ingest_doc(
    Extension(state): Extension<Arc<DocBotState>>,
    Path(collection_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Verify collection exists
    match state.store.get_collection(&collection_id) {
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "collection not found" })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
        Ok(Some(_)) => {}
    }

    if body.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "request body is empty" })),
        )
            .into_response();
    }

    let mime_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let file_name = headers
        .get("x-document-filename")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let title = headers
        .get("x-document-title")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| file_name.clone())
        .unwrap_or_else(|| "Untitled".to_string());

    let bytes = body.to_vec();

    let doc_title = title;

    match ingest_document(
        state.store.clone(),
        None, // vec_store: plugged in Wave 2
        None, // embedder: plugged in Wave 2
        &collection_id,
        &doc_title,
        file_name.as_deref(),
        &mime_type,
        &bytes,
        &state.chunk_config,
    )
    .await
    {
        Ok(result) => {
            info!(
                doc_id = %result.doc_id,
                chunks = result.chunk_count,
                collection = %collection_id,
                "document ingested via HTTP"
            );
            (
                StatusCode::CREATED,
                Json(serde_json::json!({
                    "doc_id": result.doc_id,
                    "chunk_count": result.chunk_count,
                    "collection_id": collection_id,
                    "status": "ready",
                })),
            )
                .into_response()
        }
        Err(e) => {
            error!(error = %e, "ingest_document failed");
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    }
}

/// GET /collections/:id/documents — List documents in a collection.
async fn list_documents(
    Extension(state): Extension<Arc<DocBotState>>,
    Path(collection_id): Path<String>,
) -> impl IntoResponse {
    match state.store.list_docs(&collection_id) {
        Ok(docs) => Json(serde_json::json!({
            "documents": docs,
            "count": docs.len(),
            "collection_id": collection_id,
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /ask — Ask a RAG question across all accessible collections.
async fn ask_all(
    Extension(state): Extension<Arc<DocBotState>>,
    Json(req): Json<AskRequest>,
) -> impl IntoResponse {
    match state.pipeline.ask(&req).await {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => {
            error!(error = %e, "RAG pipeline failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    }
}

/// POST /collections/:id/ask — Ask a RAG question scoped to one collection.
async fn ask_collection(
    Extension(state): Extension<Arc<DocBotState>>,
    Path(collection_id): Path<String>,
    Json(mut req): Json<AskRequest>,
) -> impl IntoResponse {
    // Override collection scope
    req.collection_id = Some(collection_id);
    match state.pipeline.ask(&req).await {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => {
            error!(error = %e, "RAG pipeline failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    }
}

/// GET /health — Gateway health check.
async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "xavier-docbot",
    }))
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rag::llm_adapter::{LlmBackend, LlmConfig};
    use crate::rag::pipeline::{DocBotPipeline, PipelineConfig};
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use tempfile::tempdir;
    use tower::util::ServiceExt;

    fn make_state() -> DocBotState {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_routes.db");
        let store = Arc::new(CollectionStore::open(&path).unwrap());
        let pipeline = Arc::new(DocBotPipeline::new(
            store.clone(),
            LlmConfig {
                backend: LlmBackend::RetrievalOnly,
                ..Default::default()
            },
            PipelineConfig::default(),
        ));
        DocBotState {
            store,
            pipeline,
            chunk_config: ChunkConfig::default(),
        }
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let state = make_state();
        let app = docbot_router(state);

        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_create_and_list_collections() {
        let state = make_state();
        let app = docbot_router(state);

        let body = serde_json::json!({ "name": "Test Collection", "language": "es" });
        let req = Request::builder()
            .method("POST")
            .uri("/collections")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }
}
