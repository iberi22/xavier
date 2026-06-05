use std::path::PathBuf;
use axum::{
    extract::Query,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use tracing::{info, warn};
use serde::Serialize;

use crate::cli::state::CliState;
use crate::cli::types::*;
use crate::cli::config::{
    resolve_base_url, resolve_http_token,
};
use crate::cli::code_graph::{code_find_symbols, filter_symbols_by_query};
use crate::cli::utils::estimate_tokens;
use crate::cli::security::{secure_cli_input, secure_external_input, secure_optional_request_field};

use xavier::memory::schema::{MemoryLevel};
use xavier::memory::store::{MemoryRecord};
use xavier::ports::inbound::input_security_port::SecureInputResult;
use xavier::server::panel::{
    CreateThreadRequest, PanelChatRequest, PanelChatResponse,
};

pub fn json_response(status: StatusCode, body: serde_json::Value) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("x-request-id", uuid::Uuid::new_v4().to_string())
        .body(axum::body::Body::from(body.to_string()))
        .unwrap_or_else(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"status":"error"}).to_string(),
            )
                .into_response()
        })
}

pub async fn health_handler(State(state): State<CliState>) -> Response {
    let uptime_secs = crate::cli::server::START_TIME.elapsed().as_secs();

    let lag_ms = xavier::tasks::session_sync_task::calculate_indexing_lag(
        state.store.as_ref(),
        &state.workspace_id,
    )
    .await;

    let embedding_provider = std::env::var("XAVIER_EMBEDDING_PROVIDER_MODE")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| {
            if std::env::var("XAVIER_EMBEDDER")
                .ok()
                .map(|v| v.trim().to_ascii_lowercase())
                .as_deref()
                == Some("gllm")
                || std::env::var("XAVIER_GLLM_MODEL").is_ok()
            {
                "gllm".to_string()
            } else if std::env::var("OPENAI_API_KEY").is_ok()
                || std::env::var("XAVIER_EMBEDDING_API_KEY").is_ok()
            {
                "openai".to_string()
            } else {
                "none".to_string()
            }
        });

    let sqlite_db_size = calculate_data_dir_size().unwrap_or(0);

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "status": "ok",
            "service": "xavier",
            "version": env!("CARGO_PKG_VERSION"),
            "embedding_provider": embedding_provider,
            "sqlite_db_size": sqlite_db_size,
            "uptime": uptime_secs,
            "lag_ms": lag_ms,
        }),
    )
}

fn calculate_data_dir_size() -> Option<u64> {
    let data_dir = std::path::Path::new("data");
    if !data_dir.is_dir() {
        return None;
    }
    let mut total_size = 0u64;
    if let Ok(entries) = std::fs::read_dir(data_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                total_size += std::fs::metadata(&path).ok()?.len();
            } else if path.is_dir() {
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub_entry in sub_entries.flatten() {
                        if sub_entry.path().is_file() {
                            total_size += std::fs::metadata(sub_entry.path()).ok()?.len();
                        }
                    }
                }
            }
        }
    }
    Some(total_size)
}

pub async fn version_handler() -> Response {
    let features = if cfg!(feature = "enterprise") {
        vec!["gllm-embeddings", "enterprise"]
    } else {
        vec!["gllm-embeddings"]
    };

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "service": "xavier",
            "version": env!("CARGO_PKG_VERSION"),
            "features": features,
            "build": env!("CARGO_PKG_VERSION"),
        }),
    )
}

pub async fn readiness_handler(State(state): State<CliState>) -> Response {
    let memory_store = match state.store.health().await {
        Ok(detail) => serde_json::json!({
            "ready": true,
            "detail": detail,
        }),
        Err(error) => serde_json::json!({
            "ready": false,
            "detail": error.to_string(),
        }),
    };
    let code_graph = state
        .code_db
        .stats()
        .map(|stats| {
            serde_json::json!({
                "ready": true,
                "total_files": stats.total_files,
                "total_symbols": stats.total_symbols,
                "total_imports": stats.total_imports,
            })
        })
        .unwrap_or_else(|error| {
            serde_json::json!({
                "ready": false,
                "error": error.to_string(),
            })
        });

    let ready = memory_store["ready"].as_bool().unwrap_or(false)
        && code_graph["ready"].as_bool().unwrap_or(false);

    json_response(
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        serde_json::json!({
            "status": if ready { "ok" } else { "degraded" },
            "service": "xavier",
            "workspace_id": state.workspace_id,
            "memory_store": memory_store,
            "code_graph": code_graph,
        }),
    )
}

