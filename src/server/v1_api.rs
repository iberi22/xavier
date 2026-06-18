//! V1 RESTful Standard Memory API handlers.

use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::{
    memory::{
        qmd_memory::query_with_embedding_filtered,
        schema::{
            ContextZone, EvidenceKind, MemoryKind, MemoryNamespace, MemoryProvenance,
            MemoryQueryFilters, TypedMemoryPayload,
        },
    },
    mesh::{
        protocol::{ChunkRef, MeshHandshake, MeshHandshakeResponse, MeshManifest, MeshSyncRequest},
        NodeIdentity,
        PeerRegistry,
    },
    session::sharing::{export_session, import_session, SessionBundle},
    sync::SyncTransport,
    workspace::WorkspaceContext,
};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct V1Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct V1AddMemoryRequest {
    pub messages: Option<Vec<V1Message>>,
    pub text: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub user_id: Option<String>,
    pub kind: Option<MemoryKind>,
    pub evidence_kind: Option<EvidenceKind>,
    pub namespace: Option<MemoryNamespace>,
    pub provenance: Option<MemoryProvenance>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct V1MemoryResponse {
    pub id: String,
    pub memory: String,
    pub user_id: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct V1PaginationParams {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct V1PaginationMetadata {
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct V1MemoryListResponse {
    pub memories: Vec<V1MemoryResponse>,
    pub pagination: V1PaginationMetadata,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct V1SearchRequest {
    pub query: String,
    pub limit: Option<usize>,
    pub filters: Option<MemoryQueryFilters>,
    pub active_zones: Option<Vec<ContextZone>>,
}

#[derive(Debug, Serialize, Clone)]
pub struct V1MemorySearchResponse {
    pub status: String,
    pub results: Vec<V1MemoryResponse>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct V1ExportParams {
    pub public: Option<bool>,
}

pub async fn v1_memories_export(
    Extension(workspace): Extension<WorkspaceContext>,
    Query(params): Query<V1ExportParams>,
) -> impl IntoResponse {
    let public_only = params.public.unwrap_or(false);
    match workspace.workspace.memory.export(public_only).await {
        Ok(docs) => Json(docs).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": e.to_string()
            })),
        )
            .into_response(),
    }
}

fn is_primary_memory(metadata: &serde_json::Value) -> bool {
    metadata.get("source_path").is_none()
}

pub async fn v1_memories_add(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<V1AddMemoryRequest>,
) -> impl IntoResponse {
    info!(
        user_id = payload.user_id.as_deref().unwrap_or("default"),
        "v1_memories_add"
    );

    let content = if let Some(t) = payload.text {
        t
    } else if let Some(m) = payload.messages {
        m.into_iter()
            .map(|msg| format!("{}: {}", msg.role, msg.content))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };
    let content_for_graph = content.clone();

    let path = payload
        .user_id
        .clone()
        .unwrap_or_else(|| "default".to_string());
    let mut meta = payload.metadata.unwrap_or(serde_json::json!({}));
    let mut namespace = payload.namespace;
    if let Some(uid) = payload.user_id {
        meta["user_id"] = serde_json::json!(uid);
        if namespace
            .as_ref()
            .and_then(|value| value.user_id.as_ref())
            .is_none()
        {
            let mut value = namespace.unwrap_or_default();
            value.user_id = meta
                .get("user_id")
                .and_then(|id| id.as_str())
                .map(|id| id.to_string());
            namespace = Some(value);
        }
    }
    let meta_for_graph = meta.clone();

    if let Err(error) = workspace
        .workspace
        .ensure_within_storage_limit(&path, &content, &meta)
        .await
    {
        return Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        }));
    }

    match workspace
        .workspace
        .memory
        .add_document_typed(
            path,
            content,
            meta,
            Some(TypedMemoryPayload {
                kind: payload.kind,
                evidence_kind: payload.evidence_kind,
                namespace,
                provenance: payload.provenance,
                ..Default::default()
            }),
        )
        .await
    {
        Ok(id) => {
            if let Err(error) = workspace
                .workspace
                .index_memory_entities(&id, &content_for_graph, &meta_for_graph)
                .await
            {
                tracing::warn!(%error, memory_id = %id, "failed to index entity graph from v1 add");
            }
            Json(serde_json::json!({
                "status": "ok",
                "message": "Memory added successfully",
                "id": id,
            }))
        }
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": e.to_string(),
        })),
    }
}

// ── Mesh API Handlers ──────────────────────────────────────────────────────

