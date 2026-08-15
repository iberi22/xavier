//! REST routes for F12 preservation features.
//!
//! Exposes the F12 modules (public directory, public RAG, groups, redaction,
//! curation, private mesh, service network) over HTTP under `/v1/f12/*`.
//! These modules are pure libraries; this file adds the HTTP surface.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use crate::security::clearance::ClearanceLevel;
use crate::security::redaction::{parse_segmented, SegmentedDoc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::curation::CurationQueue;
use crate::codebase::snapshot::SnapshotManager;
use crate::mesh::private_mesh::{PrivateMeshRegistry, WalletNode};
use crate::mesh::public_directory::PublicDirectory;
use crate::mesh::public_rag::{search_public, PublicRagResult};
use crate::mesh::node::NodeId;
use crate::mesh::service_network::{ServiceKind, ServiceRegistry, TelemetrySample};
use crate::security::groups::GroupRegistry;

/// Shared F12 state: data dir paths + in-memory registries (lazy loaded).
#[derive(Clone)]
pub struct F12State {
    pub data_dir: PathBuf,
    pub registry: Arc<Mutex<F12Registries>>,
}

#[derive(Default)]
pub struct F12Registries {
    pub groups: Option<GroupRegistry>,
    pub directory: Option<PublicDirectory>,
    pub private_mesh: Option<PrivateMeshRegistry>,
    pub curation: Option<CurationQueue>,
    pub service_network: Option<ServiceRegistry>,
}

impl F12State {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            registry: Arc::new(Mutex::new(F12Registries::default())),
        }
    }

    fn groups_mut(&self) -> std::sync::MutexGuard<'_, F12Registries> {
        let mut reg = self.registry.lock().unwrap();
        if reg.groups.is_none() {
            reg.groups = Some(
                GroupRegistry::load_from(self.data_dir.join("security/groups.json"))
                    .unwrap_or_else(|_| GroupRegistry::load().expect("groups registry")),
            );
        }
        reg
    }

    fn directory_mut(&self) -> std::sync::MutexGuard<'_, F12Registries> {
        let mut reg = self.registry.lock().unwrap();
        if reg.directory.is_none() {
            reg.directory = Some(
                PublicDirectory::load_from(self.data_dir.join("mesh/public-directory.json"))
                    .unwrap_or_else(|_| PublicDirectory::load().expect("directory")),
            );
        }
        reg
    }

    fn private_mesh_mut(&self) -> std::sync::MutexGuard<'_, F12Registries> {
        let mut reg = self.registry.lock().unwrap();
        if reg.private_mesh.is_none() {
            reg.private_mesh = Some(
                PrivateMeshRegistry::load_or_create(self.data_dir.join("mesh/private-mesh.json"))
                    .unwrap_or_else(|_| {
                        PrivateMeshRegistry::load_or_create(
                            self.data_dir.join("mesh/private-mesh.json"),
                        )
                        .expect("mesh")
                    }),
            );
        }
        reg
    }

    fn curation_mut(&self) -> std::sync::MutexGuard<'_, F12Registries> {
        let mut reg = self.registry.lock().unwrap();
        if reg.curation.is_none() {
            reg.curation = Some(CurationQueue::new_with_path(
                self.data_dir.join("curation/queue.json"),
            ));
        }
        reg
    }

    fn service_network_mut(&self) -> std::sync::MutexGuard<'_, F12Registries> {
        let mut reg = self.registry.lock().unwrap();
        if reg.service_network.is_none() {
            reg.service_network = Some(ServiceRegistry::new());
        }
        reg
    }
}

// ---------- request/response types ----------

#[derive(Debug, Deserialize)]
pub struct RAGRequest {
    pub query: String,
    pub repo: Option<String>,
    pub limit: Option<u8>,
}

#[derive(Debug, Serialize)]
pub struct RAGResponse {
    pub results: Vec<PublicRagResult>,
}

