//! Memory handlers for search, addition, deletion, and management of memories.

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use std::sync::Arc;
use tracing::info;

use crate::cli::commands::spawn::load_spawn_memory;
use crate::cli::config::{resolve_base_url, resolve_http_token, xavier_token};
use crate::cli::handlers::json_response;
use crate::cli::security::secure_cli_input;
use crate::cli::state::CliState;
use crate::cli::types::*;
use xavier::memory::qmd_memory::MemoryDocument;

use xavier::memory::schema::MemoryLevel;
use xavier::memory::store::MemoryRecord;
use xavier::ports::inbound::input_security_port::SecureInputResult;

/// Embed handler.
pub async fn embed_handler(
    State(state): State<CliState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let input = body.get("input").and_then(|v| v.as_str()).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Missing 'input' field"})),
        )
    })?;

    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("all-MiniLM-L6-v2");

    match state.embedder.encode(input).await {
        Ok(embedding) => Ok(Json(serde_json::json!({
            "object": "list",
            "data": [{
                "object": "embedding",
                "index": 0,
                "embedding": embedding,
            }],
            "model": model,
            "usage": {
                "prompt_tokens": input.len(),
                "total_tokens": input.len(),
            }
        }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Embedding failed: {}", e)})),
        )),
    }
}

/// Export pack handler.
pub async fn export_pack_handler(
    State(state): State<CliState>,
    Json(payload): Json<ExportPackPayload>,
) -> Response {
    info!(
        "Export context pack request: topic={}, max_level={}",
        payload.topic, payload.max_level
    );

    let gating = xavier::retrieval::gating::AdaptiveGating::with_defaults();

    let all_docs = match state.store.list(&state.workspace_id).await {
        Ok(records) => records
            .into_iter()
            .map(|r| r.to_document())
            .collect::<Vec<_>>(),
        Err(e) => {
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({ "error": e.to_string() }),
            )
        }
    };

    let threads = state.panel_store.list_threads(50).await.unwrap_or_default();
    let mut episodic_summaries = Vec::new();
    for s in threads {
        let summary = if let Some(ref preview) = s.last_preview {
            if preview.contains("### Extractive Session Summary") {
                preview.clone()
            } else {
                if let Ok(messages) = state.panel_store.get_thread_messages(&s.id).await {
                    if !messages.is_empty() {
                        let gen = xavier::memory::episodic::summarize_session_extractive(&messages);
                        let _ = state.panel_store.update_last_preview(&s.id, &gen).await;
                        gen
                    } else {
                        preview.clone()
                    }
                } else {
                    preview.clone()
                }
            }
        } else {
            if let Ok(messages) = state.panel_store.get_thread_messages(&s.id).await {
                if !messages.is_empty() {
                    let gen = xavier::memory::episodic::summarize_session_extractive(&messages);
                    let _ = state.panel_store.update_last_preview(&s.id, &gen).await;
                    gen
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        };
        episodic_summaries.push(xavier::retrieval::gating::SessionSummary {
            session_id: s.id.clone(),
            start_time: s.started_at,
            summary,
            key_events: vec![],
            sentiment_timeline: vec![],
        });
    }

    let semantic_entities = Vec::new();

    let layered_result = gating
        .retrieve_layered(
            &all_docs, // In CLI, we don't have a separate working memory easily available, so we use all_docs as fallback
            &all_docs,
            &episodic_summaries,
            &semantic_entities,
            &payload.topic,
        )
        .await;

    let xml = xavier::memory::pack::generate_xcp(layered_result, payload.max_level);
    let filename = format!("context-{}.xcp", payload.topic.replace(" ", "_"));

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "status": "ok",
            "xml": xml,
            "filename": filename,
        }),
    )
}

/// Search handler.
pub async fn search_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<SearchPayload>,
) -> impl axum::response::IntoResponse {
    let sec_result = state
        .security
        .process_input(&payload.query)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: payload.query.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        info!(
            "Search blocked by security: injection detected (confidence={})",
            sec_result.detection_confidence
        );
        return axum::Json(serde_json::json!({
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
        }));
    }

    let effective_query = sec_result.effective_input();
    let limit = payload.limit.clamp(1, 100);
    info!("Search request: query={}, limit={}", effective_query, limit);

    let mut filters = payload.filters.clone().unwrap_or_default();
    let zones = payload
        .active_zones
        .clone()
        .unwrap_or_else(|| xavier::memory::schema::parse_zones_from_prompt(effective_query));
    filters.zones = Some(zones);

    let results: Vec<MemoryRecord> = match state
        .memory
        .search(effective_query, 10, Some(filters))
        .await
    {
        Ok(results) => results,
        Err(e) => {
            info!("Search error: {}", e);
            return axum::Json(serde_json::json!({
                "results": [],
                "query": payload.query,
                "count": 0,
                "error": e.to_string(),
                "workspace_id": state.workspace_id,
            }));
        }
    };

    let search_results: Vec<serde_json::Value> = results
        .into_iter()
        .map(|document| {
            serde_json::json!({
                "id": document.id,
                "content": document.content,
                "embedding": document.embedding,
            })
        })
        .collect();

    axum::Json(serde_json::json!({
        "results": search_results,
        "query": payload.query,
        "count": search_results.len(),
        "workspace_id": state.workspace_id,
    }))
}