pub async fn v1_mesh_identity(
    Extension(_workspace): Extension<WorkspaceContext>,
) -> impl IntoResponse {
    // License check
    let settings = crate::settings::XavierSettings::current();
    if let Err(e) = crate::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    match NodeIdentity::load_or_create() {
        Ok(identity) => Json(identity.public_info()).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn v1_mesh_handshake(
    Extension(_workspace): Extension<WorkspaceContext>,
    Json(payload): Json<MeshHandshake>,
) -> impl IntoResponse {
    // License check
    let settings = crate::settings::XavierSettings::current();
    if let Err(e) = crate::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "accepted": false, "reason": e })),
        )
            .into_response();
    }

    info!("Received mesh handshake from {}", payload.node_id);

    // 1. Verify Signature
    let Ok(public_key) = hex::decode(&payload.public_key_hex) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "accepted": false, "reason": "Invalid public key hex" })),
        )
            .into_response();
    };

    let Ok(signature) = hex::decode(&payload.signature_hex) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "accepted": false, "reason": "Invalid signature hex" })),
        )
            .into_response();
    };

    if !NodeIdentity::verify(&public_key, payload.nonce.as_bytes(), &signature) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "accepted": false, "reason": "Invalid signature" })),
        )
            .into_response();
    }

    // 2. Verify Pairing Secret if provided
    let mut auto_register = false;
    if let Some(secret) = payload.pairing_secret {
        match crate::mesh::pairing_registry::PairingSecretRegistry::load() {
            Ok(mut registry) => {
                match registry.verify_and_remove(&secret) {
                    Ok(true) => {
                        info!("Pairing secret verified for node {}", payload.node_id);
                        auto_register = true;
                    }
                    Ok(false) => {
                        return (
                            StatusCode::UNAUTHORIZED,
                            Json(serde_json::json!({ "accepted": false, "reason": "Invalid or expired pairing secret" })),
                        ).into_response();
                    }
                    Err(e) => {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({ "accepted": false, "reason": format!("Secret registry error: {}", e) })),
                        ).into_response();
                    }
                }
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "accepted": false, "reason": format!("Failed to load secret registry: {}", e) })),
                ).into_response();
            }
        }
    }

    match NodeIdentity::load_or_create() {
        Ok(identity) => {
            if auto_register {
                // Automatically add to PeerRegistry and MeshAcl
                if let Ok(mut peers) = crate::mesh::PeerRegistry::load() {
                    let _ = peers.add_peer(crate::mesh::PeerInfo {
                        node_id: payload.node_id.clone(),
                        alias: None,
                        endpoint_url: String::new(), // We don't necessarily know their endpoint yet
                        public_key_hex: payload.public_key_hex.clone(),
                        added_at: chrono::Utc::now().timestamp(),
                        last_seen_at: Some(chrono::Utc::now().timestamp()),
                        sync_enabled: true,
                        is_cloud: false,
                    });
            info!("Auto-registered peer {} in PeerRegistry", payload.node_id);
                }

                if let Ok(mut acl) = crate::mesh::MeshAcl::load() {
                    let _ = acl.set_entry(payload.node_id.clone(), crate::mesh::NodeAclEntry {
                        role: crate::enterprise::rbac::Role::Viewer,
                        clearance: crate::memory::schema::ClearanceLevel::Unclassified,
                        namespaces: None,
                        public_key_hex: payload.public_key_hex.clone(),
                    });
            info!("Auto-registered peer {} in MeshAcl", payload.node_id);
                }
            }

            let response = MeshHandshakeResponse {
                accepted: true,
                node_id: identity.node_id.clone(),
                public_key_hex: hex::encode(&identity.public_key),
                reason: None,
            };
            Json(response).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "accepted": false, "reason": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn v1_mesh_manifest(
    Extension(workspace): Extension<WorkspaceContext>,
    Query(payload): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    // License check
    let settings = crate::settings::XavierSettings::current();
    if let Err(e) = crate::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    let node_id_str = payload.get("node_id");
    let timestamp_str = payload.get("timestamp");
    let nonce = payload.get("nonce");
    let signature_hex = payload.get("signature");

    let acl = crate::mesh::acl::MeshAcl::load().unwrap_or_else(|e| {
        tracing::error!("Failed to load mesh ACL: {}", e);
        crate::mesh::acl::MeshAcl::load_from(std::path::PathBuf::from("/tmp/mesh_acl.json")).unwrap()
    });

    let (clearance, namespaces) = if let Some(id) = node_id_str {
        if let Some(entry) = acl.get_entry(&crate::mesh::node::NodeId(id.clone())) {
            // VERIFY SIGNATURE
            if let (Some(ts), Some(n), Some(sig)) = (timestamp_str, nonce, signature_hex) {
                let message = format!("{}:{}", ts, n);
                let pubkey = hex::decode(&entry.public_key_hex).unwrap_or_default();
                let signature = hex::decode(sig).unwrap_or_default();
                if !NodeIdentity::verify(&pubkey, message.as_bytes(), &signature) {
                    return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "Invalid signature" }))).into_response();
                }
            } else {
                return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "Missing auth headers" }))).into_response();
            }

            info!("Manifest request: NodeId={} verified and found in ACL", id);
            (entry.clearance, entry.namespaces.clone())
        } else {
            info!("Manifest request: NodeId={} NOT found in ACL, denying access", id);
            (
                crate::memory::schema::ClearanceLevel::Unclassified,
                Some(vec![]), // Secure-by-default: deny all namespaces
            )
        }
    } else {
        info!("Manifest request: No NodeId provided, denying access");
        (
            crate::memory::schema::ClearanceLevel::Unclassified,
            Some(vec![]),
        )
    };

    match NodeIdentity::load_or_create() {
        Ok(identity) => {
            // Use existing chunking logic from src/sync/chunks.rs
            let sync_dir = workspace
                .workspace
                .usage_state_path
                .parent()
                .unwrap_or(&workspace.workspace.usage_state_path)
                .join("sync");
            let _ = std::fs::create_dir_all(&sync_dir);

            match crate::sync::chunks::load_manifest(&sync_dir) {
                Ok(chunk_manifest) => {
                    let mut chunks = Vec::new();
                    for c in chunk_manifest.chunks.values() {
                        // Filter chunks: only include if at least one doc is authorized
                        if let Ok(docs) = crate::sync::chunks::import_from_chunk(&sync_dir, &c.hash) {
                            let all_authorized = docs.iter().all(|doc| {
                                let clearance_ok = doc.clearance <= clearance;
                                let namespace_ok = if let Some(ref ns_list) = namespaces {
                                    let doc_project = doc.metadata.get("namespace")
                                        .and_then(|v| v.get("project"))
                                        .and_then(|v| v.as_str())
                                        .or_else(|| {
                                            doc.metadata.get("project")
                                                .and_then(|v| v.as_str())
                                        })
                                        .map(|s| s.to_string());

                                    if let Some(ref doc_ns) = doc_project {
                                        let matched = ns_list.contains(doc_ns);
                                        tracing::debug!("Namespace check: doc_ns={}, ns_list={:?}, matched={}", doc_ns, ns_list, matched);
                                        matched
                                    } else {
                                        tracing::debug!("Namespace check: doc has no project, ns_list={:?}, matched=false, metadata={:?}", ns_list, doc.metadata);
                                        false // Restricted but doc has no namespace
                                    }
                                } else {
                                    true // No restriction
                                };
                                clearance_ok && namespace_ok
                            });

                            if all_authorized {
                                chunks.push(ChunkRef {
                                    hash: c.hash.clone(),
                                    document_count: c.document_ids.len(),
                                    created_at: c.created_at,
                                });
                            }
                        }
                    }

                    let manifest = MeshManifest {
                        node_id: identity.node_id,
                        chunks,
                        generated_at: chrono::Utc::now().timestamp(),
                    };
                    Json(manifest).into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn v1_mesh_chunks_request(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<MeshSyncRequest>,
) -> impl IntoResponse {
    // License check
    let settings = crate::settings::XavierSettings::current();
    if let Err(e) = crate::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    use std::collections::HashMap;

    let acl = crate::mesh::acl::MeshAcl::load().unwrap_or_else(|e| {
        tracing::error!("Failed to load mesh ACL: {}", e);
        crate::mesh::acl::MeshAcl::load_from(std::path::PathBuf::from("/tmp/mesh_acl.json")).unwrap()
    });

    let (clearance, namespaces) = if let Some(entry) = acl.get_entry(&payload.requesting_node_id) {
        // VERIFY SIGNATURE
        let message = format!("{}:{}", payload.timestamp, payload.nonce);
        let pubkey = hex::decode(&entry.public_key_hex).unwrap_or_default();
        let signature = hex::decode(&payload.signature_hex).unwrap_or_default();
        if !NodeIdentity::verify(&pubkey, message.as_bytes(), &signature) {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "Invalid signature" }))).into_response();
        }

        (entry.clearance, entry.namespaces.clone())
    } else {
        (crate::memory::schema::ClearanceLevel::Unclassified, Some(vec![]))
    };

    let mut response_chunks = HashMap::new();
    let sync_dir = workspace
        .workspace
        .usage_state_path
        .parent()
        .unwrap_or(&workspace.workspace.usage_state_path)
        .join("sync");

    for hash in payload.wanted_hashes {
        if let Ok(docs) = crate::sync::chunks::import_from_chunk(&sync_dir, &hash) {
            let all_authorized = docs.iter().all(|doc| {
                let clearance_ok = doc.clearance <= clearance;
                let namespace_ok = if let Some(ref ns_list) = namespaces {
                    if let Some(ref doc_ns) = doc.metadata.get("namespace").and_then(|v| v.get("project")).and_then(|v| v.as_str()) {
                        ns_list.contains(&doc_ns.to_string())
                    } else {
                        false
                    }
                } else {
                    true
                };
                clearance_ok && namespace_ok
            });
            if all_authorized {
                let chunk_path = sync_dir.join("chunks").join(format!("{}.jsonl.gz", hash));
                if let Ok(data) = std::fs::read(chunk_path) {
                    response_chunks.insert(hash, data);
                }
            }
        }
    }

    Json(response_chunks).into_response()
}

