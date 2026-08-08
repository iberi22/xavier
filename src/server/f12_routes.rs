//! REST routes for F12 preservation features.
//!
//! Exposes the F12 modules (public directory, public RAG, groups, redaction,
//! curation, private mesh, service network) over HTTP under `/v1/f12/*`.
//! These modules are pure libraries; this file adds the HTTP surface.

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::codebase::snapshot::SnapshotManager;
use crate::curation::CurationQueue;
use crate::mesh::private_mesh::{PrivateMeshRegistry, WalletNode};
use crate::mesh::public_directory::PublicDirectory;
use crate::mesh::public_rag::{search_public, PublicRagResult};
use crate::mesh::service_network::ServiceKind;
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
}

#[derive(Debug, Deserialize)]
pub struct ApproveCurationRequest {
    pub id: String,
    pub curator: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSnapshotRequest {
    pub repo: String,
    pub repo_root: Option<String>,
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
    let item = queue.submit_for_curation(req.content_ref, req.proposed_clearance);
    let _ = queue.save();
    (StatusCode::CREATED, Json(item)).into_response()
}

pub async fn approve_curation(
    State(state): State<F12State>,
    Json(req): Json<ApproveCurationRequest>,
) -> impl IntoResponse {
    let mut reg = state.curation_mut();
    let queue = reg.curation.as_mut().expect("curation");
    match queue.approve(&req.id, req.curator.clone()) {
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
    let manager = SnapshotManager::new(&state.data_dir);
    match manager.get_snapshot(&repo) {
        Ok(Some(snapshot)) => Json(snapshot).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "snapshot not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// ---------- issue-context-packager ----------

/// Request body for POST /v1/f12/issue-context
#[derive(Deserialize)]
pub struct IssueContextRequest {
    /// GitHub issue number (as string).
    pub issue_id: String,
    /// Issue title.
    pub title: String,
    /// Repository name (owner/repo).
    pub repo: String,
    /// Issue body (markdown).
    pub body: String,
    /// Optional: repo root path (defaults to data_dir/repos/{repo}).
    pub repo_root: Option<String>,
}

/// POST /v1/f12/issue-context — generate an IssueContextPackage from a GitHub issue.
///
/// Analyzes the issue body, maps entities to the CodeGraph, and returns
/// PreciseChange objects that an executor agent can apply directly.
pub async fn issue_context(
    State(state): State<F12State>,
    Json(req): Json<IssueContextRequest>,
) -> impl IntoResponse {
    use crate::codebase::issue_context;

    let repo_root = req
        .repo_root
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| state.data_dir.join("repos").join(&req.repo));

    if !repo_root.exists() {
        return (
            StatusCode::BAD_REQUEST,
            format!("repo_root not found: {:?}", repo_root),
        )
            .into_response();
    }

    // Open the code graph DB
    let db_path = crate::codebase::codegraph_paths::code_graph_db_path_for(&repo_root);
    let code_graph_db = match code_graph::db::CodeGraphDB::new(&db_path) {
        Ok(db) => db,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to open CodeGraph DB: {}", e),
            )
                .into_response()
        }
    };

    let snapshot_manager = SnapshotManager::new(&state.data_dir);

    match issue_context::assemble_package(
        &req.issue_id,
        &req.title,
        &req.repo,
        &req.body,
        &code_graph_db,
        &snapshot_manager,
        &repo_root,
    ) {
        Ok(package) => (StatusCode::OK, Json(package)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to assemble package: {}", e),
        )
            .into_response(),
    }
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
        .route("/v1/f12/telemetry/metrics", get(telemetry_metrics))
        .route("/v1/f12/snapshots", get(list_snapshots))
        .route("/v1/f12/snapshots", post(create_snapshot))
        .route("/v1/f12/snapshots/{repo}", get(get_snapshot))
        .route("/v1/f12/issue-context", post(issue_context))
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
        // submit
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/f12/curation")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"content_ref": "mem-1", "proposed_clearance": "INTERNAL"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = v["id"].as_str().unwrap().to_string();
        // list
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/f12/curation")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // approve
        let req = format!(r#"{{"id": "{}", "curator": "bela"}}"#, id);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/f12/curation/approve")
                    .header("content-type", "application/json")
                    .body(Body::from(req))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
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
            .oneshot(
                Request::builder()
                    .uri("/v1/f12/telemetry/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v.as_array().unwrap().len() >= 3);
    }

    #[tokio::test]
    async fn test_list_snapshots_empty_ok() {
        let app = router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/f12/snapshots")
                    .body(Body::empty())
                    .unwrap(),
            )
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
            .oneshot(
                Request::builder()
                    .uri("/v1/f12/snapshots/nope")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