/// Add handler.
pub async fn add_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<AddPayload>,
) -> impl axum::response::IntoResponse {
    let sec_result = state
        .security
        .process_input(&payload.content)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: payload.content.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        info!(
            "Add blocked by security: injection detected (confidence={})",
            sec_result.detection_confidence
        );
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }))
        .into_response();
    }

    let effective_content = sec_result
        .sanitized_input
        .as_deref()
        .unwrap_or(&sec_result.original_input);

    let path = payload
        .path
        .unwrap_or_else(|| format!("memory/{}", ulid::Ulid::new()));
    let mut metadata = payload.metadata.unwrap_or(serde_json::json!({}));

    let cluster_id = payload.cluster_id.clone();
    let level = payload
        .level
        .map(|l| xavier::memory::schema::MemoryLevel::parse(&l))
        .unwrap_or(MemoryLevel::Raw);
    let relation = payload
        .relation
        .clone()
        .map(|r| xavier::memory::schema::RelationKind {
            name: r,
            inverse: None,
        });

    if let Some(title) = payload.title {
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert("title".to_string(), serde_json::json!(title));
        }
    }

    // H-1 fix: extract typed memory fields (kind/evidence_kind/namespace/provenance) from
    // the payload metadata and normalize, so that /memory/search filters by project/agent_id/
    // session_id actually isolate memories (multi-tenancy for subagents). Previously this was
    // hardcoded to {"kind":"Context","namespace":"Global"}, which broke all namespace filters.
    let typed = xavier::memory::schema::TypedMemoryPayload {
        kind: metadata
            .get("kind")
            .cloned()
            .map(serde_json::from_value::<xavier::memory::schema::MemoryKind>)
            .transpose()
            .ok()
            .flatten(),
        evidence_kind: metadata
            .get("evidence_kind")
            .cloned()
            .map(serde_json::from_value::<xavier::memory::schema::EvidenceKind>)
            .transpose()
            .ok()
            .flatten(),
        namespace: metadata
            .get("namespace")
            .cloned()
            .map(serde_json::from_value::<xavier::memory::schema::MemoryNamespace>)
            .transpose()
            .ok()
            .flatten(),
        provenance: metadata
            .get("provenance")
            .cloned()
            .map(serde_json::from_value::<xavier::memory::schema::MemoryProvenance>)
            .transpose()
            .ok()
            .flatten(),
        ..Default::default()
    };
    let normalized_metadata = xavier::memory::schema::normalize_metadata(
        &path,
        metadata,
        &state.workspace_id,
        if typed.kind.is_some()
            || typed.namespace.is_some()
            || typed.provenance.is_some()
            || typed.evidence_kind.is_some()
        {
            Some(&typed)
        } else {
            None
        },
    )
    .unwrap_or_else(|_| serde_json::json!({"kind": "Context", "namespace": "Global"}));

    info!(
        "Add memory request: path={}, content_len={}",
        path,
        effective_content.len()
    );

    let record = MemoryRecord {
        id: String::new(),
        workspace_id: state.workspace_id.clone(),
        path: path.clone(),
        content: effective_content.to_string(),
        metadata: normalized_metadata,
        embedding: vec![],
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        revision: 1,
        primary: true,
        score: 0.0,
        deleted_at: None,
        parent_id: None,
        cluster_id,
        level,
        relation,
        clearance: Default::default(),
        revisions: vec![],
        encrypted_dek: None,
        content_iv: None,
        metadata_iv: None,
    };
    match state.memory.add(record).await {
        Ok(id) => {
            info!("Memory added successfully: {}", path);
            axum::Json(serde_json::json!({
                "status": "ok",
                "message": "Memory added",
                "path": path,
                "id": id,
                "security": {
                    "scanned": true,
                    "sanitized": sec_result.sanitized_input.is_some(),
                    "attack_type": sec_result.attack_type,
                }
            }))
            .into_response()
        }
        Err(e) => {
            info!("Add memory error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({
                    "status": "error",
                    "message": e.to_string(),
                })),
            )
                .into_response()
        }
    }
}