pub async fn v1_mesh_chunks_push(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(chunks): Json<std::collections::HashMap<String, Vec<u8>>>,
) -> impl IntoResponse {
    // License check
    let settings = crate::settings::XavierSettings::current();
    if let Err(e) = crate::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    let mut synced_hashes = Vec::new();
    let sync_dir = workspace
        .workspace
        .usage_state_path
        .parent()
        .unwrap_or(&workspace.workspace.usage_state_path)
        .join("sync");
    let chunks_dir = sync_dir.join("chunks");
    let _ = std::fs::create_dir_all(&chunks_dir);

    for (hash, data) in chunks {
        let chunk_path = chunks_dir.join(format!("{}.jsonl.gz", hash));
        if std::fs::write(&chunk_path, &data).is_ok() {
            // Import documents from chunk into local memory
            if let Ok(docs) = crate::sync::chunks::import_from_chunk(&sync_dir, &hash) {
                for doc in docs {
                    let _ = workspace
                        .workspace
                        .memory
                        .add_document_typed(doc.path, doc.content, doc.metadata, None)
                        .await;
                }
                synced_hashes.push(hash);
            }
        }
    }

    Json(synced_hashes).into_response()
}

// ── Session Sharing API ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct V1SessionShareRequest {
    pub peer_node_id: String,
}

