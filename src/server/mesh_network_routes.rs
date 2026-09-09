//! Axum REST routes for Mesh Network creation, management, and templates.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use qrcode::render::unicode;
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::enterprise::rbac::Permission;
use crate::mesh::network::{MeshNetwork, NetworkAcl};

/// Supported mesh network templates.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NetworkTemplate {
    Enterprise,
    Dao,
    Health,
}

impl NetworkTemplate {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Enterprise => "enterprise",
            Self::Dao => "dao",
            Self::Health => "health",
        }
    }

    /// Configure default ACL and attributes based on template.
    pub fn default_acl(&self) -> NetworkAcl {
        match self {
            Self::Enterprise => NetworkAcl {
                default_permission: None, // Zero-trust explicit grants required
                grants: Vec::new(),
            },
            Self::Dao => NetworkAcl {
                default_permission: Some(Permission::Read), // Open read for DAO members
                grants: Vec::new(),
            },
            Self::Health => NetworkAcl {
                default_permission: Some(Permission::Read), // Secure health data sharing
                grants: Vec::new(),
            },
        }
    }
}

/// A mesh network record with template and host information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkRecord {
    pub id: String,
    pub name: String,
    pub template: NetworkTemplate,
    pub is_host: bool,
    #[serde(default, alias = "public")]
    pub is_public: bool,
    pub owner_node: String,
    pub members: Vec<String>,
    pub acl: NetworkAcl,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request body for POST /v1/mesh/networks.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CreateMeshNetworkRequest {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub template: NetworkTemplate,
    #[serde(default)]
    pub is_host: bool,
    #[serde(default, alias = "public")]
    pub is_public: bool,
    #[serde(default)]
    pub owner_node: Option<String>,
}

/// Response for network details.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MeshNetworkResponse {
    pub id: String,
    pub name: String,
    pub template: NetworkTemplate,
    pub is_host: bool,
    #[serde(default, alias = "public")]
    pub is_public: bool,
    pub owner_node: String,
    pub members: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&NetworkRecord> for MeshNetworkResponse {
    fn from(rec: &NetworkRecord) -> Self {
        Self {
            id: rec.id.clone(),
            name: rec.name.clone(),
            template: rec.template,
            is_host: rec.is_host,
            is_public: rec.is_public,
            owner_node: rec.owner_node.clone(),
            members: rec.members.clone(),
            created_at: rec.created_at,
            updated_at: rec.updated_at,
        }
    }
}

/// Response for GET /v1/mesh/networks.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ListMeshNetworksResponse {
    pub networks: Vec<MeshNetworkResponse>,
    pub total: usize,
}

/// Response for GET /v1/mesh/networks/:id/invite.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NetworkInviteResponse {
    pub network_id: String,
    pub network_name: String,
    pub template: NetworkTemplate,
    pub token: String,
    pub qr_code: String,
    pub expires_at: DateTime<Utc>,
}

/// Request body for POST /v1/mesh/networks/join and POST /v1/mesh/networks/{id}/join.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct JoinNetworkRequest {
    pub token: String,
    #[serde(default)]
    pub node_id: Option<String>,
}

/// Request body for POST /v1/mesh/networks/accept.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AcceptInviteRequest {
    pub token: String,
    #[serde(default)]
    pub node_id: Option<String>,
}

/// Internal thread-safe store for mesh networks.
#[derive(Debug, Clone)]
pub struct MeshNetworkStore {
    file_path: Option<PathBuf>,
    networks: Arc<RwLock<HashMap<String, NetworkRecord>>>,
}

impl MeshNetworkStore {
    pub fn new(file_path: Option<PathBuf>) -> Self {
        let store = Self {
            file_path,
            networks: Arc::new(RwLock::new(HashMap::new())),
        };
        store.load();
        store
    }

    fn load(&self) {
        if let Some(ref path) = self.file_path {
            if path.exists() {
                if let Ok(data) = std::fs::read_to_string(path) {
                    if let Ok(records) =
                        serde_json::from_str::<HashMap<String, NetworkRecord>>(&data)
                    {
                        if let Ok(mut lock) = self.networks.write() {
                            *lock = records;
                        }
                    }
                }
            }
        }
    }