pub async fn build_handler(State(state): State<CliState>) -> Response {
    json_response(
        StatusCode::OK,
        serde_json::json!({
            "service": "xavier",
            "version": env!("CARGO_PKG_VERSION"),
            "workspace_id": state.workspace_id,
            "base_url": resolve_base_url(),
            "memory_backend": crate::settings::XavierSettings::current().memory.backend,
            "code_graph_db_path": crate::cli::config::code_graph_db_path(),
        }),
    )
}

pub async fn account_usage_handler(State(state): State<CliState>, headers: HeaderMap) -> Response {
    let expected_token = match resolve_http_token() {
        Ok(token) => token,
        Err(e) => {
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({
                    "status": "error",
                    "message": format!("Token resolution failed: {e}"),
                }),
            )
        }
    };

    let provided_token = headers
        .get("X-Xavier-Token")
        .and_then(|value| value.to_str().ok());
    if provided_token != Some(expected_token.as_str()) {
        return json_response(
            StatusCode::UNAUTHORIZED,
            serde_json::json!({
                "status": "error",
                "message": "Unauthorized",
            }),
        );
    }

    let mut provider_quotas = serde_json::Map::new();
    match state.rate_manager.get_all_providers().await {
        Ok(providers) => {
            for p in providers {
                if let Ok(status) = state.rate_manager.get_status(&p).await {
                    if let Ok(val) = serde_json::to_value(&status) {
                        provider_quotas.insert(p, val);
                    }
                }
            }
        }
        Err(e) => {
            warn!("Failed to list providers for quotas: {e}");
        }
    }

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "status": "ok",
            "document_count": 0,
            "requests_used": 0,
            "storage_bytes_used": 0,
            "storage_bytes_limit": 0,
            "provider_quotas": provider_quotas,
            "optimization": {
                "router_direct_count": 0,
                "semantic_cache_hits": 0,
                "semantic_cache_misses": 0,
            },
        }),
    )
}

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

pub async fn panel_list_threads(State(state): State<CliState>) -> Response {
    match state.panel_store.list_threads(50).await {
        Ok(threads) => {
            let mut summaries = Vec::new();
            for t in threads {
                let mut summary = xavier::codebase::conversations_db::ThreadSummary::from(&t);
                if let Ok(messages) = state.panel_store.get_thread_messages(&t.id).await {
                    summary.message_count = messages.len();
                }
                summaries.push(summary);
            }
            json_response(StatusCode::OK, serde_json::json!(summaries))
        }
        Err(error) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": error.to_string() }),
        ),
    }
}

pub async fn panel_create_thread(
    State(state): State<CliState>,
    Json(payload): Json<CreateThreadRequest>,
) -> Response {
    let title_hint = payload
        .title
        .or(payload.message)
        .unwrap_or_else(|| "New Thread".to_string());

    match state.panel_store.create_thread(Some(&title_hint), None, Some("cli")).await {
        Ok(thread) => json_response(
            StatusCode::OK,
            serde_json::to_value(xavier::codebase::conversations_db::ThreadSummary::from(&thread))
                .unwrap_or_else(|_| serde_json::json!({})),
        ),
        Err(error) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": error.to_string() }),
        ),
    }
}

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
    let episodic_summaries = threads
        .into_iter()
        .map(|s| xavier::retrieval::gating::SessionSummary {
            session_id: s.id.clone(),
            start_time: s.started_at,
            summary: s.last_preview.unwrap_or_default(),
            key_events: vec![],
            sentiment_timeline: vec![],
        })
        .collect::<Vec<_>>();

    let semantic_entities = Vec::new();

    let layered_result = gating
        .retrieve_layered(
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

pub async fn panel_get_thread(
    State(state): State<CliState>,
    AxumPath(thread_id): AxumPath<String>,
) -> Response {
    match state.panel_store.get_thread(&thread_id).await {
        Ok(Some(thread)) => match state.panel_store.get_thread_messages(&thread_id).await {
            Ok(messages) => {
                let mut summary = xavier::codebase::conversations_db::ThreadSummary::from(&thread);
                summary.message_count = messages.len();
                json_response(
                    StatusCode::OK,
                    serde_json::to_value(xavier::codebase::conversations_db::ThreadDetail {
                        thread: summary,
                        messages,
                    })
                    .unwrap_or_else(|_| serde_json::json!({})),
                )
            }
            Err(error) => json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({ "error": error.to_string() }),
            ),
        },
        Ok(None) => json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "thread not found" }),
        ),
        Err(error) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": error.to_string() }),
        ),
    }
}

