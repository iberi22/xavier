//! System handlers for health, version, readiness, and build information.

use crate::cli::config::resolve_base_url;
use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use axum::{extract::State, http::StatusCode, response::Response};
use xavier::server::alerts::SYSTEM_ALERTS;

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

pub async fn system_alerts_handler() -> Response {
    json_response(
        StatusCode::OK,
        serde_json::json!({
            "alerts": SYSTEM_ALERTS.get_alerts()
        }),
    )
}

#[allow(dead_code)]
pub async fn system_scan_handler(State(state): State<CliState>) -> Response {
    let mut providers = Vec::new();
    let detected_providers = vec!["openai", "anthropic", "gemini", "minimax", "local"];

    for p in detected_providers {
        let client = xavier::agents::provider::ModelProviderClient::for_provider(p, None);
        let status = client.status();
        providers.push(serde_json::json!({
            "name": p,
            "configured": status.configured,
            "model": status.model,
        }));
    }

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "providers": providers,
            "workspace_id": state.workspace_id,
            "memory_backend": crate::settings::XavierSettings::current().memory.backend,
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
