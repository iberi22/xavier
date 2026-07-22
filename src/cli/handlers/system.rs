//! System handlers for health, version, readiness, and build information.

use crate::cli::config::{require_xavier_token, resolve_base_url};
use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use axum::{extract::State, http::StatusCode, response::Response};
use xavier::server::alerts::SYSTEM_ALERTS;

/// Health handler.
pub async fn health_handler() -> Response {
    let status = xavier::observability::health::HEALTH.get_status().await;
    json_response(
        StatusCode::OK,
        serde_json::to_value(status).unwrap_or_default(),
    )
}

/// System alerts handler.
pub async fn system_alerts_handler() -> Response {
    json_response(
        StatusCode::OK,
        serde_json::json!({
            "alerts": SYSTEM_ALERTS.get_alerts()
        }),
    )
}

/// Handle health command.
pub async fn handle_health_command(cloud: bool) -> anyhow::Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    if cloud {
        let resp = client
            .get(format!("{}/health/cloud", base_url))
            .header("X-Xavier-Token", &token)
            .send()
            .await?;

        if resp.status().is_success() {
            let data: xavier::health::CloudHealthResponse = resp.json().await?;
            println!("═══════════════════════════════════════════");
            println!("  Cloud Backend Health");
            println!("═══════════════════════════════════════════");
            println!("  Supabase:    {}", format_status(&data.supabase));
            println!("    Detail:    {}", data.supabase.detail);
            println!("  Postgres:    {}", format_status(&data.postgres));
            println!("    Detail:    {}", data.postgres.detail);
            println!("═══════════════════════════════════════════");
        } else {
            println!("❌ Failed to fetch cloud health: {}", resp.status());
        }
    } else {
        let resp = client
            .get(format!("{}/health", base_url))
            .header("X-Xavier-Token", &token)
            .send()
            .await?;

        if resp.status().is_success() {
            let data: serde_json::Value = resp.json().await?;
            println!("═══════════════════════════════════════════");
            println!("  System Health Status");
            println!("═══════════════════════════════════════════");
            println!(
                "  Status:      {}",
                data["status"].as_str().unwrap_or("unknown")
            );
            println!(
                "  Version:     {}",
                data["version"].as_str().unwrap_or("unknown")
            );
            println!("═══════════════════════════════════════════");
        } else {
            println!("❌ Failed to fetch health status: {}", resp.status());
        }
    }

    Ok(())
}

fn format_status(status: &xavier::health::BackendStatus) -> String {
    match status.status.as_str() {
        "healthy" => "✅ Healthy".to_string(),
        "unhealthy" => "❌ Unhealthy".to_string(),
        "not configured" => "⚪ Not Configured".to_string(),
        _ => "❓ Unknown".to_string(),
    }
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

/// Cloud health handler.
pub async fn cloud_health_handler() -> Response {
    // Use library settings to avoid type mismatch with health check function
    let settings = xavier::settings::XavierSettings::current();
    let health = xavier::health::check_cloud_health(&settings).await;
    json_response(
        StatusCode::OK,
        serde_json::to_value(health).unwrap_or_default(),
    )
}

/// Version handler.
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

/// Readiness handler.
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
    let code_graph_state = state.code_graph.read().await;
    let code_graph = code_graph_state
        .db
        .stats()
        .map(|stats| {
            serde_json::json!({
                "ready": true,
                "total_files": stats.total_files,
                "total_symbols": stats.total_symbols,
                "total_imports": stats.total_imports,
            })
        })
        .unwrap_or_else(|error: code_graph::GraphError| {
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

/// Build handler.
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