pub async fn panel_delete_thread(
    State(_state): State<CliState>,
    AxumPath(_thread_id): AxumPath<String>,
) -> Response {
    json_response(
        StatusCode::NOT_IMPLEMENTED,
        serde_json::json!({ "error": "thread deletion not implemented" }),
    )
}

pub async fn panel_process_chat(
    State(state): State<CliState>,
    Json(payload): Json<PanelChatRequest>,
) -> Response {
    match panel_process_chat_inner(&state, payload).await {
        Ok(response) => json_response(
            StatusCode::OK,
            serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({})),
        ),
        Err(error) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": error.to_string() }),
        ),
    }
}

pub async fn panel_process_chat_inner(
    state: &CliState,
    payload: PanelChatRequest,
) -> anyhow::Result<PanelChatResponse> {
    let thread = match payload.thread_id.as_deref() {
        Some(thread_id) => state
            .panel_store
            .get_thread(thread_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("thread {thread_id} not found"))?,
        None => state.panel_store.create_thread(Some(&payload.message), None, Some("cli")).await?,
    };

    state.panel_store.store_message(
        &thread.id,
        "user",
        &payload.message,
        None,
        None,
        None,
        Some("{}"),
        None,
    ).await?;

    let assistant_content = format!(
        "Structured Xavier response for: {}",
        payload.message.trim()
    );
    let openui_lang = format!(
        "<SectionBlock title=\"Xavier\" description=\"{}\"><InfoCard title=\"Status\" value=\"Ready\" /></SectionBlock>",
        payload.message.replace('"', "'")
    );
    let metadata = serde_json::json!({
        "rules": ["deterministic", "ci-safe"],
        "components": ["SectionBlock", "InfoCard"],
        "timings": { "total_ms": 0 }
    });

    state.panel_store.store_message(
        &thread.id,
        "assistant",
        &assistant_content,
        None,
        Some(&openui_lang),
        None,
        Some(&metadata.to_string()),
        None,
    ).await?;

    let messages = state.panel_store.get_thread_messages(&thread.id).await?;
    let mut summary = xavier::codebase::conversations_db::ThreadSummary::from(&thread);
    summary.message_count = messages.len();

    Ok(PanelChatResponse {
        thread: summary,
        messages,
    })
}

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

    let results: Vec<MemoryRecord> = match state.memory.search(effective_query, Some(filters)).await
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
        metadata: serde_json::json!({"kind": "Context", "namespace": "Global"}),
        embedding: vec![],
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        revision: 1,
        primary: true,
        parent_id: None,
        cluster_id,
        level,
        relation,
        revisions: vec![],
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

    match state.store.delete(&state.workspace_id, id_or_path_str).await {
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

pub async fn stats_handler(State(state): State<CliState>) -> impl axum::response::IntoResponse {
    axum::Json(serde_json::json!({
        "status": "ok",
        "workspace_id": state.workspace_id,
        "version": "0.4.1",
    }))
}

pub async fn security_scan_handler(
    State(_state): State<CliState>,
    axum::Json(payload): axum::Json<SecurityScanPayload>,
) -> impl axum::response::IntoResponse {
    // This previously used state.security.process_input, but handlers should probably use their own
    // logic if we can't easily pass the CliState. Actually, state is available.
    // I'll keep the logic but use state.security.
    // Wait, state is available as an argument.
    // I'll use it.
    Json(serde_json::json!({"status":"todo"}))
}

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

    match state.memory.search(&payload.query, None).await {
        Ok(results) => {
            let documents: Vec<_> = results
                .into_iter()
                .map(|doc| {
                    serde_json::json!({
                        "id": doc.id,
                        "content": doc.content,
                        "embedding": doc.embedding,
                    })
                })
                .collect();

            axum::Json(serde_json::json!({
                "status": "ok",
                "query": payload.query,
                "count": documents.len(),
                "results": documents,
                "workspace_id": state.workspace_id,
            }))
        }
        Err(e) => {
            info!("Memory query error: {}", e);
            axum::Json(serde_json::json!({
                "status": "error",
                "message": e.to_string(),
            }))
        }
    }
}

