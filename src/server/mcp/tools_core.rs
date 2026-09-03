//! Core MCP tool implementations
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
//!
//! Note: All tools define their input schema using `input_schema` internally,
//! which is serialized to the MCP-compliant `inputSchema` camelCase field via Serde.
use super::types::*;
use crate::coordination::KeyLendingEngine;
use crate::espacio::{ChannelManager, ChannelMessage};
use crate::memory::schema::{
    EvidenceKind, MemoryKind, MemoryNamespace, MemoryProvenance, MemoryQueryFilters,
    TypedMemoryPayload,
};
use crate::secrets::audit::QmdAuditLogger;
use crate::utils::crypto::hex_encode;
use crate::workspace::WorkspaceContext;
use crate::AppState;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Helper function to resolve secrets via KeyLendingEngine lease system for MCP tool operations.
/// Lends a short-lived secret lease, passes the secret value (if available) to the callback,
/// and automatically revokes the lease after the operation completes.
pub async fn resolve_tool_secret<F, Fut, T>(
    secret_name: &str,
    agent_id: &str,
    f: F,
) -> anyhow::Result<T>
where
    F: FnOnce(Option<String>) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<T>>,
{
    let engine = KeyLendingEngine::new(Box::new(QmdAuditLogger::new()), None);
    let raw_val = std::env::var(secret_name).ok();
    let lease = engine
        .lend(secret_name, raw_val.as_deref(), agent_id, 60)
        .await?;

    let val = lease.secret_value.clone();
    let res = f(val).await;

    let _ = engine
        .revoke(&lease.token, "mcp_tool_execution_complete")
        .await;

    res
}

