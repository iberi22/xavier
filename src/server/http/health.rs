// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Health and build information endpoints.
//!
//! This module provides diagnostic endpoints to monitor the server's status,
//! verify readiness of various components (embeddings, LLM, storage), and retrieve
//! build-time metadata.

use crate::agents::provider::ModelProviderClient;
use crate::memory::sqlite_vec_store::VecSqliteMemoryStore;
use crate::AppState;
use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ReadinessComponent {
    pub configured: bool,
    pub ready: bool,
    pub detail: String,
}
#[derive(Debug, Serialize)]
pub struct ReadinessResponse {
    pub status: String,
    pub service: String,
    pub version: String,
    pub workspace: ReadinessComponent,
    pub memory_store: ReadinessComponent,
    pub code_graph: ReadinessComponent,
    pub embeddings: ReadinessComponent,
    pub llm: ReadinessComponent,
}
#[derive(Debug, Serialize)]
pub struct MemoryStoreBuildInfo {
    pub selected_backend: String,
    pub backend: String,
    pub migrated_from_file: bool,
    pub migration_detail: String,
    pub rrf_k: usize,
    pub entity_extraction_enabled: bool,
    pub qjl_threshold: usize,
    pub audit_chain_enabled: bool,
}
#[derive(Debug, Serialize)]
pub struct BuildInfoResponse {
    pub service: String,
    pub version: String,
    pub rust_log: Option<String>,
    pub xavier_log_level: Option<String>,
    pub model_provider: crate::agents::provider::ModelProviderStatus,
    pub memory_store: MemoryStoreBuildInfo,
}

pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let workspace = state.workspace_registry.default_context().await;
    let mut lag_ms = 0;
    let mut hormer_metrics = serde_json::json!(null);

    if let Some(ref context) = workspace {
        lag_ms = crate::tasks::session_sync_task::calculate_indexing_lag(
            context.workspace.durable_store().as_ref(),
            &context.workspace_id,
        )
        .await;
        hormer_metrics = context.workspace.hormer.get_metrics().await;
    }

    let xavier_health = crate::health::collect_health_sync();

    Json(serde_json::json!({
        "status": xavier_health.status,
        "service": "xavier",
        "version": env!("CARGO_PKG_VERSION"),
        "lag_ms": lag_ms,
        "hormer": hormer_metrics,
        "health": xavier_health
    }))
}

pub async fn readiness(State(state): State<AppState>) -> impl IntoResponse {
    let workspace_context = state.workspace_registry.default_context().await;
    let workspace_ready = workspace_context.is_some();
    let embedding_configured = crate::memory::embedder::EmbeddingClient::is_configured_from_env();
    let embeddings = match crate::memory::embedder::EmbeddingClient::from_env() {
        Ok(client) if embedding_configured => match client.health().await {
            Ok(true) => ReadinessComponent {
                configured: true,
                ready: true,
                detail: "embedding service reachable".to_string(),
            },
            Ok(false) => ReadinessComponent {
                configured: true,
                ready: false,
                detail: "embedding service responded without vectors".to_string(),
            },
            Err(error) => ReadinessComponent {
                configured: true,
                ready: false,
                detail: error.to_string(),
            },
        },
        Ok(_) => ReadinessComponent {
            configured: false,
            ready: true,
            detail: "embedding service not configured".to_string(),
        },
        Err(error) if embedding_configured => ReadinessComponent {
            configured: true,
            ready: false,
            detail: error.to_string(),
        },
        Err(_) => ReadinessComponent {
            configured: false,
            ready: true,
            detail: "embedding service not configured".to_string(),
        },
    };
    let llm_status = ModelProviderClient::from_env().status();
    let llm = ReadinessComponent {
        configured: llm_status.configured,
        ready: llm_status.configured,
        detail: format!(
            "provider={} model={}",
            llm_status.provider, llm_status.model
        ),
    };
    let workspace = ReadinessComponent {
        configured: true,
        ready: workspace_ready,
        detail: if workspace_ready {
            "default workspace loaded".to_string()
        } else {
            "default workspace is not available".to_string()
        },
    };
    let memory_store = match workspace_context {
        Some(workspace) => match workspace.workspace.durable_store_health().await {
            Ok(detail) => ReadinessComponent {
                configured: true,
                ready: true,
                detail: format!(
                    "{detail}; migration={}",
                    workspace.workspace.durable_store_migration_detail()
                ),
            },
            Err(error) => ReadinessComponent {
                configured: true,
                ready: false,
                detail: error.to_string(),
            },
        },
        None => ReadinessComponent {
            configured: true,
            ready: false,
            detail: "default workspace is not available".to_string(),
        },
    };
    let code_graph = ReadinessComponent {
        configured: false,
        ready: true,
        detail: "code graph not available in CLI mode".to_string(),
    };
    let ready = workspace.ready
        && memory_store.ready
        && code_graph.ready
        && (!embeddings.configured || embeddings.ready)
        && (!llm.configured || llm.ready);
    Json(ReadinessResponse {
        status: if ready { "ok" } else { "degraded" }.to_string(),
        service: "xavier".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        workspace,
        memory_store,
        code_graph,
        embeddings,
        llm,
    })
}

pub async fn build_info(State(state): State<AppState>) -> impl IntoResponse {
    let workspace = state.workspace_registry.default_context().await;
    Json(BuildInfoResponse {
        service: "xavier".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        rust_log: std::env::var("RUST_LOG").ok(),
        xavier_log_level: std::env::var("XAVIER_LOG_LEVEL").ok(),
        model_provider: ModelProviderClient::from_env().status(),
        memory_store: workspace
            .map(|workspace| MemoryStoreBuildInfo {
                selected_backend: workspace
                    .workspace
                    .config()
                    .memory_backend
                    .as_str()
                    .to_string(),
                backend: workspace.workspace.durable_store_backend().to_string(),
                migrated_from_file: workspace.workspace.durable_store_migrated_from_file(),
                migration_detail: workspace
                    .workspace
                    .durable_store_migration_detail()
                    .to_string(),
                rrf_k: VecSqliteMemoryStore::configured_rrf_k(),
                entity_extraction_enabled: VecSqliteMemoryStore::entity_extraction_enabled(),
                qjl_threshold: VecSqliteMemoryStore::configured_qjl_threshold(),
                audit_chain_enabled: VecSqliteMemoryStore::audit_chain_enabled(),
            })
            .unwrap_or(MemoryStoreBuildInfo {
                selected_backend: std::env::var("XAVIER_MEMORY_BACKEND")
                    .map(|value| {
                        crate::memory::store::MemoryBackend::from_env(&value)
                            .as_str()
                            .to_string()
                    })
                    .unwrap_or_else(|_| "vec".to_string()),
                backend: "unavailable".to_string(),
                migrated_from_file: false,
                migration_detail: "default workspace is not available".to_string(),
                rrf_k: VecSqliteMemoryStore::configured_rrf_k(),
                entity_extraction_enabled: VecSqliteMemoryStore::entity_extraction_enabled(),
                qjl_threshold: VecSqliteMemoryStore::configured_qjl_threshold(),
                audit_chain_enabled: VecSqliteMemoryStore::audit_chain_enabled(),
            }),
    })
}