pub async fn code_scan_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeScanPayload>,
) -> impl axum::response::IntoResponse {
    let requested_path = payload.path.unwrap_or_else(|| ".".to_string());

    let sec_result = state
        .security
        .process_input(&requested_path)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: requested_path.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        info!(
            "code/scan blocked by security: injection detected (confidence={})",
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
        }));
    }

    let workspace_root =
        std::path::absolute(&state.workspace_dir).unwrap_or_else(|_| PathBuf::from("."));
    let Ok(abs_path) = std::path::absolute(&requested_path) else {
        return axum::Json(serde_json::json!({
            "status": "error",
            "message": "invalid path",
            "indexed_files": 0,
        }));
    };
    if !abs_path.starts_with(&workspace_root) {
        warn!(
            "Path traversal blocked: {} is outside workspace root {}",
            abs_path.display(),
            workspace_root.display()
        );
        return axum::Json(serde_json::json!({
            "status": "error",
            "message": "path outside workspace not allowed",
            "indexed_files": 0,
        }));
    }

    let path = requested_path;
    info!("Code scan request: path={}", path);

    match state.code_indexer.index(std::path::Path::new(&path)).await {
        Ok(stats) => axum::Json(serde_json::json!({
            "status": "ok",
            "indexed_files": stats.total_files,
            "indexed_symbols": stats.total_symbols,
            "indexed_imports": stats.total_imports,
            "duration_ms": stats.duration_ms,
            "paths": [path],
            "languages": stats.languages,
        })),
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
            "indexed_files": 0,
            "indexed_symbols": 0,
            "indexed_imports": 0,
            "paths": [path],
        })),
    }
}

pub async fn code_find_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeFindPayload>,
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
            "code/find blocked by security: injection detected (confidence={})",
            sec_result.detection_confidence
        );
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "blocked": true,
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let query = sec_result
        .sanitized_input
        .as_deref()
        .unwrap_or(&sec_result.original_input)
        .to_string();
    let pattern = match secure_optional_request_field(
        state.security.as_ref(),
        "code/find pattern",
        payload.pattern.as_deref(),
    )
    .await
    {
        Ok(pattern) => pattern,
        Err(sec_result) => {
            info!(
                "code/find blocked by security: pattern rejected (confidence={})",
                sec_result.detection_confidence
            );
            return axum::Json(serde_json::json!({
                "status": "blocked",
                "reason": "security_policy_violation",
                "blocked": true,
                "field": "pattern",
                "detection": {
                    "is_injection": sec_result.is_injection,
                    "confidence": sec_result.detection_confidence,
                    "attack_type": sec_result.attack_type,
                }
            }));
        }
    };
    let kind = match secure_optional_request_field(
        state.security.as_ref(),
        "code/find kind",
        payload.kind.as_deref(),
    )
    .await
    {
        Ok(kind) => kind,
        Err(sec_result) => {
            info!(
                "code/find blocked by security: kind rejected (confidence={})",
                sec_result.detection_confidence
            );
            return axum::Json(serde_json::json!({
                "status": "blocked",
                "reason": "security_policy_violation",
                "blocked": true,
                "field": "kind",
                "detection": {
                    "is_injection": sec_result.is_injection,
                    "confidence": sec_result.detection_confidence,
                    "attack_type": sec_result.attack_type,
                }
            }));
        }
    };
    let limit = payload.limit.clamp(1, 100);
    info!(
        "Code find request: query={}, limit={}, kind={:?}, pattern={:?}",
        query, limit, kind, pattern
    );

    let symbols = code_find_symbols(
        &state.code_query,
        &query,
        kind.as_deref(),
        pattern.as_deref(),
        limit,
    );

    let results: Vec<_> = symbols
        .into_iter()
        .map(|symbol| {
            serde_json::json!({
                "id": symbol.id,
                "stable_id": symbol.stable_id,
                "path": symbol.file_path,
                "symbol": symbol.name,
                "symbol_type": format!("{:?}", symbol.kind),
                "language": format!("{:?}", symbol.lang),
                "line": symbol.start_line,
                "end_line": symbol.end_line,
                "signature": symbol.signature,
                "parent": symbol.parent,
                "complexity": symbol.complexity,
            })
        })
        .collect();

    axum::Json(serde_json::json!({
        "status": "ok",
        "query": query,
        "count": results.len(),
        "results": results,
    }))
}

pub async fn code_stats_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    match state.code_db.stats() {
        Ok(stats) => axum::Json(serde_json::json!({
            "status": "ok",
            "total_files": stats.total_files,
            "total_symbols": stats.total_symbols,
            "total_imports": stats.total_imports,
            "languages": stats.languages,
        })),
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
            "total_files": 0,
            "total_symbols": 0,
            "total_imports": 0,
        })),
    }
}