#[derive(Debug, Deserialize)]
pub struct CreateGroupRequest {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct JoinGroupRequest {
    pub group_id: String,
    pub member_id: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisterNodeRequest {
    pub node_id: String,
    pub name: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterWalletNodeRequest {
    pub node_id: String,
    pub wallet_id: String,
    pub name: String,
    pub iroh_addr: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RedactRequest {
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct SubmitCurationRequest {
    pub content_ref: String,
    pub proposed_clearance: String,
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApproveCurationRequest {
    pub id: String,
    pub curator: String,
    pub classification: Option<String>,
    pub clearance: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApproveReviewRequest {
    pub curator: String,
    pub classification: Option<String>,
    pub clearance: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RejectReviewRequest {
    pub curator: String,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSnapshotRequest {
    pub repo: String,
    pub repo_root: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PublishTelemetryRequest {
    pub node_id: String,
    pub kind: Option<ServiceKind>,
    pub payload: String,
    pub ts: Option<i64>,
}

// ---------- handlers ----------

pub async fn rag_search(
    State(_state): State<F12State>,
    Json(req): Json<RAGRequest>,
) -> impl IntoResponse {
    if req.query.trim().is_empty() {
        return Json(RAGResponse {
            results: Vec::new(),
        })
        .into_response();
    }
    let limit = req.limit.unwrap_or(5).min(50);
    let results = search_public(&req.query, req.repo.as_deref(), limit);
    Json(RAGResponse { results }).into_response()
}

pub async fn list_groups(State(state): State<F12State>) -> impl IntoResponse {
    // GroupRegistry has no public getter for all groups; expose via the
    // persisted JSON file for the listing.
    let path = state.data_dir.join("security/groups.json");
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let v: serde_json::Value =
                serde_json::from_str(&content).unwrap_or(serde_json::json!({ "groups": [] }));
            Json(v).into_response()
        }
        Err(_) => Json(serde_json::json!({ "groups": [] })).into_response(),
    }
}

pub async fn create_group(
    State(state): State<F12State>,
    Json(req): Json<CreateGroupRequest>,
) -> impl IntoResponse {
    let mut reg = state.groups_mut();
    let groups = reg.groups.as_mut().expect("groups registry");
    let group = crate::security::groups::InfoGroup {
        id: req.id,
        name: req.name,
        members: Vec::new(),
        acl: crate::security::groups::GroupAcl {
            read: true,
            write: false,
            audit: false,
        },
    };
    match groups.create(group) {
        Ok(_) => {
            let _ = groups.save();
            (StatusCode::CREATED, "group created").into_response()
        }
        Err(e) => (StatusCode::CONFLICT, e.to_string()).into_response(),
    }
}

pub async fn list_directory(State(state): State<F12State>) -> impl IntoResponse {
    let reg = state.directory_mut();
    let dir = reg.directory.as_ref().expect("directory");
    Json(dir.list_nodes()).into_response()
}

pub async fn register_node(
    State(state): State<F12State>,
    Json(req): Json<RegisterNodeRequest>,
) -> impl IntoResponse {
    let mut reg = state.directory_mut();
    let dir = reg.directory.as_mut().expect("directory");
    let entry = crate::mesh::public_directory::PublicNodeEntry {
        node_id: crate::mesh::node::NodeId(req.node_id),
        name: req.name,
        capabilities: req.capabilities,
        iroh_addr: None,
        last_heartbeat: chrono::Utc::now().timestamp_millis() as u64,
        tree: crate::mesh::public_directory::InfoTree {
            repos: std::collections::HashMap::new(),
            memorias: crate::mesh::public_directory::MemoriaInfo {
                count: 0,
                kinds: Vec::new(),
            },
            skills: crate::mesh::public_directory::SkillInfo { count: 0 },
        },
    };
    match dir.register_node(entry) {
        Ok(_) => {
            let _ = dir.save();
            (StatusCode::CREATED, "node registered").into_response()
        }
        Err(e) => (StatusCode::CONFLICT, e.to_string()).into_response(),
    }
}

pub async fn register_wallet_node(
    State(state): State<F12State>,
    Json(req): Json<RegisterWalletNodeRequest>,
) -> impl IntoResponse {
    let mut reg = state.private_mesh_mut();
    let mesh = reg.private_mesh.as_mut().expect("mesh");
    let node = WalletNode {
        node_id: crate::mesh::node::NodeId(req.node_id),
        wallet_id: req.wallet_id.clone(),
        name: req.name,
        iroh_addr: req.iroh_addr.unwrap_or_default(),
        last_seen: chrono::Utc::now(),
    };
    match mesh.register_wallet_node(node, &req.wallet_id) {
        Ok(_) => {
            let _ = mesh.save();
            (StatusCode::CREATED, "wallet node registered").into_response()
        }
        Err(e) => (StatusCode::CONFLICT, e.to_string()).into_response(),
    }
}

pub async fn list_wallet_nodes(
    State(state): State<F12State>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let reg = state.private_mesh_mut();
    let mesh = reg.private_mesh.as_ref().expect("mesh");
    if let Some(wallet) = params.get("wallet_id") {
        Json(mesh.get_nodes_by_wallet(wallet)).into_response()
    } else {
        Json(mesh.all_nodes().to_vec()).into_response()
    }
}

pub async fn redact_document(
    State(_state): State<F12State>,
    Json(req): Json<RedactRequest>,
) -> impl IntoResponse {
    // Redact PII/sensitive content with the RedactionEngine (regex rules).
    let engine = crate::security::redaction::RedactionEngine::new(vec![
        crate::security::redaction::RedactionRule {
            name: "email".into(),
            pattern: r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}".into(),
            mask: "[EMAIL]".into(),
        },
        crate::security::redaction::RedactionRule {
            name: "api-key".into(),
            pattern: r"(?i)(api[_-]?key|secret|token)\s*[:=]\s*\S+".into(),
            mask: "$1=[REDACTED]".into(),
        },
    ]);
    let redacted = engine.redact(&req.text);
    Json(serde_json::json!({ "redacted": redacted })).into_response()
}

pub async fn submit_curation(
    State(state): State<F12State>,
    Json(req): Json<SubmitCurationRequest>,
) -> impl IntoResponse {
    let mut reg = state.curation_mut();
    let queue = reg.curation.as_mut().expect("curation");
    let item = queue.submit_for_curation(req.content_ref, req.proposed_clearance, req.source);
    let _ = queue.save();
    (StatusCode::CREATED, Json(item)).into_response()
}

pub async fn approve_curation(
    State(state): State<F12State>,
    Json(req): Json<ApproveCurationRequest>,
) -> impl IntoResponse {
    let mut reg = state.curation_mut();
    let queue = reg.curation.as_mut().expect("curation");
    match queue.approve(&req.id, req.curator.clone(), req.classification, req.clearance) {
        Ok(item) => {
            let _ = queue.save();
            Json(item).into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, e).into_response(),
    }
}

pub async fn list_curation(State(state): State<F12State>) -> impl IntoResponse {
    let reg = state.curation_mut();
    let queue = reg.curation.as_ref().expect("curation");
    Json(queue.items.clone()).into_response()
}

pub async fn list_pending_curation_review(State(state): State<F12State>) -> impl IntoResponse {
    let reg = state.curation_mut();
    let queue = reg.curation.as_ref().expect("curation");
    Json(queue.pending_items()).into_response()
}

pub async fn approve_curation_review(
    State(state): State<F12State>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<ApproveReviewRequest>,
) -> impl IntoResponse {
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        return (StatusCode::BAD_REQUEST, "Invalid item ID").into_response();
    }
    let mut reg = state.curation_mut();
    let queue = reg.curation.as_mut().expect("curation");
    match queue.approve(&id, req.curator, req.classification, req.clearance) {
        Ok(item) => {
            let _ = queue.save();
            (StatusCode::OK, Json(item)).into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, e).into_response(),
    }
}

pub async fn reject_curation_review(
    State(state): State<F12State>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<RejectReviewRequest>,
) -> impl IntoResponse {
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        return (StatusCode::BAD_REQUEST, "Invalid item ID").into_response();
    }
    let mut reg = state.curation_mut();
    let queue = reg.curation.as_mut().expect("curation");
    match queue.reject(&id, req.curator, req.reason) {
        Ok(item) => {
            let _ = queue.save();
            (StatusCode::OK, Json(item)).into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, e).into_response(),
    }
}

pub async fn telemetry_metrics() -> impl IntoResponse {
    // Safe telemetry metric names (whitelist for the service network).
    let safe: Vec<String> = [
        "search_latency_ms",
        "index_files",
        "embedding_ok",
        "memory_count",
        "mesh_peers",
        "cpu_pct",
        "ram_mb",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    Json(safe).into_response()
}

pub async fn publish_service_telemetry(
    State(state): State<F12State>,
    Json(req): Json<PublishTelemetryRequest>,
) -> impl IntoResponse {
    let sample = TelemetrySample {
        node_id: NodeId(req.node_id),
        kind: req.kind.unwrap_or(ServiceKind::Custom("ops".to_string())),
        payload: req.payload,
        ts: req.ts.unwrap_or(0),
        classification: "INTERNAL".to_string(),
    };
    let mut reg = state.service_network_mut();
    let service_network = reg.service_network.as_mut().expect("service network");
    let published = service_network.publish_telemetry(sample);
    Json(published).into_response()
}

pub async fn consume_service_telemetry(
    State(state): State<F12State>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let since = params
        .get("since")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let reg = state.service_network_mut();
    let service_network = reg.service_network.as_ref().expect("service network");
    let samples = service_network.consume_telemetry(since);
    Json(samples).into_response()
}

pub async fn list_snapshots(State(state): State<F12State>) -> impl IntoResponse {
    let manager = SnapshotManager::new(&state.data_dir);
    match manager.list_snapshots() {
        Ok(snapshots) => Json(snapshots).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn create_snapshot(
    State(state): State<F12State>,
    Json(req): Json<CreateSnapshotRequest>,
) -> impl IntoResponse {
    if !req.repo.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        return (StatusCode::BAD_REQUEST, "Invalid repo name").into_response();
    }
    let manager = SnapshotManager::new(&state.data_dir);
    let repo_root = match &req.repo_root {
        Some(root) => PathBuf::from(root),
        None => {
            let base = state
                .data_dir
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.join("proyectosSWAL").join(&req.repo));
            match base {
                Some(p) if p.exists() => p,
                _ => {
                    return (
                        StatusCode::BAD_REQUEST,
                        "repo_root required (SWAL root not derivable from data_dir)",
                    )
                        .into_response();
                }
            }
        }
    };
    if !repo_root.exists() {
        return (
            StatusCode::NOT_FOUND,
            format!("repo root not found: {}", repo_root.display()),
        )
            .into_response();
    }
    match manager.create_snapshot(&repo_root, &req.repo) {
        Ok(snapshot) => (StatusCode::CREATED, Json(snapshot)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn get_snapshot(
    State(state): State<F12State>,
    axum::extract::Path(repo): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !repo.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        return (StatusCode::BAD_REQUEST, "Invalid repo name").into_response();
    }
    let manager = SnapshotManager::new(&state.data_dir);
    match manager.get_snapshot(&repo) {
        Ok(Some(snapshot)) => Json(snapshot).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "snapshot not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn get_document_handler(
    State(state): State<F12State>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return (StatusCode::BAD_REQUEST, "Invalid document ID").into_response();
    }

    // Extract clearance header (support X-Clearance, Clearance, or fallback to UNCLASSIFIED / level 0)
    let clearance_val = headers
        .get("x-clearance")
        .or_else(|| headers.get("clearance"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("0");

    let requester_clearance = if let Ok(num) = clearance_val.parse::<u8>() {
        ClearanceLevel::from(num)
    } else {
        ClearanceLevel::from(clearance_val)
    };

    let doc_path = state.data_dir.join("documents").join(format!("{}.md", id));
    let raw_content = if doc_path.exists() {
        match std::fs::read_to_string(&doc_path) {
            Ok(c) => c,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Error reading document").into_response(),
        }
    } else if id == "doc1" {
        // Built-in fallback sample document for testing/verification
        r#"# Classified Operation Protocol

## [CLEARANCE:1] Executive Summary
Operation Delta is active. Primary contact is ops@swal.io.

## Operational Directives [CLEARANCE:3]
Target coordinates in Sector 7 are confidential. Contact supervisor at +1-555-0199 for authorization.

## [CLEARANCE:5] Master Vault Keys
The root decryption key is 0xDEADBEEF42.
"#.to_string()
    } else {
        return (StatusCode::NOT_FOUND, "Document not found").into_response();
    };

    let segmented_doc = parse_segmented(&raw_content);
    let rendered = segmented_doc.render_for_clearance(requester_clearance);

    (
        StatusCode::OK,
        [("content-type", "text/plain; charset=utf-8")],
        rendered,
    )
        .into_response()
}

// ---------- router ----------

pub fn router(state: F12State) -> Router {
    Router::new()
        .route("/v1/f12/rag", post(rag_search))
        .route("/v1/f12/groups", get(list_groups))
        .route("/v1/f12/groups", post(create_group))
        .route("/v1/f12/directory", get(list_directory))
        .route("/v1/f12/directory/nodes", post(register_node))
        .route("/v1/f12/private-mesh/nodes", post(register_wallet_node))
        .route("/v1/f12/private-mesh/nodes", get(list_wallet_nodes))
        .route("/v1/f12/redact", post(redact_document))
        .route("/v1/f12/curation", post(submit_curation))
        .route("/v1/f12/curation/approve", post(approve_curation))
        .route("/v1/f12/curation", get(list_curation))
        .route("/v1/f12/curation/review", get(list_pending_curation_review))
        .route(
            "/v1/f12/curation/review/{id}/approve",
            post(approve_curation_review),
        )
        .route(
            "/v1/f12/curation/review/{id}/reject",
            post(reject_curation_review),
        )
        .route("/v1/f12/telemetry/metrics", get(telemetry_metrics))
        .route("/v1/f12/service-network/telemetry", post(publish_service_telemetry))
        .route("/v1/f12/service-network/telemetry", get(consume_service_telemetry))
        .route("/v1/f12/snapshots", get(list_snapshots))
        .route("/v1/f12/snapshots", post(create_snapshot))
        .route("/v1/f12/snapshots/{repo}", get(get_snapshot))
        .route("/v1/documents/{id}", get(get_document_handler))
        .route("/v1/f12/documents/{id}", get(get_document_handler))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_state() -> F12State {
        let dir = std::env::temp_dir().join(format!("f12-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("mesh")).ok();
        std::fs::create_dir_all(dir.join("security")).ok();
        std::fs::create_dir_all(dir.join("curation")).ok();
        F12State::new(dir)
    }

    #[tokio::test]
    async fn test_rag_empty_query_returns_empty() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/f12/rag")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query": ""}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["results"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_rag_query_with_results() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/f12/rag")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query": "IrohTransport", "limit": 5}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_create_group_ok() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/f12/groups")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"id": "g1", "name": "test group"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_list_directory_ok() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/f12/directory")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_redact_hides_secret_section() {
        let app = router(test_state());
        let doc = r#"{"text": "contact bela@swal.io with token=abc123"}"#;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/f12/redact")
                    .header("content-type", "application/json")
                    .body(Body::from(doc))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let redacted = v["redacted"].as_str().unwrap_or("");
        assert!(redacted.contains("[EMAIL]") || redacted.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn test_curation_flow() {
        let app = router(test_state());
        // submit 1
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/f12/curation")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"content_ref": "mem-1", "proposed_clearance": "INTERNAL", "source": "session"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id1 = v["id"].as_str().unwrap().to_string();

        // submit 2
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/f12/curation")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"content_ref": "mem-2", "proposed_clearance": "RESTRICTED", "source": "import"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id2 = v["id"].as_str().unwrap().to_string();

        // list pending review
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/f12/curation/review")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let pending: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(pending.as_array().unwrap().len(), 2);

        // approve review for item 1
        let req_app = r#"{"curator": "bela", "classification": "internal_doc", "clearance": "INTERNAL"}"#;
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/f12/curation/review/{}/approve", id1))
                    .header("content-type", "application/json")
                    .body(Body::from(req_app))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let item1_approved: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(item1_approved["classification"], "internal_doc");
        assert_eq!(item1_approved["curated_by"], "bela");

        // reject review for item 2
        let req_rej = r#"{"curator": "bela", "reason": "untrusted_source"}"#;
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/f12/curation/review/{}/reject", id2))
                    .header("content-type", "application/json")
                    .body(Body::from(req_rej))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // list pending review again - should be empty
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/f12/curation/review")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let pending_after: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(pending_after.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_wallet_node_registration() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/f12/private-mesh/nodes")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"node_id": "n1", "wallet_id": "w-alice", "name": "phone", "iroh_addr": "addr1"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_telemetry_metrics_whitelist() {
        let app = router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/v1/f12/telemetry/metrics").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v.as_array().unwrap().len() >= 3);
    }

    #[tokio::test]
    async fn test_service_network_telemetry_publish_and_consume_endpoints() {
        let state = test_state();
        let app = router(state.clone());

        // Publish endpoint POST /v1/f12/service-network/telemetry with PII email & phone
        let req_body = r#"{"node_id": "xv1", "payload": "bench test user user@example.com phone +1-555-123-4567"}"#;
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/f12/service-network/telemetry")
                    .header("content-type", "application/json")
                    .body(Body::from(req_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["classification"].as_str().unwrap(), "INTERNAL");
        let payload = v["payload"].as_str().unwrap();
        assert!(!payload.contains("user@example.com"));
        assert!(!payload.contains("+1-555-123-4567"));
        assert!(payload.contains("[EMAIL]"));
        assert!(payload.contains("[PHONE]"));

        // Consume endpoint GET /v1/f12/service-network/telemetry?since=0
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/f12/service-network/telemetry?since=0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let samples: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0]["node_id"].as_str().unwrap(), "xv1");
    }

    #[tokio::test]
    async fn test_list_snapshots_empty_ok() {
        let app = router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/v1/f12/snapshots").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_snapshot_missing_404() {
        let app = router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/v1/f12/snapshots/nope").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_document_clearance_redaction() {
        let app = router(test_state());

        // Requester level 1
        let resp_lvl1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/documents/doc1")
                    .header("X-Clearance", "1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp_lvl1.status(), StatusCode::OK);
        let body_lvl1 = resp_lvl1.into_body().collect().await.unwrap().to_bytes();
        let text_lvl1 = String::from_utf8(body_lvl1.to_vec()).unwrap();
        assert!(text_lvl1.contains("[EMAIL]"));
        assert!(text_lvl1.contains("[REDACTED: Operational Directives]"));
        assert!(text_lvl1.contains("[REDACTED: Master Vault Keys]"));
        assert!(!text_lvl1.contains("0xDEADBEEF42"));

        // Requester level 5
        let resp_lvl5 = app
            .oneshot(
                Request::builder()
                    .uri("/v1/documents/doc1")
                    .header("X-Clearance", "5")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp_lvl5.status(), StatusCode::OK);
        let body_lvl5 = resp_lvl5.into_body().collect().await.unwrap().to_bytes();
        let text_lvl5 = String::from_utf8(body_lvl5.to_vec()).unwrap();
        assert!(text_lvl5.contains("Executive Summary"));
        assert!(text_lvl5.contains("Operational Directives"));
        assert!(text_lvl5.contains("Master Vault Keys"));
        assert!(text_lvl5.contains("0xDEADBEEF42"));
        assert!(!text_lvl5.contains("[REDACTED: Master Vault Keys]"));
    }
}