pub async fn v1_session_export(
    Extension(workspace): Extension<WorkspaceContext>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    // Session export/import is often used for mesh sharing, but can be standalone.
    // However, the design docs link it to the Mesh License when used for network sharing.
    // We'll gate it if it's part of the Mesh/V1 API surface.
    let settings = crate::settings::XavierSettings::current();
    if let Err(e) = crate::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    match export_session(&workspace.workspace.memory, &session_id).await {
        Ok(bundle) => Json(bundle).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn v1_session_import(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(bundle): Json<SessionBundle>,
) -> impl IntoResponse {
    // License check
    let settings = crate::settings::XavierSettings::current();
    if let Err(e) = crate::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    match import_session(&workspace.workspace.memory, bundle).await {
        Ok(_) => Json(serde_json::json!({ "status": "ok" })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn v1_mesh_session_share(
    Extension(workspace): Extension<WorkspaceContext>,
    Path(session_id): Path<String>,
    Json(payload): Json<V1SessionShareRequest>,
) -> impl IntoResponse {
    // License check
    let settings = crate::settings::XavierSettings::current();
    if let Err(e) = crate::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    let registry = match PeerRegistry::load() {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };

    let peer_node_id = crate::mesh::node::NodeId(payload.peer_node_id);
    let peer = match registry.get_peer(&peer_node_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Peer not found" })),
            )
                .into_response()
        }
    };

    let bundle = match export_session(&workspace.workspace.memory, &session_id).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };

    let identity = match NodeIdentity::load_or_create() {
        Ok(id) => Arc::new(id),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };

    let transport = match SyncTransport::for_peer(peer, identity.clone()) {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };

    // Note: We need a token to talk to the peer. For now, assume we use the local token
    // or a specialized mesh token if available in PeerInfo.
    let token = std::env::var("XAVIER_TOKEN").unwrap_or_default();

    match transport.share_session(peer, &token, bundle).await {
        Ok(_) => Json(serde_json::json!({ "status": "ok" })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn v1_memories_search(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<V1SearchRequest>,
) -> impl IntoResponse {
    let limit = payload.limit.unwrap_or(10);

    let mut filters = payload.filters.clone().unwrap_or_default();
    let zones = payload
        .active_zones
        .clone()
        .unwrap_or_else(|| crate::memory::schema::parse_zones_from_prompt(&payload.query));
    if !zones.is_empty() {
        filters.zones = Some(zones);
    }

    let results = query_with_embedding_filtered(
        &workspace.workspace.memory,
        &payload.query,
        limit,
        Some(&filters),
    )
    .await
    .unwrap_or_default()
    .into_iter()
    .filter(|doc| is_primary_memory(&doc.metadata))
    .map(|doc| V1MemoryResponse {
        id: doc.id.unwrap_or_default(),
        memory: doc.content,
        user_id: Some(doc.path),
        metadata: doc.metadata,
    })
    .collect();

    Json(V1MemorySearchResponse {
        status: "ok".to_string(),
        results,
    })
}

pub async fn v1_memories_list(
    Extension(workspace): Extension<WorkspaceContext>,
    Query(params): Query<V1PaginationParams>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);

    let all_docs: Vec<_> = workspace
        .workspace
        .memory
        .all_documents()
        .await
        .into_iter()
        .filter(|doc| is_primary_memory(&doc.metadata))
        .collect();
    let total = all_docs.len();

    let memories = all_docs
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|doc| V1MemoryResponse {
            id: doc.id.unwrap_or_default(),
            memory: doc.content,
            user_id: Some(doc.path),
            metadata: doc.metadata,
        })
        .collect();

    Json(V1MemoryListResponse {
        memories,
        pagination: V1PaginationMetadata {
            total,
            limit,
            offset,
        },
    })
}

pub async fn v1_memories_get(
    Extension(workspace): Extension<WorkspaceContext>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match workspace.workspace.memory.get(&id).await {
        Ok(Some(doc)) if is_primary_memory(&doc.metadata) => Json(serde_json::json!({
            "status": "ok",
            "memory": V1MemoryResponse {
                id: doc.id.unwrap_or_default(),
                memory: doc.content,
                user_id: Some(doc.path),
                metadata: doc.metadata,
            }
        })),
        _ => Json(serde_json::json!({
            "status": "error",
            "message": "Memory not found"
        })),
    }
}

pub async fn v1_memories_update(
    Extension(workspace): Extension<WorkspaceContext>,
    Path(id): Path<String>,
    Json(payload): Json<V1AddMemoryRequest>,
) -> impl IntoResponse {
    let Some(existing) = workspace.workspace.memory.get(&id).await.ok().flatten() else {
        return Json(serde_json::json!({
            "status": "error",
            "message": "Memory not found"
        }));
    };

    let content = if let Some(text) = payload.text {
        text
    } else if let Some(messages) = payload.messages {
        messages
            .into_iter()
            .map(|msg| format!("{}: {}", msg.role, msg.content))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        existing.content.clone()
    };

    let path = payload
        .user_id
        .clone()
        .unwrap_or_else(|| existing.path.clone());
    let mut metadata = existing.metadata.clone();
    if let Some(extra) = payload.metadata {
        if let (Some(target), Some(source)) = (metadata.as_object_mut(), extra.as_object()) {
            for (key, value) in source {
                target.insert(key.clone(), value.clone());
            }
        } else {
            metadata = extra;
        }
    }

    let mut namespace = payload.namespace;
    if let Some(uid) = payload.user_id {
        metadata["user_id"] = serde_json::json!(uid);
        if namespace
            .as_ref()
            .and_then(|value| value.user_id.as_ref())
            .is_none()
        {
            let mut value = namespace.unwrap_or_default();
            value.user_id = metadata
                .get("user_id")
                .and_then(|entry| entry.as_str())
                .map(|entry| entry.to_string());
            namespace = Some(value);
        }
    }

    match workspace
        .workspace
        .update_primary_memory(
            &id,
            path,
            content,
            metadata,
            Some(TypedMemoryPayload {
                kind: payload.kind,
                evidence_kind: payload.evidence_kind,
                namespace,
                provenance: payload.provenance,
                ..Default::default()
            }),
        )
        .await
    {
        Ok(Some(updated_id)) => Json(serde_json::json!({
            "status": "ok",
            "message": "Memory updated successfully",
            "id": updated_id,
        })),
        Ok(None) => Json(serde_json::json!({
            "status": "error",
            "message": "Memory not found"
        })),
        Err(error) => Json(serde_json::json!({
            "status": "error",
            "message": error.to_string()
        })),
    }
}

pub async fn v1_memories_delete(
    Extension(workspace): Extension<WorkspaceContext>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match workspace.workspace.memory.delete(&id).await {
        Ok(Some(doc)) => {
            if let Some(memory_id) = doc.id.clone().or_else(|| Some(doc.path.clone())) {
                if let Err(error) = workspace.workspace.remove_memory_entities(&memory_id).await {
                    tracing::warn!(%error, memory_id = %memory_id, "failed to remove entity graph memory index");
                }
            }
            Json(serde_json::json!({
                "status": "ok",
                "message": "Memory deleted successfully"
            }))
        }
        _ => Json(serde_json::json!({
            "status": "error",
            "message": "Memory not found"
        })),
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CloudNodeRequest {
    pub url: String,
    pub token: String,
    pub instance_id: String,
}

pub async fn v1_mesh_cloud_get() -> impl IntoResponse {
    // License check
    let settings = crate::settings::XavierSettings::current();
    if let Err(e) = crate::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    Json(settings.pgheart).into_response()
}

pub async fn v1_mesh_cloud_update(Json(payload): Json<CloudNodeRequest>) -> impl IntoResponse {
    // License check
    let mut settings = crate::settings::XavierSettings::current();
    if let Err(e) = crate::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    settings.pgheart.url = Some(payload.url);
    settings.pgheart.token = Some(payload.token);
    settings.pgheart.instance_id = Some(payload.instance_id);
    
    if let Err(e) = settings.save().await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to save settings: {}", e)).into_response();
    }
    
    Json(serde_json::json!({ "status": "ok" })).into_response()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DataCommonsOptInRequest {
    pub enabled: bool,
    pub consent_given: bool,
    pub wallet_address: Option<String>,
}

pub async fn v1_mesh_data_commons_get() -> impl IntoResponse {
    // License check
    let settings = crate::settings::XavierSettings::current();
    if let Err(e) = crate::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    Json(settings.data_commons).into_response()
}

pub async fn v1_mesh_data_commons_opt_in(Json(payload): Json<DataCommonsOptInRequest>) -> impl IntoResponse {
    // License check
    let mut settings = crate::settings::XavierSettings::current();
    if let Err(e) = crate::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    settings.data_commons.enabled = payload.enabled;
    settings.data_commons.consent_given = payload.consent_given;
    if payload.wallet_address.is_some() {
        settings.data_commons.wallet_address = payload.wallet_address;
    }
    
    if let Err(e) = settings.save().await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to save settings: {}", e)).into_response();
    }
    
    Json(serde_json::json!({ "status": "ok" })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::util::ServiceExt;

    use crate::{
        agents::RuntimeConfig,
        memory::file_indexer::{FileIndexer, FileIndexerConfig},
        workspace::{WorkspaceConfig, WorkspaceContext, WorkspaceRegistry, WorkspaceState},
        AppState,
    };
    use ulid::Ulid;

    fn unique_test_path(prefix: &str, suffix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should not be before UNIX epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{unique}-{suffix}"))
    }

    async fn test_state() -> (AppState, WorkspaceContext) {
        let unique_id = Ulid::new().to_string();
        let db_path = unique_test_path(&format!("xavier-v1-test-{}", unique_id), "code_graph.db");
        let code_db = Arc::new(
            code_graph::db::CodeGraphDB::new(&db_path)
                .expect("failed to create CodeGraphDB for test"),
        );
        let code_indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&code_db)));
        let code_query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&code_db)));
        let workspace_registry = Arc::new(WorkspaceRegistry::new());
        let workspace = WorkspaceState::new(
            WorkspaceConfig {
                id: format!("test-{}", unique_id),
                token: format!("test-token-{}", unique_id),
                plan: crate::workspace::PlanTier::Personal,
                memory_backend: crate::memory::store::MemoryBackend::Memory,
                storage_limit_bytes: Some(10 * 1024 * 1024),
                request_limit: Some(10_000),
                request_unit_limit: Some(20_000),
                embedding_provider_mode: crate::workspace::EmbeddingProviderMode::BringYourOwn,
                managed_google_embeddings: false,
                sync_policy: crate::workspace::SyncPolicy::CloudMirror,
            },
            RuntimeConfig::default(),
            unique_test_path(&format!("xavier-v1-panel-{}", unique_id), "threads"),
        )
        .await
        .expect("failed to create WorkspaceState for test");
        workspace_registry
            .insert(workspace)
            .await
            .expect("failed to insert workspace into registry");
        let workspace = workspace_registry
            .authenticate(&format!("test-token-{}", unique_id))
            .await
            .expect("failed to authenticate with test token");

        (
            AppState {
                workspace_registry,
                indexer: FileIndexer::new(FileIndexerConfig::default(), Some(code_indexer.clone())),
                agent_indexer: crate::memory::agent_indexer::AgentIndexer::new(FileIndexer::new(
                    FileIndexerConfig::default(),
                    Some(code_indexer.clone()),
                )),
                code_indexer,
                code_query,
                code_db,
                security_service: Arc::new(crate::app::security_service::SecurityService::new()),
            },
            workspace,
        )
    }

    fn test_router(state: AppState, workspace: WorkspaceContext) -> Router {
        Router::new()
            .route("/v1/memories", post(v1_memories_add).get(v1_memories_list))
            .route(
                "/v1/memories/{id}",
                get(v1_memories_get)
                    .put(v1_memories_update)
                    .delete(v1_memories_delete),
            )
            .route("/v1/memories/search", post(v1_memories_search))
            .layer(Extension(workspace))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_v1_memories_crud() {
        let (state, workspace) = test_state().await;
        let app = test_router(state, workspace);

        // 1. Add Memory
        let add_req = Request::builder()
            .method("POST")
            .uri("/v1/memories")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "text": "test memory content",
                    "user_id": "user123",
                    "metadata": {"category": "test"}
                })
                .to_string(),
            ))
            .expect("failed to build add memory request");

        let resp = app
            .clone()
            .oneshot(add_req)
            .await
            .expect("failed to execute add memory request");
        assert_eq!(resp.status(), StatusCode::OK);

        // 2. List Memories
        let list_req = Request::builder()
            .method("GET")
            .uri("/v1/memories?limit=10")
            .body(Body::empty())
            .expect("failed to build list memories request");
        let resp = app
            .clone()
            .oneshot(list_req)
            .await
            .expect("failed to execute list memories request");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("failed to read list response body");
        let list_resp: V1MemoryListResponse =
            serde_json::from_slice(&body).expect("failed to parse list response JSON");
        assert_eq!(list_resp.memories.len(), 1);
        let memory_id = list_resp.memories[0].id.clone();

        // 3. Get Memory
        let get_req = Request::builder()
            .method("GET")
            .uri(format!("/v1/memories/{}", memory_id))
            .body(Body::empty())
            .expect("failed to build get memory request");
        let resp = app
            .clone()
            .oneshot(get_req)
            .await
            .expect("failed to execute get memory request");
        assert_eq!(resp.status(), StatusCode::OK);

        // 4. Update Memory
        let update_req = Request::builder()
            .method("PUT")
            .uri(format!("/v1/memories/{}", memory_id))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "text": "updated content",
                    "user_id": "user123"
                })
                .to_string(),
            ))
            .expect("failed to build update memory request");
        let resp = app
            .clone()
            .oneshot(update_req)
            .await
            .expect("failed to execute update memory request");
        assert_eq!(resp.status(), StatusCode::OK);

        let get_req = Request::builder()
            .method("GET")
            .uri(format!("/v1/memories/{}", memory_id))
            .body(Body::empty())
            .expect("failed to build get (after update) memory request");
        let resp = app
            .clone()
            .oneshot(get_req)
            .await
            .expect("failed to execute get (after update) memory request");
        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("failed to read get (after update) response body");
        let payload: serde_json::Value = serde_json::from_slice(&body)
            .expect("failed to parse get (after update) response JSON");
        assert_eq!(payload["memory"]["id"].as_str(), Some(memory_id.as_str()));
        assert_eq!(
            payload["memory"]["memory"].as_str(),
            Some("updated content")
        );
        assert_eq!(payload["memory"]["metadata"]["revision"].as_u64(), Some(2));

        // 5. Search Memory
        let search_req = Request::builder()
            .method("POST")
            .uri("/v1/memories/search")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "query": "updated",
                    "limit": 5
                })
                .to_string(),
            ))
            .expect("failed to build search memory request");
        let resp = app
            .clone()
            .oneshot(search_req)
            .await
            .expect("failed to execute search memory request");
        assert_eq!(resp.status(), StatusCode::OK);

        // 6. Delete Memory
        let delete_req = Request::builder()
            .method("DELETE")
            .uri(format!("/v1/memories/{}", memory_id))
            .body(Body::empty())
            .expect("failed to build delete memory request");
        let resp = app
            .oneshot(delete_req)
            .await
            .expect("failed to execute delete memory request");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_v1_memories_pagination() {
        let (state, workspace) = test_state().await;
        let app = test_router(state, workspace);

        // Add 5 memories
        for i in 0..5 {
            let add_req = Request::builder()
                .method("POST")
                .uri("/v1/memories")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "text": format!("memory {}", i),
                        "user_id": "user123"
                    })
                    .to_string(),
                ))
                .expect("failed to build add (pagination) memory request");
            app.clone()
                .oneshot(add_req)
                .await
                .expect("failed to execute add (pagination) memory request");
        }

        // Test pagination: limit=2, offset=1
        let list_req = Request::builder()
            .method("GET")
            .uri("/v1/memories?limit=2&offset=1")
            .body(Body::empty())
            .expect("failed to build pagination list request");
        let resp = app
            .oneshot(list_req)
            .await
            .expect("failed to execute pagination list request");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("failed to read pagination response body");
        let list_resp: V1MemoryListResponse =
            serde_json::from_slice(&body).expect("failed to parse pagination response JSON");

        assert_eq!(list_resp.memories.len(), 2);
        assert_eq!(list_resp.pagination.total, 5);
        assert_eq!(list_resp.pagination.limit, 2);
        assert_eq!(list_resp.pagination.offset, 1);
    }

    #[tokio::test]
    async fn test_v1_memories_search_supports_typed_filters_and_user_namespace() {
        // Isolate from parallel tests that may set embedding env vars
        let _prev_emb = std::env::var("XAVIER_EMBEDDING_PROVIDER_MODE").ok();
        let _prev_url = std::env::var("XAVIER_EMBEDDING_URL").ok();
        let _prev_emb2 = std::env::var("XAVIER_EMBEDDER").ok();
        let _prev_key = std::env::var("OPENAI_API_KEY").ok();
        let _prev_model = std::env::var("XAVIER_MODEL_PROVIDER").ok();
        let _prev_emb_model = std::env::var("XAVIER_EMBEDDING_MODEL").ok();
        std::env::remove_var("XAVIER_EMBEDDING_PROVIDER_MODE");
        std::env::remove_var("XAVIER_EMBEDDING_URL");
        std::env::remove_var("XAVIER_EMBEDDER");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("XAVIER_MODEL_PROVIDER");
        std::env::remove_var("XAVIER_EMBEDDING_MODEL");

        let (state, workspace) = test_state().await;
        // DEBUG: list docs after test_state creation
        {
            let docs = workspace.workspace.memory.all_documents().await;
            eprintln!("DEBUG initial docs count: {}", docs.len());
            for d in workspace.workspace.memory.docs.read().await.iter() {
                eprintln!(
                    "DEBUG doc: id={:?}, path={}, content={}..",
                    d.id,
                    d.path,
                    &d.content[..std::cmp::min(50, d.content.len())]
                );
            }
        }
        let app = test_router(state, workspace);

        for payload in [
            serde_json::json!({
                "text": "Decision: use typed provenance for OpenClaw bridge.",
                "user_id": "belal",
                "kind": "decision",
                "namespace": {
                    "project": "xavier",
                    "session_id": "session-typed"
                },
                "provenance": {
                    "source_app": "openclaw",
                    "source_type": "bridge_import"
                }
            }),
            serde_json::json!({
                "text": "Task: keep generic summaries secondary to specific evidence.",
                "user_id": "other-user",
                "kind": "task",
                "namespace": {
                    "project": "xavier"
                },
                "provenance": {
                    "source_app": "engram",
                    "source_type": "observation"
                }
            }),
        ] {
            let add_req = Request::builder()
                .method("POST")
                .uri("/v1/memories")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .expect("failed to build add (typed) memory request");
            let resp = app
                .clone()
                .oneshot(add_req)
                .await
                .expect("failed to execute add (typed) memory request");
            assert_eq!(resp.status(), StatusCode::OK);
            // DEBUG after add
            {
                let docs = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method("GET")
                            .uri("/v1/memories?limit=100")
                            .body(Body::empty())
                            .expect("DEBUG list req"),
                    )
                    .await
                    .expect("DEBUG list resp");
                let body = to_bytes(docs.into_body(), usize::MAX).await.expect("body");
                eprintln!("DEBUG after add: {}", String::from_utf8_lossy(&body));
            }
        }

        let search_req = Request::builder()
            .method("POST")
            .uri("/v1/memories/search")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "query": "typed provenance bridge",
                    "limit": 5,
                    "filters": {
                        "kinds": ["decision"],
                        "project": "xavier",
                        "user_id": "belal",
                        "source_app": "openclaw"
                    }
                })
                .to_string(),
            ))
            .expect("failed to build search (typed) request");
        let resp = app
            .oneshot(search_req)
            .await
            .expect("failed to execute search (typed) request");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("failed to read search (typed) response body");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("failed to parse search (typed) response JSON");
        let results = payload["results"]
            .as_array()
            .expect("search response 'results' should be an array");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["user_id"], "belal");
        assert_eq!(results[0]["metadata"]["kind"], "decision");
        assert_eq!(results[0]["metadata"]["namespace"]["project"], "xavier");
        assert_eq!(
            results[0]["metadata"]["provenance"]["source_app"],
            "openclaw"
        );
    }
}