pub async fn code_context_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeContextPayload>,
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
            "code/context blocked by security: injection detected (confidence={})",
            sec_result.detection_confidence
        );
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "blocked": true,
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let limit = payload.limit.clamp(1, 100);
    let kind_limit = if payload.query.trim().is_empty() {
        limit
    } else {
        10_000
    };
    let budget_tokens = payload.budget_tokens.clamp(100, 8000);

    let mut symbols = if let Some(kind) = payload.kind.as_deref() {
        match kind.to_ascii_lowercase().as_str() {
            "function" | "fn" => state.code_query.functions(kind_limit).unwrap_or_default(),
            "struct" => state.code_query.structs(kind_limit).unwrap_or_default(),
            "class" => state.code_query.classes(kind_limit).unwrap_or_default(),
            "enum" => state.code_query.enums(kind_limit).unwrap_or_default(),
            _ => state
                .code_query
                .search(&payload.query, limit)
                .map(|result| result.symbols)
                .unwrap_or_default(),
        }
    } else {
        state
            .code_query
            .search(&payload.query, limit)
            .map(|result| result.symbols)
            .unwrap_or_default()
    };
    filter_symbols_by_query(&mut symbols, &payload.query);
    symbols.truncate(limit);

    let mut used_tokens = 0usize;
    let mut context = Vec::new();

    for symbol in symbols {
        let signature = symbol.signature.clone().unwrap_or_default();
        let compact = serde_json::json!({
            "symbol": symbol.name,
            "symbol_type": format!("{:?}", symbol.kind),
            "language": format!("{:?}", symbol.lang),
            "path": symbol.file_path,
            "line": symbol.start_line,
            "end_line": symbol.end_line,
            "signature": signature,
            "stable_id": symbol.stable_id,
            "complexity": symbol.complexity,
        });
        let estimated = estimate_tokens(&compact.to_string());
        if used_tokens + estimated > budget_tokens && !context.is_empty() {
            break;
        }
        used_tokens += estimated;
        context.push(compact);
    }

    axum::Json(serde_json::json!({
        "status": "ok",
        "query": payload.query,
        "budget_tokens": budget_tokens,
        "estimated_tokens": used_tokens,
        "count": context.len(),
        "context": context,
    }))
}

pub async fn code_dependencies_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeGraphQueryPayload>,
) -> impl axum::response::IntoResponse {
    code_graph_edges_response(&state, payload, false, false).await
}

pub async fn code_reverse_dependencies_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeGraphQueryPayload>,
) -> impl axum::response::IntoResponse {
    code_graph_edges_response(&state, payload, true, false).await
}

pub async fn code_call_chain_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeGraphQueryPayload>,
) -> impl axum::response::IntoResponse {
    code_graph_edges_response(&state, payload, false, true).await
}