/// Update handler.
pub async fn update_handler(
    State(state): State<CliState>,
    headers: HeaderMap,
    axum::extract::Json(payload): axum::extract::Json<UpdateMemoryRequest>,
) -> Response {
    let expected_token = match resolve_http_token() {
        Ok(token) => token,
        Err(e) => {
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"status":"error","message": format!("Token resolution failed: {e}")}),
            );
        }
    };

    match headers
        .get("X-Xavier-Token")
        .and_then(|value| value.to_str().ok())
    {
        Some(token) if token == expected_token => {}
        _ => {
            return json_response(
                StatusCode::UNAUTHORIZED,
                serde_json::json!({"status":"error","message":"Unauthorized"}),
            );
        }
    }

    let sec_result = state
        .security
        .process_input(&payload.content)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: payload.content.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        info!(
            "Update blocked by security: injection detected (confidence={})",
            sec_result.detection_confidence
        );
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }))
        .into_response();
    }

    let effective_content = sec_result
        .sanitized_input
        .as_deref()
        .unwrap_or(&sec_result.original_input);

    let path = payload
        .path
        .unwrap_or_else(|| format!("memory/{}", payload.id));
    let metadata = payload.metadata.unwrap_or(serde_json::json!({}));

    let record = MemoryRecord {
        id: payload.id.clone(),
        workspace_id: state.workspace_id.clone(),
        path: path.clone(),
        content: effective_content.to_string(),
        metadata,
        embedding: vec![],
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        revision: 1,
        primary: true,
        deleted_at: None,
        score: 0.0,
        parent_id: None,
        cluster_id: None,
        level: MemoryLevel::Raw,
        relation: None,
        clearance: Default::default(),
        revisions: vec![],
        encrypted_dek: None,
        content_iv: None,
        metadata_iv: None,
    };

    match state.memory.update(&payload.id, record).await {
        Ok(updated_record) => json_response(
            StatusCode::OK,
            serde_json::json!({
                "status": "ok",
                "message": "Memory updated",
                "id": updated_record.id,
                "path": updated_record.path,
                "security": {
                    "scanned": true,
                    "sanitized": sec_result.sanitized_input.is_some(),
                    "attack_type": sec_result.attack_type,
                }
            }),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({
                "status": "error",
                "message": e.to_string(),
            }),
        ),
    }
}

/// Delete handler.
pub async fn delete_handler(
    State(state): State<CliState>,
    headers: HeaderMap,
    axum::extract::Json(payload): axum::extract::Json<DeleteMemoryRequest>,
) -> Response {
    let expected_token = match resolve_http_token() {
        Ok(token) => token,
        Err(e) => {
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"status":"error","message": format!("Token resolution failed: {e}")}),
            );
        }
    };

    match headers
        .get("X-Xavier-Token")
        .and_then(|value| value.to_str().ok())
    {
        Some(token) if token == expected_token => {}
        _ => {
            return json_response(
                StatusCode::UNAUTHORIZED,
                serde_json::json!({"status":"error","message":"Unauthorized"}),
            );
        }
    }

    let id_or_path = payload
        .id
        .or(payload.path)
        .filter(|value| !value.trim().is_empty());
    let Some(id_or_path_str) = id_or_path.as_deref() else {
        return json_response(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"status":"error","message":"Provide either id or path"}),
        );
    };

    match state
        .store
        .delete(&state.workspace_id, id_or_path_str)
        .await
    {
        Ok(Some(record)) => json_response(
            StatusCode::OK,
            serde_json::json!({
                "status": "ok",
                "deleted": true,
                "id": record.id,
                "path": record.path,
            }),
        ),
        Ok(None) => json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({
                "status": "not_found",
                "deleted": false,
                "id_or_path": id_or_path_str,
            }),
        ),
        Err(error) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({
                "status": "error",
                "message": error.to_string(),
            }),
        ),
    }
}

/// Re-index memories that have missing or empty embeddings.
///
/// Iterates over all stored memories, checks for empty embedding vectors,
/// recalculates embeddings via the embedder, and updates each record.
pub async fn reindex_handler(State(state): State<CliState>, headers: HeaderMap) -> Response {
    use std::time::Duration;
    use tokio::time::timeout;

    if let Err(r) = check_cli_token(&headers) {
        return r;
    }

    let records = match state.store.list(&state.workspace_id).await {
        Ok(records) => records,
        Err(e) => {
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"status":"error","message": format!("Failed to list memories: {e}")}),
            );
        }
    };

    let total = records.len();
    let mut reindexed = 0usize;
    let mut errors = Vec::new();
    let mut skipped = 0usize;

    for record in &records {
        // Skip records that already have a non-empty embedding
        if !record.embedding.is_empty() {
            skipped += 1;
            continue;
        }

        // Try embedding within a 60-second timeout per doc
        match timeout(
            Duration::from_secs(60),
            state.embedder.encode(&record.content),
        )
        .await
        {
            Ok(Ok(embedding)) => {
                let mut updated = record.clone();
                updated.embedding = embedding;
                updated.updated_at = chrono::Utc::now();
                match state.store.update(updated).await {
                    Ok(()) => reindexed += 1,
                    Err(e) => errors.push(format!("{}: update failed: {e}", record.id)),
                }
            }
            Ok(Err(e)) => {
                errors.push(format!("{}: embedding failed: {e}", record.id));
            }
            Err(_) => {
                errors.push(format!("{}: embedding timed out", record.id));
            }
        }
    }

    if errors.is_empty() {
        json_response(
            StatusCode::OK,
            serde_json::json!({
                "status": "ok",
                "total": total,
                "reindexed": reindexed,
                "skipped": skipped,
                "errors": [],
            }),
        )
    } else {
        json_response(
            StatusCode::OK,
            serde_json::json!({
                "status": "partial",
                "total": total,
                "reindexed": reindexed,
                "skipped": skipped,
                "errors": errors,
            }),
        )
    }
}