    fn save(&self) {
        if let Some(ref path) = self.file_path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(lock) = self.networks.read() {
                if let Ok(data) = serde_json::to_string_pretty(&*lock) {
                    let _ = std::fs::write(path, data);
                }
            }
        }
    }

    pub fn create_network(
        &self,
        id: String,
        name: String,
        template: NetworkTemplate,
        is_host: bool,
        is_public: bool,
        owner_node: String,
    ) -> Result<NetworkRecord, String> {
        let mut lock = self.networks.write().map_err(|e| e.to_string())?;
        if lock.contains_key(&id) {
            return Err(format!("Network with id '{}' already exists", id));
        }

        let now = Utc::now();
        let record = NetworkRecord {
            id: id.clone(),
            name,
            template,
            is_host,
            is_public,
            owner_node: owner_node.clone(),
            members: vec![owner_node],
            acl: template.default_acl(),
            created_at: now,
            updated_at: now,
        };

        lock.insert(id, record.clone());
        drop(lock);
        self.save();
        Ok(record)
    }

    pub fn list_networks(&self) -> Vec<NetworkRecord> {
        if let Ok(lock) = self.networks.read() {
            lock.values().cloned().collect()
        } else {
            Vec::new()
        }
    }

    pub fn get_network(&self, id: &str) -> Option<NetworkRecord> {
        if let Ok(lock) = self.networks.read() {
            lock.get(id).cloned()
        } else {
            None
        }
    }

    pub fn add_member(&self, id: &str, node_id: String) -> Result<NetworkRecord, String> {
        let mut lock = self.networks.write().map_err(|e| e.to_string())?;
        let record = lock
            .get_mut(id)
            .ok_or_else(|| format!("Network '{}' not found", id))?;
        if !record.members.contains(&node_id) {
            record.members.push(node_id);
            record.updated_at = Utc::now();
        }
        let record_cloned = record.clone();
        drop(lock);
        self.save();
        Ok(record_cloned)
    }
}

/// Shared state for mesh network routes.
#[derive(Debug, Clone)]
pub struct MeshNetworkState {
    pub store: MeshNetworkStore,
}

impl MeshNetworkState {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            store: MeshNetworkStore::new(Some(data_dir.join("mesh/networks.json"))),
        }
    }

    pub fn in_memory() -> Self {
        Self {
            store: MeshNetworkStore::new(None),
        }
    }
}

/// Handler for POST /v1/mesh/networks.
pub async fn create_network(
    State(state): State<MeshNetworkState>,
    Json(payload): Json<CreateMeshNetworkRequest>,
) -> impl IntoResponse {
    if payload.name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Network name is required" })),
        )
            .into_response();
    }

    let network_id = payload
        .id
        .unwrap_or_else(|| format!("net-{}", ulid::Ulid::new().to_string().to_lowercase()));
    let owner_node = payload
        .owner_node
        .unwrap_or_else(|| "node-self".to_string());

    match state.store.create_network(
        network_id,
        payload.name,
        payload.template,
        payload.is_host,
        payload.is_public,
        owner_node,
    ) {
        Ok(record) => (
            StatusCode::CREATED,
            Json(MeshNetworkResponse::from(&record)),
        )
            .into_response(),
        Err(err) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": err })),
        )
            .into_response(),
    }
}

/// Handler for GET /v1/mesh/networks.
pub async fn list_networks(State(state): State<MeshNetworkState>) -> impl IntoResponse {
    let records = state.store.list_networks();
    let networks: Vec<MeshNetworkResponse> =
        records.iter().map(MeshNetworkResponse::from).collect();
    let total = networks.len();

    (
        StatusCode::OK,
        Json(ListMeshNetworksResponse { networks, total }),
    )
        .into_response()
}