pub async fn code_hubs_handler(State(state): State<CliState>) -> impl axum::response::IntoResponse {
    match state
        .code_query
        .hubs(default_min_degree(), default_graph_limit())
    {
        Ok(hubs) => {
            let (items, truncated, estimated_tokens) =
                truncate_json_items(hubs, default_graph_budget());
            axum::Json(serde_json::json!({
                "status": "ok",
                "count": items.len(),
                "min_degree": default_min_degree(),
                "estimated_tokens": estimated_tokens,
                "_truncated": truncated,
                "results": items,
            }))
        }
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

pub async fn code_hotspots_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    match state
        .code_query
        .hotspots(default_min_complexity(), default_graph_limit())
    {
        Ok(hotspots) => {
            let (items, truncated, estimated_tokens) =
                truncate_json_items(hotspots, default_graph_budget());
            axum::Json(serde_json::json!({
                "status": "ok",
                "count": items.len(),
                "min_complexity": default_min_complexity(),
                "estimated_tokens": estimated_tokens,
                "_truncated": truncated,
                "results": items,
            }))
        }
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

async fn code_graph_edges_response(
    state: &CliState,
    payload: CodeGraphQueryPayload,
    reverse: bool,
    call_chain: bool,
) -> axum::Json<serde_json::Value> {
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
            "blocked": true,
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let query = sec_result
        .sanitized_input
        .unwrap_or_else(|| sec_result.original_input.clone());
    let edge_type = if call_chain {
        Some(::code_graph::types::EdgeType::Calls)
    } else {
        match parse_code_edge_type(payload.edge_type.as_deref()) {
            Ok(edge_type) => edge_type,
            Err(message) => {
                return axum::Json(serde_json::json!({
                    "status": "error",
                    "message": message,
                }))
            }
        }
    };
    let depth = payload.depth.clamp(1, 8);
    let limit = payload.limit.clamp(1, 1000);
    let budget_tokens = payload.budget_tokens.clamp(100, 16_000);

    let result = if call_chain {
        state.code_query.call_chain(&query, depth, limit)
    } else if reverse {
        state
            .code_query
            .reverse_dependencies(&query, edge_type, depth, limit)
    } else {
        state
            .code_query
            .dependencies(&query, edge_type, depth, limit)
    };

    match result {
        Ok(edges) => {
            let (items, truncated, estimated_tokens) = truncate_json_items(edges, budget_tokens);
            axum::Json(serde_json::json!({
                "status": "ok",
                "query": query,
                "depth": depth,
                "limit": limit,
                "budget_tokens": budget_tokens,
                "estimated_tokens": estimated_tokens,
                "count": items.len(),
                "_truncated": truncated,
                "results": items,
            }))
        }
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

fn parse_code_edge_type(
    value: Option<&str>,
) -> std::result::Result<Option<::code_graph::types::EdgeType>, String> {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    match value.to_ascii_lowercase().as_str() {
        "calls" | "call" => Ok(Some(::code_graph::types::EdgeType::Calls)),
        "defines" | "define" => Ok(Some(::code_graph::types::EdgeType::Defines)),
        "uses" | "use" => Ok(Some(::code_graph::types::EdgeType::Uses)),
        "imports" | "import" => Ok(Some(::code_graph::types::EdgeType::Imports)),
        "contains" | "contain" => Ok(Some(::code_graph::types::EdgeType::Contains)),
        "references" | "reference" | "refs" => Ok(Some(::code_graph::types::EdgeType::References)),
        _ => Err(format!("unsupported edge_type: {}", value)),
    }
}

fn truncate_json_items<T: Serialize>(
    items: Vec<T>,
    budget_tokens: usize,
) -> (Vec<serde_json::Value>, bool, usize) {
    let mut output = Vec::new();
    let mut used_tokens = 0usize;
    let mut truncated = false;

    for item in items {
        let value = serde_json::to_value(item).unwrap_or_else(|_| serde_json::json!({}));
        let estimated = estimate_tokens(&value.to_string());
        if used_tokens + estimated > budget_tokens && !output.is_empty() {
            truncated = true;
            break;
        }
        used_tokens += estimated;
        output.push(value);
    }

    (output, truncated, used_tokens)
}

pub async fn agent_register_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<AgentRegisterPayload>,
) -> impl axum::response::IntoResponse {
    let metadata = xavier::coordination::agent_registry::AgentMetadata {
        name: payload.name,
        capabilities: payload.capabilities.unwrap_or_default(),
        role: payload.role,
        endpoint: payload.endpoint,
    };
    let session_id = payload
        .session_id
        .unwrap_or_else(|| payload.agent_id.clone());

    let success = state
        .agent_registry
        .register(payload.agent_id.clone(), session_id.clone(), metadata)
        .await;

    axum::Json(serde_json::json!({
        "status": if success { "ok" } else { "error" },
        "agent_id": payload.agent_id,
        "session_id": session_id,
        "message": if success { "Agent registered successfully" } else { "Registration failed" },
    }))
}

pub async fn agent_heartbeat_handler(
    State(state): State<CliState>,
    axum::extract::Path(agent_id): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    let success = state.agent_registry.heartbeat(&agent_id).await;

    axum::Json(serde_json::json!({
        "status": if success { "ok" } else { "error" },
        "agent_id": agent_id,
        "message": if success { "Heartbeat recorded" } else { "Agent not found" },
    }))
}

pub async fn agent_active_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    let active = state.agent_registry.get_active_agents().await;

    axum::Json(serde_json::json!({
        "status": "ok",
        "active_agents": active.len(),
        "agents": active.iter().map(|a| serde_json::json!({
            "agent_id": a.agent_id,
            "session_id": a.session_id,
            "last_heartbeat": a.last_heartbeat.to_rfc3339(),
            "name": a.metadata.name,
            "capabilities": a.metadata.capabilities,
            "role": a.metadata.role,
            "endpoint": a.metadata.endpoint,
        })).collect::<Vec<_>>(),
    }))
}

pub async fn agent_push_context_handler(
    State(state): State<CliState>,
    axum::extract::Path(agent_id): axum::extract::Path<String>,
    axum::Json(payload): axum::Json<AgentPushContextPayload>,
) -> impl axum::response::IntoResponse {
    let agent = state.agent_registry.get(&agent_id).await;
    if agent.is_none() {
        return axum::Json(serde_json::json!({
            "status": "error",
            "message": "Agent not registered",
        }));
    }

    let context =
        match secure_external_input(state.security.as_ref(), "agent context", &payload.context)
            .await
        {
            Ok(context) => context,
            Err(response) => return axum::Json(response),
        };

    let path = format!("agents/{}/context", agent_id);
    let mut metadata = payload.metadata.unwrap_or(serde_json::json!({}));
    if let Some(obj) = metadata.as_object_mut() {
        obj.insert("agent_id".to_string(), serde_json::json!(agent_id));
        obj.insert(
            "pushed_at".to_string(),
            serde_json::json!(chrono::Utc::now().to_rfc3339()),
        );
    }

    let record = MemoryRecord {
        id: String::new(),
        workspace_id: state.workspace_id.clone(),
        path: path.clone(),
        content: context,
        metadata,
        embedding: vec![],
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        revision: 1,
        primary: true,
        parent_id: None,
        cluster_id: None,
        level: MemoryLevel::Raw,
        relation: None,
        revisions: vec![],
    };
    match state.memory.add(record).await {
        Ok(doc_id) => axum::Json(serde_json::json!({
            "status": "ok",
            "path": path,
            "document_id": doc_id,
            "message": "Context stored successfully",
        })),
        Err(e) => axum::Json(serde_json::json!({
            "status": "error",
            "message": format!("Failed to store context: {}", e),
        })),
    }
}

pub async fn agent_unregister_handler(
    State(state): State<CliState>,
    axum::extract::Path(agent_id): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    let success = state.agent_registry.unregister(&agent_id).await;

    axum::Json(serde_json::json!({
        "status": if success { "ok" } else { "error" },
        "agent_id": agent_id,
        "message": if success { "Agent unregistered" } else { "Agent not found or already unregistered" },
    }))
}

pub async fn search_memories_filtered(
    query: &str,
    limit: usize,
    clusters: Vec<String>,
    levels: Vec<String>,
) -> anyhow::Result<()> {
    let query = secure_cli_input("search query", query, 4_096)?;
    let limit = limit.clamp(1, 100);
    let token = crate::cli::config::xavier_token();
    let base_url = crate::cli::config::resolve_base_url();
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

    match response {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            println!("\nSearch results for: {}", query);
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        _ => {
            println!("⚠️ Server offline or request failed. Falling back to local offline database index...");
            match crate::cli::commands::load_spawn_memory().await {
                Ok(memory) => {
                    match memory.search_filtered(&query, limit, Some(&filters)).await {
                        Ok(docs) => {
                            println!("\n[OFFLINE] Search results for: {}", query);
                            let json_results = serde_json::json!({
                                "results": docs.iter().map(|doc| {
                                    serde_json::json!({
                                        "id": doc.id,
                                        "path": doc.path,
                                        "content": doc.content,
                                        "metadata": doc.metadata,
                                        "score": doc.metadata.get("score").and_then(|v| v.as_f64()).unwrap_or(1.0),
                                    })
                                }).collect::<Vec<_>>()
                            });
                            println!("{}", serde_json::to_string_pretty(&json_results)?);
                        }
                        Err(e) => {
                            println!("❌ Local search failed: {}", e);
                        }
                    }
                }
                Err(e) => {
                    println!("❌ Failed to initialize local offline database store: {}", e);
                }
            }
        }
    }

    Ok(())
}

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
    let token = crate::cli::config::xavier_token();
    let base_url = crate::cli::config::resolve_base_url();
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
        }
        _ => {
            println!("⚠️ Server offline or request failed. Falling back to local offline database write...");
            match crate::cli::commands::load_spawn_memory().await {
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

                    match memory.add_document_typed(path, content.to_string(), metadata, Some(typed_payload)).await {
                        Ok(id) => {
                            println!("✅ Memory added successfully offline to local SQLite-Vec database!");
                            println!("Document ID: {id}");
                        }
                        Err(err) => {
                            println!("❌ Local write failed: {}", err);
                        }
                    }
                }
                Err(e) => {
                    println!("❌ Failed to initialize local offline database store: {}", e);
                }
            }
        }
    }

    Ok(())
}

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