/// Stats handler.
pub async fn stats_handler(State(state): State<CliState>) -> impl axum::response::IntoResponse {
    axum::Json(serde_json::json!({
        "status": "ok",
        "workspace_id": state.workspace_id,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Memory query handler.
pub async fn memory_query_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<MemoryQueryPayload>,
) -> impl axum::response::IntoResponse {
    let sec_result = state
        .security
        .process_input(&payload.query)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: payload.query.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let limit = payload.limit.unwrap_or(10).clamp(1, 100);
    info!(
        "Memory query request: query={}, limit={}",
        payload.query, limit
    );

    let local_node_id = if let Ok(identity) = xavier::mesh::NodeIdentity::load_or_create() {
        identity.node_id.0
    } else {
        "local".to_string()
    };

    let federated = payload.federated.clone().unwrap_or_default();
    let local_dbs = federated.local_dbs.clone();
    let peer_nodes = federated.peer_nodes.clone();
    let propagate_to_mesh = federated.propagate_to_mesh;
    let max_hops = federated.max_hops;

    // 1. Local workspace search future
    let local_query = payload.query.clone();
    let store = state.store.clone();
    let default_workspace = state.workspace_id.clone();
    let local_node_id_clone = local_node_id.clone();
    let mut search_results = async move {
        let mut results_list = Vec::new();
        let target_dbs = if local_dbs.is_empty() {
            vec![default_workspace]
        } else {
            local_dbs
        };

        for ws_id in target_dbs {
            match store.search(&ws_id, &local_query, None).await {
                Ok(results) => {
                    for record in results {
                        let doc = record.to_document();
                        results_list.push(serde_json::json!({
                            "id": doc.id,
                            "path": doc.path,
                            "content": doc.content,
                            "metadata": doc.metadata,
                            "embedding": doc.embedding,
                            "source": "local",
                            "source_node_id": local_node_id_clone.clone(),
                            "source_db_id": ws_id.clone(),
                        }));
                    }
                }
                Err(e) => {
                    info!("Memory query local error for DB {}: {}", ws_id, e);
                }
            }
        }
        results_list
    }
    .await;
    // 2. Parallel fan-out to remote workspaces via the Mesh P2P API
    let mut remote_futures = Vec::new();
    if max_hops > 0 {
        let next_federated = xavier::memory::schema::FederatedSearchRequest {
            max_hops: max_hops - 1,
            ..federated.clone()
        };

        if let Ok(registry) = xavier::mesh::PeerRegistry::load() {
            let peers = registry.list_peers();

            for peer in peers {
                if !propagate_to_mesh && !peer_nodes.contains(&peer.node_id.0) {
                    continue;
                }

                for ws_id in &peer.shared_workspace_ids {
                    if let Some(token) = peer.shared_workspace_tokens.get(ws_id) {
                        let client = state.http_client.clone();
                        let url = format!("{}/v1/mesh/workspaces/query", peer.endpoint_url);
                        let query_payload = serde_json::json!({
                            "token": token,
                            "query": payload.query,
                            "limit": limit,
                            "federated": next_federated,
                        });

                        let ws_id = ws_id.clone();
                        let peer_node_id = peer.node_id.0.clone();
                        remote_futures.push(async move {
                            let res = client
                                .post(&url)
                                .json(&query_payload)
                                .timeout(std::time::Duration::from_secs(5))
                                .send()
                                .await;

                            match res {
                                Ok(resp) => {
                                    if resp.status().is_success() {
                                        if let Ok(body) = resp.json::<serde_json::Value>().await {
                                            if let Some(results_arr) =
                                                body.get("results").and_then(|v| v.as_array())
                                            {
                                                let mut remote_docs = Vec::new();
                                                for r in results_arr {
                                                    let mut r_clone = r.clone();
                                                    if let Some(obj) = r_clone.as_object_mut() {
                                                        obj.insert(
                                                            "source".to_string(),
                                                            serde_json::json!(format!(
                                                                "remote:{}::{}",
                                                                peer_node_id, ws_id
                                                            )),
                                                        );
                                                        if obj.get("source_node_id").is_none() {
                                                            obj.insert(
                                                                "source_node_id".to_string(),
                                                                serde_json::json!(
                                                                    peer_node_id.clone()
                                                                ),
                                                            );
                                                        }
                                                        if obj.get("source_db_id").is_none() {
                                                            obj.insert(
                                                                "source_db_id".to_string(),
                                                                serde_json::json!(ws_id.clone()),
                                                            );
                                                        }
                                                    }
                                                    remote_docs.push(r_clone);
                                                }
                                                return Some(remote_docs);
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Failed to query remote workspace {} on peer {}: {}",
                                        ws_id,
                                        peer_node_id,
                                        e
                                    );
                                }
                            }
                            None
                        });
                    }
                }
            }
        }

        let remote_results = futures_util::future::join_all(remote_futures).await;
        for mut remote_list in remote_results.into_iter().flatten() {
            search_results.append(&mut remote_list);
        }
    }

    axum::Json(serde_json::json!({
        "status": "ok",
        "query": payload.query,
        "count": search_results.len(),
        "results": search_results,
        "workspace_id": state.workspace_id,
    }))
}

/// Search memories filtered.
pub async fn search_memories_filtered(
    query: &str,
    limit: usize,
    clusters: Vec<String>,
    levels: Vec<String>,
    offline_ok: bool,
) -> anyhow::Result<()> {
    use crate::cli::config::{
        auth_failed_error, auth_failed_message, classify_error_response, classify_transport_error,
        CliHttpOutcome,
    };

    let query = secure_cli_input("search query", query, 4_096)?;
    let limit = limit.clamp(1, 100);
    let token = xavier_token();
    let base_url = resolve_base_url();
    let url = format!("{}/memory/search", base_url);

    let parsed_levels = levels
        .iter()
        .map(|l| xavier::memory::schema::MemoryLevel::parse(l))
        .collect::<Vec<_>>();

    let filters = xavier::memory::schema::MemoryQueryFilters {
        cluster_ids: if clusters.is_empty() {
            None
        } else {
            Some(clusters)
        },
        levels: if parsed_levels.is_empty() {
            None
        } else {
            Some(parsed_levels)
        },
        ..xavier::memory::schema::MemoryQueryFilters::default()
    };

    let client = crate::cli::commands::CLI_HTTP_CLIENT.clone();

    let response = client
        .post(&url)
        .header("X-Xavier-Token", &token)
        .json(&serde_json::json!({
            "query": query,
            "limit": limit,
            "filters": filters
        }))
        .send()
        .await;

    async fn offline_search(
        query: &str,
        limit: usize,
        filters: &xavier::memory::schema::MemoryQueryFilters,
    ) -> anyhow::Result<()> {
        match load_spawn_memory().await {
            Ok(memory) => match memory.search_filtered(query, limit, Some(filters)).await {
                Ok(docs) => {
                    println!("\n[OFFLINE] Search results for: {}", query);
                    let json_results = serde_json::json!({
                        "results": docs.iter().map(|doc: &MemoryDocument| {
                            serde_json::json!({
                                "id": doc.id,
                                "path": doc.path,
                                "content": doc.content,
                                "metadata": doc.metadata,
                                "score": doc.metadata.get("score").and_then(|v: &serde_json::Value| v.as_f64()).unwrap_or(1.0),
                            })
                        }).collect::<Vec<_>>()
                    });
                    println!("{}", serde_json::to_string_pretty(&json_results)?);
                    Ok(())
                }
                Err(e) => {
                    println!("❌ Local search failed: {}", e);
                    Err(anyhow::anyhow!("local offline search failed: {e}"))
                }
            },
            Err(e) => {
                println!(
                    "❌ Failed to initialize local offline database store: {}",
                    e
                );
                Err(anyhow::anyhow!("offline store init failed: {e}"))
            }
        }
    }

    match response {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            println!("\nSearch results for: {}", query);
            println!("{}", serde_json::to_string_pretty(&body)?);
            Ok(())
        }
        Ok(resp) => {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            match classify_error_response(status, body_text) {
                CliHttpOutcome::AuthFailed { status } => {
                    if offline_ok {
                        eprintln!("{}", auth_failed_message(status));
                        println!(
                            "⚠️ AUTH_FAILED but --offline-ok set. Falling back to local offline database index..."
                        );
                        offline_search(&query, limit, &filters).await
                    } else {
                        eprintln!("{}", auth_failed_message(status));
                        Err(auth_failed_error(status))
                    }
                }
                CliHttpOutcome::HttpError { status, body } => {
                    println!(
                        "⚠️ Server HTTP {status} ({body}). Falling back to local offline database index..."
                    );
                    offline_search(&query, limit, &filters).await
                }
                CliHttpOutcome::ConnectionRefused { detail } => {
                    println!(
                        "⚠️ CONNECTION_REFUSED ({detail}). Falling back to local offline database index..."
                    );
                    offline_search(&query, limit, &filters).await
                }
            }
        }
        Err(e) => {
            let outcome = classify_transport_error(&e);
            if let CliHttpOutcome::ConnectionRefused { detail } = &outcome {
                println!(
                    "⚠️ CONNECTION_REFUSED ({detail}). Falling back to local offline database index..."
                );
            } else {
                println!(
                    "⚠️ Server offline or request failed. Falling back to local offline database index..."
                );
            }
            offline_search(&query, limit, &filters).await
        }
    }
}

/// Add memory hierarchical.
pub async fn add_memory_hierarchical(
    content: &str,
    title: Option<&str>,
    kind: Option<&str>,
    cluster_id: Option<&str>,
    level: Option<&str>,
    relation: Option<&str>,
) -> anyhow::Result<()> {
    let content = secure_cli_input("memory content", content, 1_000_000)?;
    let title = title
        .map(|title| secure_cli_input("memory title", title, 512))
        .transpose()?;
    let token = xavier_token();
    let base_url = resolve_base_url();
    let url = format!("{}/memory/add", base_url);

    let mut body = serde_json::json!({
        "content": content,
        "metadata": {}
    });

    if let Some(t) = title.as_deref() {
        body["metadata"]["title"] = serde_json::json!(t);
    }
    if let Some(k) = kind {
        body["metadata"]["kind"] = serde_json::json!(k);
    }
    if let Some(c) = cluster_id {
        body["cluster_id"] = serde_json::json!(c);
    }
    if let Some(l) = level {
        body["level"] = serde_json::json!(l);
    }
    if let Some(r) = relation {
        body["relation"] = serde_json::json!(r);
    }

    let client = crate::cli::commands::CLI_HTTP_CLIENT.clone();

    let response = client
        .post(&url)
        .header("X-Xavier-Token", &token)
        .json(&body)
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            println!("Memory added successfully via HTTP API!");
            Ok(())
        }
        Ok(resp) => {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            if crate::cli::config::is_auth_failure(status) {
                eprintln!(
                    "{}",
                    crate::cli::config::auth_failed_message(status.as_u16())
                );
                return Err(crate::cli::config::auth_failed_error(status.as_u16()));
            }
            println!(
                "⚠️ Server HTTP {} ({}). Falling back to local offline database write...",
                status.as_u16(),
                body_text
            );
            offline_add_memory(
                &content,
                title.as_deref(),
                kind,
                cluster_id,
                level,
                relation,
            )
            .await
        }
        Err(e) => {
            println!(
                "⚠️ CONNECTION_REFUSED ({}). Falling back to local offline database write...",
                e
            );
            offline_add_memory(
                &content,
                title.as_deref(),
                kind,
                cluster_id,
                level,
                relation,
            )
            .await
        }
    }
}

async fn offline_add_memory(
    content: &str,
    title: Option<&str>,
    kind: Option<&str>,
    cluster_id: Option<&str>,
    level: Option<&str>,
    relation: Option<&str>,
) -> anyhow::Result<()> {
    match load_spawn_memory().await {
        Ok(memory) => {
            let path = format!("cli/add/{}", chrono::Utc::now().timestamp());
            let mut metadata = serde_json::json!({});
            if let Some(t) = title {
                metadata["title"] = serde_json::json!(t);
            }
            if let Some(k) = kind {
                metadata["kind"] = serde_json::json!(k);
            }

            let typed_payload = xavier::memory::schema::TypedMemoryPayload {
                cluster_id: cluster_id.map(|s| s.to_string()),
                level: level.map(xavier::memory::schema::MemoryLevel::parse),
                relation: relation.map(xavier::memory::schema::RelationKind::new),
                ..Default::default()
            };

            match memory
                .add_document_typed(path, content.to_string(), metadata, Some(typed_payload))
                .await
            {
                Ok(id) => {
                    println!("✅ Memory added successfully offline to local SQLite-Vec database!");
                    println!("Document ID: {id}");
                    Ok(())
                }
                Err(err) => {
                    println!("❌ Local write failed: {}", err);
                    Err(anyhow::anyhow!("local offline write failed: {err}"))
                }
            }
        }
        Err(e) => {
            println!(
                "❌ Failed to initialize local offline database store: {}",
                e
            );
            Err(anyhow::anyhow!("offline store init failed: {e}"))
        }
    }
}

/// Export handler.
pub async fn export_handler(
    State(state): State<CliState>,
    Query(params): Query<ExportPayload>,
) -> impl IntoResponse {
    let public_only = params.public.unwrap_or(false);
    match state.memory.export(public_only).await {
        Ok(docs) => Json(docs).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": e.to_string()
            })),
        )
            .into_response(),
    }
}