/// Handler for GET /v1/mesh/networks/:id/invite.
pub async fn generate_invite(
    State(state): State<MeshNetworkState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(record) = state.store.get_network(&id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Network '{}' not found", id) })),
        )
            .into_response();
    };

    let expires_at = Utc::now() + chrono::Duration::hours(24);
    let raw_token = format!(
        "xavier:invite:v1:{}:{}:{}:{}",
        record.id,
        record.template.as_str(),
        expires_at.timestamp(),
        ulid::Ulid::new().to_string().to_lowercase()
    );

    let qr_code_str = match QrCode::new(raw_token.as_bytes()) {
        Ok(qr) => qr.render::<unicode::Dense1x2>().build(),
        Err(_) => String::new(),
    };

    (
        StatusCode::OK,
        Json(NetworkInviteResponse {
            network_id: record.id,
            network_name: record.name,
            template: record.template,
            token: raw_token,
            qr_code: qr_code_str,
            expires_at,
        }),
    )
        .into_response()
}

/// Helper function to parse and validate network invite tokens.
pub fn parse_and_validate_invite_token(token: &str) -> Result<(String, i64), String> {
    let parts: Vec<&str> = token.split(':').collect();
    if parts.len() < 7 || parts[0] != "xavier" || parts[1] != "invite" || parts[2] != "v1" {
        return Err("Invalid invite token format".to_string());
    }
    let network_id = parts[3].to_string();
    let expires_at_ts: i64 = parts[5]
        .parse()
        .map_err(|_| "Invalid expiration timestamp in invite token".to_string())?;

    if Utc::now().timestamp() > expires_at_ts {
        return Err("Invite token expired".to_string());
    }

    Ok((network_id, expires_at_ts))
}

/// Handler for POST /v1/mesh/networks/join.
pub async fn join_network(
    State(state): State<MeshNetworkState>,
    Json(payload): Json<JoinNetworkRequest>,
) -> impl IntoResponse {
    let (net_id, _) = match parse_and_validate_invite_token(&payload.token) {
        Ok(res) => res,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": err }))).into_response();
        }
    };

    let node_id = payload
        .node_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "node-self".to_string());

    match state.store.add_member(&net_id, node_id) {
        Ok(record) => (StatusCode::OK, Json(MeshNetworkResponse::from(&record))).into_response(),
        Err(err) => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": err }))).into_response(),
    }
}

/// Handler for POST /v1/mesh/networks/:id/join.
pub async fn join_network_by_id(
    State(state): State<MeshNetworkState>,
    Path(path_id): Path<String>,
    Json(payload): Json<JoinNetworkRequest>,
) -> impl IntoResponse {
    let (net_id, _) = match parse_and_validate_invite_token(&payload.token) {
        Ok(res) => res,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": err }))).into_response();
        }
    };

    if net_id != path_id {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Token network '{}' does not match path network '{}'", net_id, path_id) })),
        )
            .into_response();
    }

    let node_id = payload
        .node_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "node-self".to_string());

    match state.store.add_member(&net_id, node_id) {
        Ok(record) => (StatusCode::OK, Json(MeshNetworkResponse::from(&record))).into_response(),
        Err(err) => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": err }))).into_response(),
    }
}

/// Handler for POST /v1/mesh/networks/accept.
pub async fn accept_invite(
    State(state): State<MeshNetworkState>,
    Json(payload): Json<AcceptInviteRequest>,
) -> impl IntoResponse {
    let (net_id, _) = match parse_and_validate_invite_token(&payload.token) {
        Ok(res) => res,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": err }))).into_response();
        }
    };

    let node_id = payload
        .node_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "node-self".to_string());

    match state.store.add_member(&net_id, node_id) {
        Ok(record) => (StatusCode::OK, Json(MeshNetworkResponse::from(&record))).into_response(),
        Err(err) => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": err }))).into_response(),
    }
}