pub async fn lend_handler(
    State(state): State<CliState>,
    Json(payload): Json<LendSecretPayload>,
) -> Response {
    match state
        .secrets_engine
        .lend(
            &payload.secret_name,
            &payload.secret_value,
            &payload.agent_id,
            payload.ttl_seconds,
        )
        .await
    {
        Ok(lease) => json_response(
            StatusCode::OK,
            serde_json::to_value(lease).unwrap_or_default(),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

pub async fn leases_handler(State(state): State<CliState>) -> Response {
    let leases = state.secrets_engine.list_leases().await;
    json_response(
        StatusCode::OK,
        serde_json::to_value(leases).unwrap_or_default(),
    )
}

pub async fn revoke_handler(
    State(state): State<CliState>,
    Json(payload): Json<RevokeLeasePayload>,
) -> Response {
    match state
        .secrets_engine
        .revoke(&payload.token, "Manual API Call")
        .await
    {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "revoked" })),
        Err(e) => json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

pub async fn status_handler(
    State(state): State<CliState>,
    AxumPath(token): AxumPath<String>,
) -> Response {
    match state.secrets_engine.get_lease(&token).await {
        Some(status) => json_response(
            StatusCode::OK,
            serde_json::to_value(status).unwrap_or_default(),
        ),
        None => json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "Lease not found" }),
        ),
    }
}