#[allow(clippy::result_large_err)]
/// Check cli token.
pub(crate) fn check_cli_token(headers: &HeaderMap) -> Result<(), Response> {
    let expected_token = match resolve_http_token() {
        Ok(token) => token,
        Err(e) => {
            return Err(json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"status":"error","message": format!("Token resolution failed: {e}")}),
            ));
        }
    };

    match headers
        .get("X-Xavier-Token")
        .and_then(|value| value.to_str().ok())
    {
        Some(token) if token == expected_token => Ok(()),
        _ => Err(json_response(
            StatusCode::UNAUTHORIZED,
            serde_json::json!({"status":"error","message":"Unauthorized"}),
        )),
    }
}

/// Decay handler.
pub async fn decay_handler(State(state): State<CliState>, headers: HeaderMap) -> Response {
    if let Err(r) = check_cli_token(&headers) {
        return r;
    }
    let manager =
        xavier::memory::manager::core::MemoryManager::new(Arc::clone(&state.qmd_memory), None);
    match manager.decay_memories().await {
        Ok(res) => json_response(
            StatusCode::OK,
            serde_json::json!({ "status": "ok", "documents_affected": res.documents_affected }),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "status": "error", "message": e.to_string() }),
        ),
    }
}

/// Consolidate handler.
pub async fn consolidate_handler(
    State(state): State<CliState>,
    headers: HeaderMap,
    axum::extract::Json(payload): axum::extract::Json<serde_json::Value>,
) -> Response {
    if let Err(r) = check_cli_token(&headers) {
        return r;
    }
    let nightly = payload
        .get("nightly")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let manager =
        xavier::memory::manager::core::MemoryManager::new(Arc::clone(&state.qmd_memory), None);
    let result = if nightly {
        manager.nightly_consolidate().await
    } else {
        manager.consolidate_memories().await
    };
    match result {
        Ok(res) => json_response(
            StatusCode::OK,
            serde_json::json!({ "status": "ok", "documents_affected": res.documents_affected, "bytes_freed": res.bytes_freed }),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "status": "error", "message": e.to_string() }),
        ),
    }
}

