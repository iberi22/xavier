//! System handlers for health, version, readiness, and build information.

use crate::cli::config::{require_xavier_token, resolve_base_url};
use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use axum::{extract::State, http::StatusCode, response::Response};
use xavier::server::alerts::SYSTEM_ALERTS;

/// Health handler.
///
/// Devuelve el estado CACHEADO del monitor, que se recalcula en segundo plano cada 60 s
/// (`HEALTH.spawn()` en el arranque del servidor).
///
/// Antes ejecutaba `run_checks()` completo en el camino de la peticion: eso recorre la base de
/// datos, el indice vectorial, el embedder y el proveedor de LLM. Con un almacen grande
/// (~35.5k documentos) la suma pasa del corte de 10 s del middleware de timeout y `/health`
/// respondia **504**, asi que el nodo con datos parecia caido justo cuando mas importaba
/// (una demo, un monitor de salud, un orquestador). El nodo con el almacen vacio respondia en
/// 0.3 s, que es lo que enmascaraba el problema.
///
/// Serializes `xavier::observability::health::HealthStatus` as-is: the `version` and
/// `degraded_reasons` that `xavier health` reads come from there, nothing is injected here.
pub async fn health_handler() -> Response {
    let status = xavier::observability::health::HEALTH.get_status().await;
    json_response(
        StatusCode::OK,
        serde_json::to_value(status).unwrap_or_default(),
    )
}

/// /healthz — lightweight liveness probe with embedder reachability.
///
/// Reports `"ok"`, `"degraded"`, or `"down"` for the embedding subsystem
/// so orchestrators can route traffic away from degraded instances.
/// Usa tambien el estado cacheado: una sonda de vida no puede permitirse el mismo trabajo que
/// el chequeo completo (era el mismo 504 que en `/health`).
pub async fn healthz_handler() -> Response {
    use xavier::observability::health::HEALTH;

    let status = HEALTH.get_status().await;
    let embedder_status = match status.embedding.status {
        xavier::observability::health::HealthLevel::Healthy => "ok",
        xavier::observability::health::HealthLevel::Degraded => "degraded",
        xavier::observability::health::HealthLevel::Unhealthy => "down",
    };

    let overall = if embedder_status == "down" {
        "degraded"
    } else {
        "ok"
    };

    let http_status = if embedder_status == "down" {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };

    json_response(
        http_status,
        serde_json::json!({
            "status": overall,
            "embedder": {
                "status": embedder_status,
                "provider": status.embedding.provider,
                "model": status.embedding.model,
                "latency_ms": status.embedding.latency_ms,
            },
        }),
    )
}