/// Build router for mesh network REST endpoints.
pub fn router(state: MeshNetworkState) -> Router {
    Router::new()
        .route("/v1/mesh/networks", post(create_network).get(list_networks))
        .route("/v1/mesh/networks/{id}/invite", get(generate_invite))
        .route("/v1/mesh/networks/join", post(join_network))
        .route("/v1/mesh/networks/{id}/join", post(join_network_by_id))
        .route("/v1/mesh/networks/accept", post(accept_invite))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn test_mesh_network_crud_and_templates() {
        let state = MeshNetworkState::in_memory();
        let app = router(state);

        // 1. List networks initially empty
        let req = Request::builder()
            .method("GET")
            .uri("/v1/mesh/networks")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let list_resp: ListMeshNetworksResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list_resp.total, 0);
        assert!(list_resp.networks.is_empty());

        // 2. Create Enterprise Network (is_host = true)
        let create_ent_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/networks")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "id": "net-enterprise-01",
                    "name": "Enterprise HQ Mesh",
                    "template": "enterprise",
                    "is_host": true
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(create_ent_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let ent_resp: MeshNetworkResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(ent_resp.id, "net-enterprise-01");
        assert_eq!(ent_resp.name, "Enterprise HQ Mesh");
        assert_eq!(ent_resp.template, NetworkTemplate::Enterprise);
        assert!(ent_resp.is_host);

        // 3. Create DAO Network (is_host = false)
        let create_dao_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/networks")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "name": "Global Governance DAO",
                    "template": "dao",
                    "is_host": false
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(create_dao_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let dao_resp: MeshNetworkResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(dao_resp.template, NetworkTemplate::Dao);
        assert!(!dao_resp.is_host);

        // 4. Create Health Network
        let create_health_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/networks")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "name": "BioHealth Mesh Node",
                    "template": "health",
                    "is_host": true
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(create_health_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let health_resp: MeshNetworkResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(health_resp.template, NetworkTemplate::Health);

        // 5. List networks returns 3 networks
        let list_req = Request::builder()
            .method("GET")
            .uri("/v1/mesh/networks")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(list_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let list_resp: ListMeshNetworksResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list_resp.total, 3);
        assert_eq!(list_resp.networks.len(), 3);

        // 6. Generate invite for existing network
        let invite_req = Request::builder()
            .method("GET")
            .uri("/v1/mesh/networks/net-enterprise-01/invite")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(invite_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let invite_resp: NetworkInviteResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(invite_resp.network_id, "net-enterprise-01");
        assert_eq!(invite_resp.template, NetworkTemplate::Enterprise);
        assert!(invite_resp
            .token
            .starts_with("xavier:invite:v1:net-enterprise-01:enterprise:"));
        assert!(!invite_resp.qr_code.is_empty());

        // 7. Generate invite for non-existent network returns 404
        let missing_invite_req = Request::builder()
            .method("GET")
            .uri("/v1/mesh/networks/non-existent-net/invite")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(missing_invite_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // 8. Join network via /v1/mesh/networks/join
        let join_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/networks/join")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "token": invite_resp.token,
                    "node_id": "node-peer-1"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(join_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let join_resp: MeshNetworkResponse = serde_json::from_slice(&body).unwrap();
        assert!(join_resp.members.contains(&"node-peer-1".to_string()));

        // 9. Accept invite via /v1/mesh/networks/accept
        let accept_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/networks/accept")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "token": invite_resp.token,
                    "node_id": "node-peer-2"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(accept_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let accept_resp: MeshNetworkResponse = serde_json::from_slice(&body).unwrap();
        assert!(accept_resp.members.contains(&"node-peer-2".to_string()));

        // 10. Join by network ID /v1/mesh/networks/:id/join
        let join_by_id_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/networks/net-enterprise-01/join")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "token": invite_resp.token,
                    "node_id": "node-peer-3"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(join_by_id_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let join_by_id_resp: MeshNetworkResponse = serde_json::from_slice(&body).unwrap();
        assert!(join_by_id_resp.members.contains(&"node-peer-3".to_string()));

        // 11. Create public network and verify public flag
        let create_pub_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/networks")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "id": "net-public-01",
                    "name": "Public Mesh Commons",
                    "template": "dao",
                    "is_host": false,
                    "public": true
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(create_pub_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let pub_resp: MeshNetworkResponse = serde_json::from_slice(&body).unwrap();
        assert!(pub_resp.is_public);

        // 12. Invalid/expired invite token returns 400
        let invalid_join_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/networks/join")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "token": "invalid:token:format",
                    "node_id": "node-bad"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(invalid_join_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