/// Evict handler.
pub async fn evict_handler(
    State(state): State<CliState>,
    headers: HeaderMap,
    axum::extract::Json(payload): axum::extract::Json<EvictPayload>,
) -> Response {
    if let Err(r) = check_cli_token(&headers) {
        return r;
    }
    let mut manager =
        xavier::memory::manager::core::MemoryManager::new(Arc::clone(&state.qmd_memory), None);

    let result = if let Some(priority_str) = &payload.priority {
        let p = match priority_str.as_str() {
            "critical" => xavier::memory::manager::types::MemoryPriority::Critical,
            "high" => xavier::memory::manager::types::MemoryPriority::High,
            "medium" => xavier::memory::manager::types::MemoryPriority::Medium,
            "low" => xavier::memory::manager::types::MemoryPriority::Low,
            "ephemeral" => xavier::memory::manager::types::MemoryPriority::Ephemeral,
            _ => xavier::memory::manager::types::MemoryPriority::Medium,
        };
        manager.evict_by_priority(p).await
    } else {
        if let Some(threshold) = payload.threshold {
            let mut config = manager.config().clone();
            config.quality_threshold = threshold;
            manager.set_config(config);
        }
        manager.evict_low_quality().await
    };

    match result {
        Ok(res) => json_response(
            StatusCode::OK,
            serde_json::json!({ "status": "ok", "documents_affected": res.documents_affected, "bytes_freed": res.bytes_freed }),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "status": "error", "message": e.to_string() }),
        ),
    }
}