/// Get xavier core tools.
pub fn get_xavier_core_tools() -> Vec<MCPTool> {
    vec![
        MCPTool {
            name: "list_projects".to_string(),
            description: "List all projects in Xavier".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        MCPTool {
            name: "log_scan".to_string(),
            description: "Scan logs under ~/.xavier/logs or fallback. Supports incremental cursor, regex secret redaction, pattern filtering, and Telegram Polling Dead detection.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "since": { "type": "string", "description": "Filter logs since RFC3339 timestamp" },
                    "level_min": { "type": "string", "description": "Minimum log level to show" },
                    "pattern": { "type": "string", "description": "Regex pattern to match" },
                    "source": { "type": "string", "description": "xavier | hermes | journalctl" },
                    "max_entries": { "type": "number", "default": 500 }
                }
            }),
        },
        MCPTool {
            name: "ticket_create".to_string(),
            description: "Create GitHub issue or Maloca backlog entry safely with fingerprint-based deduplication and rate-limiting.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Title of the issue / ticket (max 120 chars)" },
                    "body": { "type": "string", "description": "Detailed body/evidence of the issue / ticket (max 8KB)" },
                    "labels": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional list of labels"
                    },
                    "severity": { "type": "string", "description": "critical | warn" },
                    "fingerprint": { "type": "string", "description": "Optional unique fingerprint override to prevent duplicates" },
                    "backend": { "type": "string", "description": "github | maloca" }
                },
                "required": ["title", "body", "severity"]
            }),
        },
        MCPTool {
            name: "env_status".to_string(),
            description: "Check systemd services, TCP network connectivity, PSI metrics, and swap memory snapshot on the host node.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "include_processes": { "type": "boolean", "description": "Whether to include running processes RSS" },
                    "top_n": { "type": "number", "description": "Limit top N processes (max 20)" }
                }
            }),
        },
        MCPTool {
            name: "get_project_context".to_string(),
            description: "Get full context for a project with configurable limits".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Project identifier"
                    },
                    "max_records": {
                        "type": "number",
                        "description": "Maximum records to return (default: 10, max: 50)",
                        "default": 10
                    },
                    "max_chars": {
                        "type": "number",
                        "description": "Maximum total characters (default: 8000, max: 32000)",
                        "default": 8000
                    },
                    "depth": {
                        "type": "number",
                        "description": "Project depth: 0 = only this project, 1 = include sub-projects (default: 0, max: 2)",
                        "default": 0
                    }
                },
                "required": ["project_id"]
            }),
        },
        MCPTool {
            name: "sync_gitcore".to_string(),
            description: "Sync documentation from GitCore project".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_path": {
                        "type": "string",
                        "description": "Path to GitCore project"
                    }
                },
                "required": ["project_path"]
            }),
        },
        MCPTool {
            name: "health_check".to_string(),
            description: "Report Xavier system health (status, system resources, database, embedding, mesh, checks)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        MCPTool {
            name: "sys_health".to_string(),
            description: "Snapshot del HOST (guardian del nodo): PSI (cpu/memory/io avg10/60/300), swap usado, load average, top 10 procesos por RSS, conteo D-state y alertas con umbrales (psi.io.full.avg10>50% critical, swap>80% critical, VmSwap>4GB warn). Read-only, sin efectos".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        MCPTool {
            name: "get_code_graph".to_string(),
            description: "Get the portable code graph dump (.xavier/codegraph.json)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        MCPTool {
            name: "xavier_local_status".to_string(),
            description: "Report Xavier local-first operation mode and reachability. \
                          Returns: mode (local-healthy|local-degraded|cloud-fallback|disabled), \
                          provider_setting, llm_reachable (bool), embedding_reachable (bool), \
                          ollama_reachable (bool). Use this before delegating reasoning to Xavier."
                .to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
        },
        MCPTool {
            name: "codegraph_explore".to_string(),
            description: "Search the code graph for symbols matching a query".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query (e.g. part of symbol name)"
                    },
                    "limit": {
                        "type": "number",
                        "description": "Maximum symbols to return (default: 20, max: 100)",
                        "default": 20
                    }
                },
                "required": ["query"]
            }),
        },
        MCPTool {
            name: "trace_path".to_string(),
            description: "Trace the dependency path or call chain of a given symbol".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Symbol stable ID or symbol name to trace from"
                    },
                    "max_depth": {
                        "type": "number",
                        "description": "Maximum trace depth (default: 3, max: 8)",
                        "default": 3
                    },
                    "reverse": {
                        "type": "boolean",
                        "description": "If true, trace callers / reverse dependencies; if false, trace callees / forward dependencies",
                        "default": false
                    },
                    "edge_type": {
                        "type": "string",
                        "description": "Filter by edge type (e.g., 'Calls', 'References', 'Imports', etc)"
                    },
                    "limit": {
                        "type": "number",
                        "description": "Maximum edges to return (default: 100, max: 1000)",
                        "default": 100
                    }
                },
                "required": ["symbol"]
            }),
        },
        MCPTool {
            name: "espacio_channel_list".to_string(),
            description: "List channels/messages for a specified space".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "space_id": {
                        "type": "string",
                        "description": "Space identifier"
                    }
                },
                "required": ["space_id"]
            }),
        },
        MCPTool {
            name: "espacio_channel_create".to_string(),
            description: "Create a channel within a space".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "space_id": {
                        "type": "string",
                        "description": "Space identifier"
                    },
                    "name": {
                        "type": "string",
                        "description": "Channel name"
                    }
                },
                "required": ["space_id", "name"]
            }),
        },
    ]
}

/// Is core tool.
pub fn is_core_tool(name: &str) -> bool {
    matches!(
        name,
        "list_projects"
            | "get_project_context"
            | "sync_gitcore"
            | "health_check"
            | "sys_health"
            | "log_scan"
            | "env_status"
            | "ticket_create"
            | "get_code_graph"
            | "xavier_local_status"
            | "codegraph_explore"
            | "trace_path"
            | "espacio_channel_list"
            | "espacio_channel_create"
    )
}