/// Health history handler.
pub async fn health_history_handler() -> Response {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let history = xavier_lib::health::history::fetch_health_history(now_secs);
    json_response(
        StatusCode::OK,
        serde_json::to_value(history).unwrap_or_default(),
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

/// Renders the box-drawing system health report and flags an unhealthy node.
///
/// `data` is the raw `GET /health` body, i.e. `HealthStatus` as serialized by
/// `health_handler`. Returns the report plus `true` when the node reports
/// `unhealthy`, so the command can exit non-zero in scripts and CI.
pub fn render_health_report(data: &serde_json::Value) -> (String, bool) {
    let status_str = data
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unreported");

    // The server build wins; the CLI only fills in for a payload that carries none,
    // and it says so, because a bare version number from the other process is not
    // the same fact as one from this one.
    let (version_str, version_note) = match data
        .get("version")
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
    {
        Some(version) => (version.to_string(), None),
        None => (
            env!("CARGO_PKG_VERSION").to_string(),
            Some("cli version — server payload had no `version`"),
        ),
    };

    let mut out = String::new();
    out.push_str("═══════════════════════════════════════════\n");
    out.push_str("  System Health Status\n");
    out.push_str("═══════════════════════════════════════════\n");
    out.push_str(&format!("  Status:      {}\n", status_str));
    out.push_str(&format!("  Version:     {}", version_str));
    if let Some(note) = version_note {
        out.push_str(&format!(" ({})", note));
    }
    out.push('\n');

    if status_str != "healthy" {
        let reasons: Vec<&str> = data
            .get("degraded_reasons")
            .and_then(|r| r.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        if reasons.is_empty() {
            out.push_str(&format!(
                "  Reasons:     none reported by server for status `{}`\n",
                status_str
            ));
        } else {
            out.push_str("  Reasons:\n");
            for reason in reasons {
                let clean: String = reason.chars().filter(|c| !c.is_control()).collect();
                out.push_str(&format!("    - {}\n", clean));
            }
        }
    }
    out.push_str("═══════════════════════════════════════════");

    let is_unhealthy = status_str == "unhealthy";
    (out, is_unhealthy)
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

            if data.supabase.status == "unhealthy" || data.postgres.status == "unhealthy" {
                anyhow::bail!("Cloud backend is unhealthy");
            }
        } else {
            println!("❌ Failed to fetch cloud health: {}", resp.status());
            anyhow::bail!("Cloud health check failed with HTTP {}", resp.status());
        }
    } else {
        let resp = client
            .get(format!("{}/health", base_url))
            .header("X-Xavier-Token", &token)
            .send()
            .await?;

        if resp.status().is_success() {
            let data: serde_json::Value = resp.json().await?;
            let (report, is_unhealthy) = render_health_report(&data);
            println!("{}", report);

            if is_unhealthy {
                anyhow::bail!("System health status is unhealthy");
            }
        } else {
            println!("❌ Failed to fetch health status: {}", resp.status());
            anyhow::bail!("Health check failed with HTTP {}", resp.status());
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

/// Formats system stats into a canonical JSON payload matching MCP `xavier_stats`.
pub fn format_system_stats(
    workspace_id: &str,
    total_memories: usize,
    total_entities: usize,
    semantic_entities: usize,
    semantic_relations: usize,
    storage_bytes: u64,
) -> serde_json::Value {
    let semantic_layer_state = if semantic_entities == 0 {
        "unpopulated"
    } else {
        "populated"
    };

    serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "workspace_id": workspace_id,
        "total_memories": total_memories,
        "total_entities": total_entities,
        "semantic_entities": semantic_entities,
        "semantic_relations": semantic_relations,
        "storage_bytes": storage_bytes,
        "semantic_layer_state": semantic_layer_state,
    })
}

/// Calculates and formats system/workspace statistics matching MCP `xavier_stats`.
///
/// Every counter comes from the SAME workspace when one is present. Mixing
/// `state.qmd_memory` (the port built in `server.rs`) with `ws.workspace.*` compared two
/// different stores, so `total_memories` came from one and the entity/semantic numbers
/// from another: "parity" with MCP was a coincidence. `tools_memory.rs` reads
/// `workspace.workspace.memory.usage()`, so that is what is read here too.
pub async fn calculate_system_stats(
    state: &CliState,
    workspace: Option<&xavier::workspace::WorkspaceContext>,
) -> serde_json::Value {
    // Workspace-level counters — only available when the full workspace layer is wired.
    let (total_memories, total_entities, semantic_entities, semantic_relations, storage_bytes) =
        match workspace {
            Some(ws) => {
                let usage = ws.workspace.memory.usage().await;
                let entity_count = ws.workspace.entity_graph.all_entities().await.len();
                let semantic_stats = ws.workspace.semantic_memory.stats().await;
                let working_mem_len = ws.workspace.working_memory.read().await.len();
                (
                    usage.document_count.max(working_mem_len),
                    entity_count,
                    semantic_stats.total_entities,
                    semantic_stats.total_relations,
                    usage.storage_bytes,
                )
            }
            None => {
                let usage = state.qmd_memory.usage().await;
                (usage.document_count, 0, 0, 0, usage.storage_bytes)
            }
        };

    let mut stats = format_system_stats(
        &state.workspace_id,
        total_memories,
        total_entities,
        semantic_entities,
        semantic_relations,
        storage_bytes,
    );
    if workspace.is_none() {
        // Without a workspace the semantic counters are placeholders, not measurements.
        stats["semantic_layer_state"] = serde_json::json!("unknown");
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use xavier::observability::health::{HealthLevel, HealthStatus};

    /// The `HealthStatus` the live `GET /health` serves, serialized exactly as
    /// `health_handler` does — no hand-written JSON, so the report is exercised against
    /// the payload shape production actually emits.
    fn serialized(status: HealthStatus) -> serde_json::Value {
        serde_json::to_value(status).expect("HealthStatus must serialize")
    }

    #[test]
    fn health_report_shows_version_and_reasons_from_real_payload() {
        let mut node = HealthStatus {
            status: HealthLevel::Degraded,
            ..Default::default()
        };
        node.embedding.status = HealthLevel::Degraded;
        node.system.ram_usage_percent = 91.0;
        node.degraded_reasons = node.compute_degraded_reasons();

        let payload = serialized(node);
        assert_eq!(payload["version"], env!("CARGO_PKG_VERSION"));

        let (report, is_unhealthy) = render_health_report(&payload);
        assert!(!is_unhealthy, "degraded must not be treated as unhealthy");
        assert!(
            report.contains(env!("CARGO_PKG_VERSION")),
            "report must show the server version, got:\n{report}"
        );
        assert!(
            report.contains("subsystem:embedding"),
            "report must name the degraded subsystem, got:\n{report}"
        );
        assert!(
            report.contains("host:memory"),
            "report must name the host resource, got:\n{report}"
        );
        assert!(!report.contains("unknown"), "got:\n{report}");
    }

    #[test]
    fn healthy_status_reports_no_degraded_reasons() {
        let node = HealthStatus::default();
        assert_eq!(node.status, HealthLevel::Healthy);
        assert!(
            node.compute_degraded_reasons().is_empty(),
            "a healthy node must report no reasons"
        );

        let payload = serialized(node);
        assert_eq!(
            payload["degraded_reasons"].as_array().map(Vec::len),
            Some(0)
        );

        let (report, is_unhealthy) = render_health_report(&payload);
        assert!(!is_unhealthy);
        assert!(!report.contains("Reasons:"), "got:\n{report}");
    }

    #[test]
    fn health_report_falls_back_to_cli_version_when_server_omits_it() {
        let (report, _) = render_health_report(&serde_json::json!({
            "status": "degraded",
        }));
        assert!(report.contains(env!("CARGO_PKG_VERSION")), "got:\n{report}");
        assert!(
            report.contains("cli version"),
            "the fallback must say which process it came from, got:\n{report}"
        );
        // A degraded node with no reasons must say so rather than print an empty block.
        assert!(report.contains("none reported by server"), "got:\n{report}");
    }

    #[test]
    fn stats_payload_matches_mcp_field_contract() {
        let stats = format_system_stats("default", 1482, 20, 0, 0, 10956148);

        let mut keys: Vec<&str> = stats
            .as_object()
            .expect("stats must be an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "semantic_entities",
                "semantic_layer_state",
                "semantic_relations",
                "status",
                "storage_bytes",
                "total_entities",
                "total_memories",
                "version",
                "workspace_id",
            ],
            "CLI stats must carry every MCP `xavier_stats` field (and no drift)"
        );

        // The five substantive counters must survive serialization as numbers, not
        // strings or nulls: a CLI printing `"total_memories": null` is the same defect
        // as omitting the field.
        for field in [
            "total_memories",
            "total_entities",
            "semantic_entities",
            "semantic_relations",
            "storage_bytes",
        ] {
            assert!(
                stats[field].is_number(),
                "{field} must be a number, got {:?}",
                stats[field]
            );
        }
        assert_eq!(stats["total_memories"], 1482);
        assert_eq!(stats["total_entities"], 20);
        assert_eq!(stats["storage_bytes"], 10956148);
    }

    #[test]
    fn stats_reports_semantic_layer_state() {
        // The signal the wave exists to surface: entities extracted, graph layer empty.
        let unpopulated = format_system_stats("default", 1482, 20, 0, 0, 10956148);
        assert_eq!(unpopulated["semantic_entities"], 0);
        assert_eq!(unpopulated["semantic_layer_state"], "unpopulated");

        let populated = format_system_stats("default", 1482, 20, 15, 8, 10956148);
        assert_eq!(populated["semantic_layer_state"], "populated");
    }
}