/// Manage handler.
pub async fn manage_handler(State(state): State<CliState>, headers: HeaderMap) -> Response {
    if let Err(r) = check_cli_token(&headers) {
        return r;
    }
    let manager =
        xavier::memory::manager::core::MemoryManager::new(Arc::clone(&state.qmd_memory), None);

    let mut total_affected = 0;
    let mut total_freed = 0;

    if let Ok(res) = manager.decay_memories().await {
        total_affected += res.documents_affected;
    }
    if let Ok(res) = manager.consolidate_memories().await {
        total_affected += res.documents_affected;
        total_freed += res.bytes_freed;
    }
    if let Ok(res) = manager.evict_low_quality().await {
        total_affected += res.documents_affected;
        total_freed += res.bytes_freed;
    }

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "status": "ok",
            "message": "Auto-management cycle complete",
            "documents_affected": total_affected,
            "bytes_freed": total_freed
        }),
    )
}

/// Memory index self handler.
pub async fn memory_index_self_handler(
    State(state): State<CliState>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = check_cli_token(&headers) {
        return r;
    }

    let files_to_index = vec![
        ("SOUL.md", "identity"),
        ("CHANGELOG.md", "log"),
        ("PLAN_TRES_MEMORIAS.md", "decision"),
        ("USER.md", "identity"),
        ("AGENTS.md", "identity"),
        ("architecture.md", "architecture"),
        ("PIPELINE.md", "architecture"),
        ("PLAN.md", "architecture"),
    ];

    let mut indexed_count = 0;
    let mut errors = Vec::new();

    for (filename, doc_type) in files_to_index {
        match index_file_by_sections(&state, filename, doc_type).await {
            Ok(count) => indexed_count += count,
            Err(e) => {
                if tokio::fs::metadata(filename).await.is_ok() {
                    errors.push(format!("{}: {}", filename, e));
                }
            }
        }
    }

    // Index all markdown files in memory/ directory
    if let Ok(mut entries) = tokio::fs::read_dir("memory").await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("md") {
                let filename = path.to_string_lossy().to_string();
                match index_file_by_sections(&state, &filename, "log").await {
                    Ok(count) => indexed_count += count,
                    Err(e) => errors.push(format!("{}: {}", filename, e)),
                }
            }
        }
    }

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "status": if errors.is_empty() { "ok" } else { "partial" },
            "indexed_chunks": indexed_count,
            "errors": if errors.is_empty() { serde_json::Value::Null } else { serde_json::json!(errors) }
        }),
    )
}