/// Handle core tool.
pub async fn handle_core_tool(
    _state: AppState,
    workspace: WorkspaceContext,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    match name {
        "list_projects" => {
            let records = workspace.workspace.list_memory_records().await?;
            let mut projects = std::collections::BTreeMap::<String, usize>::new();

            for record in records {
                if let Ok(resolved) = crate::memory::schema::resolve_metadata(
                    &record.path,
                    &record.metadata,
                    &workspace.workspace_id,
                    None,
                ) {
                    if let Some(project) = resolved.namespace.project {
                        *projects.entry(project).or_insert(0) += 1;
                    }
                }
            }

            let text = if projects.is_empty() {
                "No projects found.".to_string()
            } else {
                projects
                    .into_iter()
                    .map(|(project, count)| format!("{project}: {count} memories"))
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            super::server::mcp_text_result(text, false)
        }
        "get_project_context" => {
            let project_id = arguments
                .get("project_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing project_id"))?;
            let max_records = arguments
                .get("max_records")
                .and_then(|v| v.as_u64())
                .unwrap_or(10)
                .clamp(1, 50) as usize;
            let max_chars = arguments
                .get("max_chars")
                .and_then(|v| v.as_u64())
                .unwrap_or(8000)
                .clamp(1, 32000) as usize;
            let _depth = arguments
                .get("depth")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                .clamp(0, 2) as usize;

            let records = workspace
                .workspace
                .list_memory_records_filtered(
                    MemoryQueryFilters {
                        project: Some(project_id.to_string()),
                        ..Default::default()
                    },
                    max_records,
                )
                .await?;

            let mut total_chars = 0usize;
            let mut truncated = false;
            let mut truncated_reason = Option::<String>::None;
            let mut sources = Vec::<MCPSearchResult>::new();
            let mut content_parts = Vec::<String>::new();

            for record in &records {
                let entry = format!(
                    "Id: {}\nPath: {}\nRevision: {}\nContent: {}",
                    record.id, record.path, record.revision, record.content
                );
                let entry_len = entry.len();

                if total_chars + entry_len > max_chars {
                    truncated = true;
                    truncated_reason = Some(format!(
                        "truncated at {} chars (max: {})",
                        total_chars, max_chars
                    ));
                    break;
                }

                total_chars += entry_len;
                content_parts.push(entry);

                sources.push(MCPSearchResult {
                    id: record.id.clone(),
                    path: record.path.clone(),
                    score: 0.0, // context retrieval doesn't have a score
                    snippet: crate::memory::snippet::clip_chars(&record.content, 200).to_string(),
                    provenance: MCPProvenance {
                        source: "memory_store".to_string(),
                        retrieved_at: chrono::Utc::now().to_rfc3339(),
                        retrieval_method: "exact".to_string(),
                        embedding_model: None,
                        version: Some(
                            option_env!("XAVIER_VERSION")
                                .unwrap_or("development")
                                .to_string(),
                        ),
                    },
                    metadata: record.metadata.clone(),
                });
            }

            if content_parts.is_empty() {
                return Ok(serde_json::to_value(MCPToolResult::structured(
                    json!(MCPContextResult {
                        total_chars: 0,
                        total_records: 0,
                        truncated: false,
                        truncated_reason: None,
                        content: format!("No context found for project {project_id}."),
                        sources: vec![],
                        estimated_tokens: 0,
                    }),
                    false,
                ))?);
            }

            let content = content_parts.join("\n\n---\n\n");
            let total_records = content_parts.len();
            let estimated_tokens = crate::context::estimate_tokens(&content);

            Ok(serde_json::to_value(MCPToolResult::structured(
                json!(MCPContextResult {
                    total_chars,
                    total_records,
                    truncated,
                    truncated_reason,
                    content,
                    sources,
                    estimated_tokens,
                }),
                false,
            ))?)
        }
        "sync_gitcore" => {
            let project_path = arguments
                .get("project_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing project_path"))?;
            let root = std::path::PathBuf::from(project_path);
            let mut created = 0usize;
            let mut updated = 0usize;
            let mut unchanged = 0usize;
            let mut skipped = 0usize;

            for relative in ["AGENTS.md", ".gitcore/ARCHITECTURE.md", "README.md"] {
                let candidate = root.join(relative);
                if !tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
                    skipped += 1;
                    continue;
                }

                let content = tokio::fs::read_to_string(&candidate).await?;
                let project = root
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("gitcore");
                let path = format!("gitcore/{project}/{}", relative.replace('\\', "/"));
                let content_hash = hex_encode(&Sha256::digest(content.as_bytes()));
                let metadata = json!({
                    "synced_from": candidate.display().to_string(),
                    "content_hash": content_hash,
                });
                let typed = Some(TypedMemoryPayload {
                    kind: Some(MemoryKind::Document),
                    evidence_kind: Some(EvidenceKind::Observation),
                    namespace: Some(MemoryNamespace {
                        project: Some(project.to_string()),
                        ..MemoryNamespace::default()
                    }),
                    provenance: Some(MemoryProvenance {
                        source_app: Some("gitcore".to_string()),
                        source_type: Some("repository_doc".to_string()),
                        file_path: Some(relative.replace('\\', "/")),
                        ..MemoryProvenance::default()
                    }),
                    ..Default::default()
                });

                if let Some(existing) = workspace.workspace.get_memory_record(&path).await? {
                    let existing_hash = existing
                        .metadata
                        .get("content_hash")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default();
                    if existing_hash == content_hash && existing.content == content {
                        unchanged += 1;
                        continue;
                    }

                    workspace
                        .workspace
                        .update_primary_memory(&existing.id, path, content, metadata, typed)
                        .await?;
                    updated += 1;
                    continue;
                }

                workspace
                    .workspace
                    .ingest_typed(path, content, metadata, typed, None, false)
                    .await?;
                created += 1;
            }

            super::server::mcp_text_result(
                format!(
                    "Synced GitCore documents from {project_path}\ncreated={created}\nupdated={updated}\nunchanged={unchanged}\nskipped={skipped}"
                ),
                false
            )
        }
        "health_check" => {
            let health = crate::health::collect_health_sync();
            let tools_count = get_xavier_core_tools().len()
                + super::tools_memory::get_xavier_memory_tools().len()
                + super::tools_context::get_xavier_context_tools().len();

            let result = MCPHealthResult {
                status: health.status.clone(),
                tools_count,
                handshake_ok: true,
                memory_store_ok: health.database.size_mb >= 0.0, // store exists
                embedding_ok: health.embedding.connected,
                mcp_protocol: "2026-07-28".to_string(),
            };

            Ok(serde_json::to_value(MCPToolResult::structured(
                serde_json::to_value(&result)?,
                health.status != "healthy",
            ))?)
        }
        "sys_health" => {
            // Guardian del nodo (P0, 2026-08-08): snapshot read-only del HOST —
            // PSI, swap, load average, top procesos por RSS, D-state y alertas
            // con umbrales (docs/research/SELF-MANAGEMENT-RUNTIME.md §5).
            let snapshot = crate::self_manage::collect_system_snapshot();
            let in_process_health = crate::health::collect_health_sync();

            let db_integrity = in_process_health.checks.iter().any(|c| {
                c.name == "sqlite_integrity" && matches!(c.status, crate::health::CheckStatus::Pass)
            });

            let benchmark = crate::auto_improvement::benchmark::BenchmarkSnapshot {
                timestamp_secs: chrono::Utc::now().timestamp() as u64,
                recall_at_k: 0.0,
                precision: 0.0,
                avg_latency_ms: 0.0,
                p99_latency_ms: 0.0,
                memory_hit_rate: in_process_health.database.size_mb / 1024.0,
                cache_hit_rate: 0.0,
                mesh_peers_reachable: in_process_health.mesh.connected_peers,
                health_status: in_process_health.status.clone(),
                db_integrity_ok: db_integrity,
                total_documents: 0,
                test_iterations: 0,
            };
            let active_gaps = crate::auto_improvement::gaps::analyze_gaps(&benchmark, None);

            let history_path = std::path::Path::new(".xavier/improvement-history.json");
            let last_experiment = crate::auto_improvement::cycle::load_history(history_path)
                .ok()
                .and_then(|entries| entries.into_iter().next())
                .and_then(|entry| entry.experiments.into_iter().next());

            let overall_alert = snapshot.overall.clone();

            let output = serde_json::json!({
                "overall": overall_alert,
                "components": in_process_health,
                "active_gaps": active_gaps,
                "last_experiment": last_experiment,
                "system_snapshot": snapshot,
            });

            Ok(serde_json::to_value(MCPToolResult::structured(
                output,
                overall_alert != "healthy",
            ))?)
        }
        "log_scan" => {
            let since = arguments
                .get("since")
                .and_then(|v| v.as_str().map(|s| s.to_string()));
            let level_min = arguments
                .get("level_min")
                .and_then(|v| v.as_str().map(|s| s.to_string()));
            let pattern = arguments
                .get("pattern")
                .and_then(|v| v.as_str().map(|s| s.to_string()));
            let source = arguments
                .get("source")
                .and_then(|v| v.as_str().map(|s| s.to_string()));
            let max_entries = arguments
                .get("max_entries")
                .and_then(|v| v.as_u64())
                .unwrap_or(500) as usize;

            let args = crate::self_manage::LogScanArgs {
                since,
                level_min,
                pattern,
                source,
                max_entries,
            };

            let result = crate::self_manage::log_scan(args);
            Ok(serde_json::to_value(MCPToolResult::structured(
                serde_json::to_value(&result)?,
                result.telegram_polling_dead,
            ))?)
        }
        "env_status" => {
            let include_processes = arguments.get("include_processes").and_then(|v| v.as_bool());
            let top_n = arguments
                .get("top_n")
                .and_then(|v| v.as_u64().map(|n| n as usize));

            let args = crate::self_manage::EnvStatusArgs {
                include_processes,
                top_n,
            };

            let result = crate::self_manage::env_status(args);
            Ok(serde_json::to_value(MCPToolResult::structured(
                serde_json::to_value(&result)?,
                result.overall != "healthy",
            ))?)
        }
        "ticket_create" => {
            let title = arguments
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let body = arguments
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let labels = arguments.get("labels").and_then(|v| {
                v.as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|item| item.as_str().map(|s| s.to_string()))
                        .collect::<Vec<String>>()
                })
            });
            let severity = arguments
                .get("severity")
                .and_then(|v| v.as_str())
                .unwrap_or("warn")
                .to_string();
            let fingerprint = arguments
                .get("fingerprint")
                .and_then(|v| v.as_str().map(|s| s.to_string()));
            let backend = arguments
                .get("backend")
                .and_then(|v| v.as_str().map(|s| s.to_string()));

            let args = crate::self_manage::TicketCreateArgs {
                title,
                body,
                labels,
                severity,
                fingerprint,
                backend,
            };

            resolve_tool_secret(
                "GITHUB_TOKEN",
                "mcp_ticket_create",
                |_secret_val| async move {
                    match crate::self_manage::ticket_create(args) {
                        Ok(result) => Ok(serde_json::to_value(MCPToolResult::structured(
                            serde_json::to_value(&result)?,
                            result.deduplicated,
                        ))?),
                        Err(error) => Err(error),
                    }
                },
            )
            .await
        }
        "xavier_local_status" => {
            let mode = crate::server::alerts::SYSTEM_ALERTS.get_mode();
            let provider = std::env::var("XAVIER_PROVIDER")
                .or_else(|_| std::env::var("XAVIER_MODEL_PROVIDER"))
                .unwrap_or_else(|_| "local".into());
            let health = crate::observability::health::HEALTH.get_status().await;
            let llm_reachable = health.llm.reachable;
            let embedding_reachable = !matches!(
                health.embedding.status,
                crate::observability::health::HealthLevel::Unhealthy
            );
            let ollama_ok = llm_reachable && (provider == "local" || provider == "ollama");
            let mode_str = serde_json::to_value(&mode)
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| format!("{:?}", mode).to_lowercase());

            let val = json!({
                "mode": mode_str,
                "provider_setting": provider,
                "llm_reachable": llm_reachable,
                "embedding_reachable": embedding_reachable,
                "ollama_reachable": ollama_ok,
                "fallback_chain": [],
            });

            Ok(serde_json::to_value(MCPToolResult::structured(val, false))?)
        }
        "get_code_graph" => {
            let dump_path = _state.code_graph_dump_path.clone().unwrap_or_else(|| {
                let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                crate::codebase::codegraph_paths::codegraph_dump_path_for(&cwd)
            });

            if tokio::fs::try_exists(&dump_path).await.unwrap_or(false) {
                let json_content = tokio::fs::read_to_string(&dump_path).await?;
                let dump: Value = serde_json::from_str(&json_content)?;
                return Ok(serde_json::to_value(MCPToolResult {
                    content: vec![MCPContent::Text(MCPTextContent {
                        content_type: "text".to_string(),
                        text: serde_json::to_string(&dump)?,
                    })],
                    is_error: Some(false),
                })?);
            }

            // Live fallback when dump is missing/stale — avoid false "not found".
            let stats = _state
                .code_db
                .stats()
                .unwrap_or(code_graph::types::IndexStats {
                    total_files: 0,
                    total_symbols: 0,
                    total_imports: 0,
                    languages: vec![],
                    duration_ms: 0,
                });
            let hubs = _state.code_query.hubs(0, 20).unwrap_or_default();
            let summary = json!({
                "source": "live_db",
                "dump_path": dump_path.display().to_string(),
                "dump_present": false,
                "hint": "Run `xavier code dump .` or `xavier code scan .` to refresh the portable dump",
                "stats": {
                    "total_files": stats.total_files,
                    "total_symbols": stats.total_symbols,
                    "total_imports": stats.total_imports,
                },
                "hubs": hubs.iter().take(10).map(|h| json!({
                    "name": h.symbol.name,
                    "file": h.symbol.file_path,
                    "incoming": h.incoming,
                    "outgoing": h.outgoing,
                })).collect::<Vec<_>>(),
            });
            Ok(serde_json::to_value(MCPToolResult::structured(
                summary, false,
            ))?)
        }
        "codegraph_explore" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing query"))?;
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20)
                .clamp(1, 100) as usize;

            let result = _state.code_query.search(query, limit)?;
            let returned = result.symbols.len();
            let val = json!({
                "returned": returned,
                "symbols": result.symbols,
            });

            Ok(serde_json::to_value(MCPToolResult::structured(val, false))?)
        }
        "trace_path" => {
            let symbol = arguments
                .get("symbol")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing symbol"))?;
            let max_depth = arguments
                .get("max_depth")
                .and_then(|v| v.as_u64())
                .unwrap_or(3)
                .clamp(1, 8) as usize;
            let reverse = arguments
                .get("reverse")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(100)
                .clamp(1, 1000) as usize;

            let edge_type_str = arguments.get("edge_type").and_then(|v| v.as_str());

            let edge_type_filter = match edge_type_str {
                Some(s) => match s.to_lowercase().as_str() {
                    "calls" => Some(code_graph::types::EdgeType::Calls),
                    "defines" => Some(code_graph::types::EdgeType::Defines),
                    "uses" => Some(code_graph::types::EdgeType::Uses),
                    "imports" => Some(code_graph::types::EdgeType::Imports),
                    "exports" => Some(code_graph::types::EdgeType::Exports),
                    "contains" => Some(code_graph::types::EdgeType::Contains),
                    "references" => Some(code_graph::types::EdgeType::References),
                    "extends" => Some(code_graph::types::EdgeType::Extends),
                    "implements" => Some(code_graph::types::EdgeType::Implements),
                    "typeof" => Some(code_graph::types::EdgeType::TypeOf),
                    "returns" => Some(code_graph::types::EdgeType::Returns),
                    "instantiates" => Some(code_graph::types::EdgeType::Instantiates),
                    "overrides" => Some(code_graph::types::EdgeType::Overrides),
                    "decorates" => Some(code_graph::types::EdgeType::Decorates),
                    _ => None,
                },
                None => None,
            };

            let edges = if reverse {
                _state.code_query.reverse_dependencies(
                    symbol,
                    edge_type_filter,
                    max_depth,
                    limit,
                )?
            } else {
                _state
                    .code_query
                    .dependencies(symbol, edge_type_filter, max_depth, limit)?
            };

            let direction = if reverse { "callers" } else { "dependencies" };
            let val = json!({
                "symbol": symbol,
                "direction": direction,
                "edges": edges,
            });

            Ok(serde_json::to_value(MCPToolResult::structured(val, false))?)
        }
        "espacio_channel_list" => {
            let space_id = arguments
                .get("space_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing space_id"))?;

            let channel_mgr = ChannelManager::new();
            let messages: Vec<ChannelMessage> = channel_mgr.list_all(space_id).await;

            let val = json!({
                "space_id": space_id,
                "messages": messages,
                "count": messages.len(),
            });

            Ok(serde_json::to_value(MCPToolResult::structured(val, false))?)
        }
        "espacio_channel_create" => {
            let space_id = arguments
                .get("space_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing space_id"))?;
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing name"))?;

            let channel_mgr = ChannelManager::new();
            let msg: ChannelMessage = channel_mgr
                .post(
                    space_id.to_string(),
                    "mcp_operator".to_string(),
                    name.to_string(),
                )
                .await;

            let val = json!({
                "space_id": space_id,
                "channel_name": name,
                "status": "created",
                "message": msg,
            });

            Ok(serde_json::to_value(MCPToolResult::structured(val, false))?)
        }
        _ => Err(anyhow::anyhow!("Unknown core tool: {}", name)),
    }
}