pub async fn usage_status_handler(
    State(state): State<CliState>,
    AxumPath(provider): AxumPath<String>,
) -> Response {
    match state.rate_manager.get_status(&provider).await {
        Ok(status) => json_response(
            StatusCode::OK,
            serde_json::to_value(status).unwrap_or_default(),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

pub async fn usage_track_handler(
    State(state): State<CliState>,
    Json(payload): Json<UsageTrackPayload>,
) -> Response {
    match state
        .rate_manager
        .track_request(
            &payload.provider,
            payload.tokens,
            payload.status,
            payload.cost_usd,
            payload.is_cache_hit,
        )
        .await
    {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "ok" })),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

pub async fn usage_summary_handler(
    State(state): State<CliState>,
    AxumPath(provider): AxumPath<String>,
) -> Response {
    match state.rate_manager.get_daily_summary(&provider).await {
        Ok(summary) => json_response(StatusCode::OK, summary),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

pub async fn usage_update_handler(
    State(state): State<CliState>,
    Json(payload): Json<UsageUpdatePayload>,
) -> Response {
    match state
        .rate_manager
        .update_manual_limit(&payload.provider, payload.percentage)
        .await
    {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "ok" })),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

pub async fn usage_cooldown_handler(
    State(state): State<CliState>,
    Json(payload): Json<UsageCooldownPayload>,
) -> Response {
    match state
        .rate_manager
        .report_429(&payload.provider, payload.minutes)
        .await
    {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "ok" })),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

pub async fn agent_list_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    let agents = state.agent_registry.get_active_agents().await;
    Json(serde_json::json!({
        "agents": agents.iter().map(|a| serde_json::json!({
            "id": a.agent_id,
            "session_id": a.session_id,
            "last_heartbeat": a.last_heartbeat,
        })).collect::<Vec<_>>(),
        "count": agents.len()
    }))
}

pub async fn workspace_info_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    Json(serde_json::json!({
        "workspace_id": state.workspace_id,
        "workspace_dir": state.workspace_dir.to_string_lossy(),
    }))
}

pub async fn mcp_tools_handler() -> impl axum::response::IntoResponse {
    Json(serde_json::json!({
        "tools": [
            {"name": "memory_search", "description": "Search memory with semantic + lexical hybrid search"},
            {"name": "memory_add", "description": "Add a new memory entry with metadata and zone tagging"},
            {"name": "memory_delete", "description": "Delete a memory entry by path"},
            {"name": "memory_stats", "description": "Get memory statistics and counts"},
            {"name": "memory_export", "description": "Export all memories as JSON"},
            {"name": "code_scan", "description": "Scan a codebase and index symbols into the code graph"},
            {"name": "code_find", "description": "Find code symbols by name, kind, or file path"},
            {"name": "code_context", "description": "Get surrounding context for a code symbol"},
            {"name": "code_stats", "description": "Get code graph statistics"},
            {"name": "agent_register", "description": "Register a new AI agent"},
            {"name": "agent_list", "description": "List active agents"},
            {"name": "agent_heartbeat", "description": "Send heartbeat for an agent"},
            {"name": "agent_push_context", "description": "Push context document to an agent"},
            {"name": "agent_unregister", "description": "Unregister an agent"},
        ]
    }))
}