async fn index_file_by_sections(
    state: &CliState,
    filename: &str,
    doc_type: &str,
) -> anyhow::Result<usize> {
    let content = tokio::fs::read_to_string(filename).await?;
    let sections = split_markdown_by_sections(&content);
    let mut count = 0;

    for (title, section_content) in sections {
        let path = format!("xavier-self/{}", filename);

        // Generate embedding for the section
        let embedding = match state.embedder.encode(&section_content).await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Failed to generate embedding for {}: {}", path, e);
                vec![]
            }
        };

        let record = MemoryRecord {
            id: uuid::Uuid::new_v4().to_string(),
            workspace_id: state.workspace_id.clone(),
            path: path.clone(),
            content: section_content,
            metadata: serde_json::json!({
                "source": "xavier-self",
                "type": doc_type,
                "title": title,
                "file": filename,
                "kind": "Document",
                "namespace": "Global"
            }),
            embedding,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            revision: 1,
            deleted_at: None,
            primary: true,
            score: 0.0,
            level: MemoryLevel::Raw,
            ..Default::default()
        };

        if state.memory.add(record).await.is_ok() {
            count += 1;
        }
    }

    Ok(count)
}

fn split_markdown_by_sections(content: &str) -> Vec<(String, String)> {
    let mut sections = Vec::new();
    let mut current_title = "Introduction".to_string();
    let mut current_content = String::new();

    for line in content.lines() {
        if line.starts_with('#') {
            if !current_content.trim().is_empty() {
                sections.push((current_title.clone(), current_content.clone()));
            }
            current_title = line.trim_start_matches('#').trim().to_string();
            current_content = line.to_string();
            current_content.push('\n');
        } else {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    if !current_content.trim().is_empty() {
        sections.push((current_title, current_content));
    }

    if sections.is_empty() && !content.trim().is_empty() {
        sections.push(("General".to_string(), content.to_string()));
    }

    sections
}

/// Timeline query handler.
pub async fn timeline_query_handler(
    State(state): State<CliState>,
    headers: HeaderMap,
    axum::extract::Json(payload): axum::extract::Json<TimelineQueryPayload>,
) -> Response {
    if let Err(r) = check_cli_token(&headers) {
        return r;
    }

    let engine = xavier::context::timeline::TimelineEngine::new(Arc::clone(&state.qmd_memory));

    let query = xavier::context::timeline::TimelineQuery {
        query: payload.query.clone(),
        start_date: payload.start_date,
        end_date: payload.end_date,
        agent_id: payload.agent_id.clone(),
        limit: payload.limit,
    };

    match engine.get_time_slice(&query).await {
        Ok(slice) => json_response(
            StatusCode::OK,
            serde_json::json!({
                "status": "ok",
                "period_start": slice.period_start,
                "period_end": slice.period_end,
                "memories_count": slice.memories.len(),
                "memories": slice.memories,
                "events_count": slice.timeline_events.len(),
                "timeline_events": slice.timeline_events
            }),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({
                "status": "error",
                "message": e.to_string()
            }),
        ),
    }
}
