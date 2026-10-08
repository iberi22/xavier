//! Core MCP tool implementations
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
//!
//! Note: All tools define their input schema using `input_schema` internally,
//! which is serialized to the MCP-compliant `inputSchema` camelCase field via Serde.
use super::tools_secrets::{resolve_tool_secret, SecretSource, PRODUCTION_VAULT_SERVICE};
use super::types::*;
use crate::espacio::{ChannelManager, ChannelMessage};
use crate::memory::schema::{
    EvidenceKind, MemoryKind, MemoryNamespace, MemoryProvenance, MemoryQueryFilters,
    TypedMemoryPayload,
};
use crate::secrets::vault::HardwareVault;
use crate::self_manage::TicketCreateArgs;
use crate::utils::crypto::hex_encode;
use crate::workspace::WorkspaceContext;
use crate::AppState;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Runs `ticket_create` and wraps the outcome as an MCP tool result.
async fn ticket_create_result(args: TicketCreateArgs) -> anyhow::Result<Value> {
    match crate::self_manage::ticket_create(args) {
        Ok(result) => Ok(serde_json::to_value(MCPToolResult::structured(
            serde_json::to_value(&result)?,
            false,
        ))?),
        Err(error) => Err(error),
    }
}

/// `ticket_create` only needs `GITHUB_TOKEN` when the GitHub backend is
/// selected: the maloca backend must work against an empty vault, so the
/// vault-backed lease is requested conditionally rather than up front.
async fn ticket_create_tool(
    source: &dyn SecretSource,
    args: TicketCreateArgs,
) -> anyhow::Result<Value> {
    if args.backend.as_deref() == Some("github") {
        resolve_tool_secret(
            source,
            "GITHUB_TOKEN",
            "mcp_ticket_create",
            |_secret_val| async move { ticket_create_result(args).await },
        )
        .await
    } else {
        ticket_create_result(args).await
    }
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
                    "page": {
                        "type": "number",
                        "description": "Page number (1-based, default: 1)",
                        "default": 1
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
            name: "codegraph_route".to_string(),
            description: "Route a search query through the code graph, combining exact matches and centrality-ranked symbol candidates with confidence margin reporting".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query or symbol name to route"
                    },
                    "limit": {
                        "type": "number",
                        "description": "Maximum symbols to return (default: 10, max: 100)",
                        "default": 10
                    }
                },
                "required": ["query"]
            }),
        },
        MCPTool {
            name: "codegraph_gods".to_string(),
            description: "Get god/hub nodes (top structurally central and connected symbols excluding builtins) in the code graph".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "number",
                        "description": "Maximum hub nodes to return (default: 10, max: 100)",
                        "default": 10
                    }
                }
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
    MCPTool {
            name: "recovery_status".to_string(),
            description: "Report whether the memory store's encryption keys can actually be recovered. \
                          Returns: recoverable (bool), gaps (what is missing), rclone remote in use, \
                          and whether the Drive OAuth credential is sealed on disk. \
                          A vault is only recoverable when a key seal exists AND the rclone crypt \
                          passphrase is backed up somewhere: recovering the key alone does not make \
                          the encrypted snapshots readable."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "crypt_passphrase_backed_up": {
                        "type": "boolean",
                        "description": "Operator assertion: is the rclone crypt passphrase stored outside this host? Omit to report it as unverified."
                    },
                    "recovery_dir": {
                        "type": "string",
                        "description": "Optional override of the recovery directory (defaults to $XAVIER_RECOVERY_DIR or ~/.xavier/recovery)"
                    }
                }
            }),
        },
        MCPTool {
            name: "drive_connect_status".to_string(),
            description: "Report the Google Drive connection used for encrypted backups: whether an \
                          existing rclone remote will be reused (no OAuth needed), which scopes the \
                          sealed credential holds, and whether the access token is expired. \
                          Read-only: never performs OAuth and never returns a token."
                .to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
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
            | "codegraph_route"
            | "codegraph_gods"
            | "espacio_channel_list"
            | "espacio_channel_create"
            | "recovery_status"
            | "drive_connect_status"
    )
}

/// Result of the `sqlite_integrity` check, or `None` when it was not run.
fn sqlite_integrity(health: &crate::health::HealthResponse) -> Option<bool> {
    health
        .checks
        .iter()
        .find(|c| c.name == "sqlite_integrity")
        .map(|c| matches!(c.status, crate::health::CheckStatus::Pass))
}

pub(crate) fn mcp_health_result(
    health: &crate::health::HealthResponse,
    tools_count: usize,
    store_reachable: bool,
) -> MCPHealthResult {
    MCPHealthResult {
        status: health.status.clone(),
        tools_count,
        handshake_ok: true,
        memory_store_ok: store_reachable && sqlite_integrity(health) != Some(false),
        embedding_ok: health.embedding.connected || health.embedding.fallback_success,
        mcp_protocol: "2026-07-28".to_string(),
    }
}

/// The node's shared `SpaceManager` for the espacio MCP tools. Root only:
/// the call must carry the root-credential marker (set by the transport from
/// the `RootCredential` request extension). A claimed `Admin` role, such as
/// an Admin JWT, is not enough. The node must also run with espacio enabled.
fn espacio_manager_for_root() -> anyhow::Result<std::sync::Arc<crate::espacio::SpaceManager>> {
    if !super::server::root_credential_present() {
        return Err(anyhow::anyhow!(
            "Forbidden: espacio tools require the root credential"
        ));
    }
    crate::adapters::inbound::http::routes::get_space_manager()
        .ok_or_else(|| anyhow::anyhow!("espacio is disabled on this node"))
}

/// Handle core tool.
pub async fn handle_core_tool(
    _state: AppState,
    workspace: WorkspaceContext,
    _claims: Option<&crate::security::auth::Claims>,
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
            let page = arguments
                .get("page")
                .and_then(|v| v.as_u64())
                .unwrap_or(1)
                .max(1) as usize;
            let max_records = arguments
                .get("max_records")
                .or_else(|| arguments.get("limit"))
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

            let fetch_limit = (page * max_records).saturating_add(1);
            let all_records = workspace
                .workspace
                .list_memory_records_filtered(
                    MemoryQueryFilters {
                        project: Some(project_id.to_string()),
                        ..Default::default()
                    },
                    fetch_limit,
                )
                .await?;

            let offset = (page - 1) * max_records;
            let total_fetched = all_records.len();
            let has_more = total_fetched > page * max_records;
            let records: Vec<_> = all_records
                .into_iter()
                .skip(offset)
                .take(max_records)
                .collect();
            let total_pages = if total_fetched == 0 {
                0
            } else if has_more {
                page + 1
            } else {
                total_fetched.div_ceil(max_records)
            };

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
                if records.is_empty() {
                    return Ok(serde_json::to_value(MCPToolResult::structured(
                        json!(MCPContextResult {
                            total_chars: 0,
                            total_records: 0,
                            truncated: false,
                            truncated_reason: None,
                            content: format!("No context found for project {project_id}."),
                            sources: vec![],
                            estimated_tokens: 0,
                            page: Some(page),
                            limit: Some(max_records),
                            total_pages: Some(total_pages),
                            has_more: Some(has_more),
                        }),
                        false,
                    ))?);
                }

                // Records DO exist but even the first entry exceeds max_chars:
                // answering "No context found" here is a lie that misleads
                // agents (regression 2026-09-12: max_chars=1200 against a
                // 1219-char state entry made Hermes read "No context found"
                // and conclude the store was empty). Return the first entry
                // clipped to the budget, flagged as truncated.
                let first = &records[0];
                let full_entry = format!(
                    "Id: {}\nPath: {}\nRevision: {}\nContent: {}",
                    first.id, first.path, first.revision, first.content
                );
                let clipped =
                    crate::memory::snippet::clip_chars(&full_entry, max_chars).to_string();
                sources.push(MCPSearchResult {
                    id: first.id.clone(),
                    path: first.path.clone(),
                    score: 0.0,
                    snippet: crate::memory::snippet::clip_chars(&first.content, 200).to_string(),
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
                    metadata: first.metadata.clone(),
                });
                return Ok(serde_json::to_value(MCPToolResult::structured(
                    json!(MCPContextResult {
                        total_chars: clipped.len(),
                        total_records: 1,
                        truncated: true,
                        truncated_reason: Some(format!(
                            "first entry exceeded max_chars ({} > {}), clipped to budget",
                            full_entry.len(),
                            max_chars
                        )),
                        content: clipped.clone(),
                        sources,
                        estimated_tokens: crate::context::estimate_tokens(&clipped),
                        page: Some(page),
                        limit: Some(max_records),
                        total_pages: Some(total_pages),
                        has_more: Some(has_more),
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
                    page: Some(page),
                    limit: Some(max_records),
                    total_pages: Some(total_pages),
                    has_more: Some(has_more),
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

            // The store is OK when it actually answers a query; a measured SQLite
            // integrity failure overrides that.
            let store_reachable = workspace.workspace.memory.count().await.is_ok();
            let result = mcp_health_result(&health, tools_count, store_reachable);

            // MCP `isError` = tool EXECUTION failure (spec 2025-06-18+), not host
            // state: warn/degraded alerts ride inside the payload. Only a hard
            // unhealthy/critical state marks the result as an error.
            Ok(serde_json::to_value(MCPToolResult::structured(
                serde_json::to_value(&result)?,
                matches!(health.status.as_str(), "unhealthy" | "critical"),
            ))?)
        }
        "sys_health" => {
            // Guardian del nodo (P0, 2026-08-08): snapshot read-only del HOST —
            // PSI, swap, load average, top procesos por RSS, D-state y alertas
            // con umbrales (docs/research/SELF-MANAGEMENT-RUNTIME.md §5).
            let snapshot = crate::self_manage::collect_system_snapshot();
            let in_process_health = crate::health::collect_health_sync();

            // `None` = not measured on this path (no DB connection is threaded into
            // `collect_health_sync`), reported as null instead of a fabricated `false`.
            let db_integrity = sqlite_integrity(&in_process_health);

            let total_documents = workspace.workspace.memory.count().await.unwrap_or(0);

            // Measured vs unmeasured fields (WAVE-29.02 / issue #2555):
            // - Measured: timestamp_secs, memory_hit_rate (size proxy), mesh_peers_reachable,
            //   health_status, db_integrity_ok, total_documents.
            // - Unmeasured sentinels (-1.0): recall_at_k, precision, avg_latency_ms,
            //   p99_latency_ms, cache_hit_rate. Using -1.0 sentinel prevents analyze_gaps
            //   from treating 0.0 as measured degraded performance.
            // - Unmeasured counter (0): test_iterations (no synthetic benchmarks executed).
            let benchmark = crate::auto_improvement::benchmark::BenchmarkSnapshot {
                timestamp_secs: chrono::Utc::now().timestamp() as u64,
                recall_at_k: -1.0,    // unmeasured sentinel
                precision: -1.0,      // unmeasured sentinel
                avg_latency_ms: -1.0, // unmeasured sentinel
                p99_latency_ms: -1.0, // unmeasured sentinel
                memory_hit_rate: in_process_health.database.size_mb / 1024.0,
                cache_hit_rate: -1.0, // unmeasured sentinel
                mesh_peers_reachable: in_process_health.mesh.connected_peers,
                health_status: in_process_health.status.clone(),
                // Unmeasured integrity must not raise a Critical `db_integrity` gap.
                db_integrity_ok: db_integrity.unwrap_or(true),
                total_documents,
                test_iterations: 0,
            };
            let active_gaps = crate::auto_improvement::gaps::analyze_gaps(&benchmark, None);

            let history_path = std::path::Path::new(".xavier/improvement-history.json");
            let last_experiment = crate::auto_improvement::cycle::load_history(history_path)
                .ok()
                // Skip truncated entries: they carry no accepted experiments.
                .and_then(|entries| entries.into_iter().find(|e| e.partial.is_none()))
                .and_then(|entry| entry.experiments.into_iter().next());

            let overall_alert = snapshot.overall.clone();

            let output = serde_json::json!({
                "overall": overall_alert,
                "components": in_process_health,
                "active_gaps": active_gaps,
                "db_integrity": db_integrity,
                "last_experiment": last_experiment,
                "system_snapshot": snapshot,
            });

            Ok(serde_json::to_value(MCPToolResult::structured(
                output,
                overall_alert == "critical",
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
                result.overall == "critical",
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

            let args = TicketCreateArgs {
                title,
                body,
                labels,
                severity,
                fingerprint,
                backend,
            };

            let vault = HardwareVault::new(PRODUCTION_VAULT_SERVICE);
            ticket_create_tool(&vault, args).await
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
                    structured_content: None,
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
        "codegraph_route" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing query"))?;
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10)
                .clamp(1, 100) as usize;

            let report = _state.code_query.route_query(query, limit)?;

            let hits_json: Vec<Value> = report
                .hits
                .iter()
                .map(|h| {
                    json!({
                        "symbol": h.symbol,
                        "score": h.score
                    })
                })
                .collect();

            let mode_str = match report.mode {
                code_graph::query::RouteMode::ExactName => "ExactName",
                code_graph::query::RouteMode::GraphRanked => "GraphRanked",
            };

            let val = json!({
                "mode": mode_str,
                "margin": report.margin_pct,
                "confident": report.confident,
                "refusal": !report.confident,
                "est_tokens": report.meta.est_tokens,
                "total": report.meta.total,
                "shown": report.meta.shown,
                "hits": hits_json,
            });

            Ok(serde_json::to_value(MCPToolResult::structured(val, false))?)
        }
        "codegraph_gods" => {
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10)
                .clamp(1, 100) as usize;

            let raw_hubs = _state
                .code_query
                .god_nodes(limit.saturating_mul(10).clamp(50, 200))?;
            let mut unique_names = std::collections::HashSet::new();
            let mut hubs = Vec::new();
            for hub in raw_hubs {
                let name = hub.symbol.name.to_lowercase();
                if ["new", "default", "from", "with_capacity", "clone"].contains(&name.as_str()) {
                    continue;
                }
                // Dedup per symbol, not per name: same-named hubs in different
                // modules are distinct.
                let key = hub.symbol.stable_id.clone().unwrap_or_else(|| {
                    format!(
                        "{}:{}:{}",
                        hub.symbol.file_path, hub.symbol.start_line, name
                    )
                });
                if unique_names.insert(key) {
                    hubs.push(hub);
                    if hubs.len() >= limit {
                        break;
                    }
                }
            }

            if hubs.is_empty() {
                let val = json!({
                    "returned": 0,
                    "god_nodes": [],
                    "reason": "No structurally significant god nodes found (generic constructors excluded)."
                });
                return Ok(serde_json::to_value(MCPToolResult::structured(val, false))?);
            }

            let returned = hubs.len();
            let val = json!({
                "returned": returned,
                "god_nodes": hubs,
            });

            Ok(serde_json::to_value(MCPToolResult::structured(val, false))?)
        }
        "espacio_channel_list" => {
            let manager = espacio_manager_for_root()?;
            let space_id = arguments
                .get("space_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing space_id"))?;
            manager.get(space_id).await?;

            let channel_mgr = ChannelManager::with_stores(manager.stores());
            let messages: Vec<ChannelMessage> = channel_mgr.list_all(space_id).await;

            let val = json!({
                "space_id": space_id,
                "messages": messages,
                "count": messages.len(),
            });

            Ok(serde_json::to_value(MCPToolResult::structured(val, false))?)
        }
        "espacio_channel_create" => {
            let manager = espacio_manager_for_root()?;
            let space_id = arguments
                .get("space_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing space_id"))?;
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing name"))?;
            manager.get(space_id).await?;

            let channel_mgr = ChannelManager::with_stores(manager.stores());
            let msg: ChannelMessage = channel_mgr
                .try_post(
                    space_id.to_string(),
                    "mcp_operator".to_string(),
                    name.to_string(),
                )
                .await?;

            let val = json!({
                "space_id": space_id,
                "channel_name": name,
                "status": "created",
                "message": msg,
            });

            Ok(serde_json::to_value(MCPToolResult::structured(val, false))?)
        }
        "drive_connect_status" => {
            let payload = drive_connect_status_report();
            Ok(serde_json::to_value(MCPToolResult::structured(
                payload, false,
            ))?)
        }
        "recovery_status" => {
            let crypt = arguments
                .get("crypt_passphrase_backed_up")
                .and_then(|v| v.as_bool());
            let store = match arguments.get("recovery_dir").and_then(|v| v.as_str()) {
                Some(dir) if !dir.trim().is_empty() => crate::recovery::RecoveryStore::at(dir),
                _ => crate::recovery::RecoveryStore::from_env_or_default(),
            };

            use crate::memory::sqlite_vec_store::at_rest::{
                peek_record_key_with_source, KeySource,
            };
            use crate::recovery::{KcvState, SealHealth};
            let (live_key, source) = peek_record_key_with_source();
            let status = store.status(crypt, live_key.as_ref());
            let live_source = match source {
                KeySource::Env => "env",
                KeySource::File(_) => "file",
                KeySource::Missing(_) => "missing",
                KeySource::Unavailable(_) | KeySource::Generated(_) => "unavailable",
            };
            let seal_report = |seal: &SealHealth| match seal {
                SealHealth::Absent => json!({"state": "absent", "reason": null, "mode0600": false}),
                SealHealth::Malformed(reason) => {
                    json!({"state": "malformed", "reason": reason, "mode0600": false})
                }
                SealHealth::Valid { mode_0600 } => {
                    json!({"state": "valid", "reason": null, "mode0600": mode_0600})
                }
            };
            let openable = (status.mnemonic_seal_present || status.passphrase_seal_present)
                && matches!(
                    status.kcv,
                    KcvState::MatchesLiveKey | KcvState::LiveKeyMissing
                );
            let manifest_present = store.read_manifest().ok().flatten().is_some();
            let kcv_present = store.read_kcv().ok().flatten().is_some();

            let payload = json!({
                "recoverable": status.is_recoverable(),
                "mnemonicSeal": seal_report(&status.mnemonic_seal),
                "passphraseSeal": seal_report(&status.passphrase_seal),
                "kcvState": status.kcv,
                "liveKeySource": live_source,
                "openable": openable,
                "gaps": status.gaps(),
                "manifestPresent": manifest_present,
                "kcvPresent": kcv_present,
                "cryptPassphraseBackedUp": crypt,
                "rcloneRemote": crate::drive::detect_rclone_remote().map(|r| json!({
                    "name": r.name,
                    "encrypted": r.suitable_for_encrypted_backup(),
                })),
                "note": "The node record key protects XRK1 rows; XDK2 rows require the master key and default-space keystore. Recovery is reported as false while the rclone crypt passphrase is unverified, because the key alone cannot read the encrypted snapshots."
            });
            Ok(serde_json::to_value(MCPToolResult::structured(
                payload, false,
            ))?)
        }
        _ => anyhow::bail!("unknown core tool: {name}"),
    }
}

/// Read-only report of the Drive connection. Never returns a token or a key.
fn drive_connect_status_report() -> serde_json::Value {
    let remote = crate::drive::detect_rclone_remote();
    let store = crate::drive::credentials::DriveCredentialStore::from_env_or_default();

    json!({
        "rcloneRemote": remote.as_ref().map(|r| json!({
            "name": r.name,
            "remote": r.remote_string(),
            "encrypted": r.suitable_for_encrypted_backup(),
        })),
        // The whole point of the native path: OAuth is only entered when no
        // usable remote exists, so the two working timers keep using theirs.
        "oauthRequired": remote.is_none(),
        "credentialSealedOnDisk": store.exists(),
        "requestedScopes": crate::drive::DriveScopes::REQUESTED,
        "note": "Credentials are sealed under a key derived from master.key; existence is reported without unsealing them. No token is ever returned by this tool."
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty vault under a tempdir: no keyring, no real `~/.xavier`.
    fn isolated_empty_vault(temp_dir: &tempfile::TempDir) -> HardwareVault {
        HardwareVault::new("x").isolated(temp_dir.path().join("secrets"), [11u8; 32])
    }

    fn unique_ticket_args(backend: &str) -> TicketCreateArgs {
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        TicketCreateArgs {
            title: format!("V6c test ticket {suffix}"),
            body: "generated by tools_core unit test".to_string(),
            labels: None,
            severity: "warn".to_string(),
            fingerprint: Some(format!("v6c-test-{suffix}")),
            backend: Some(backend.to_string()),
        }
    }

    #[tokio::test]
    async fn test_ticket_create_maloca_backend_succeeds_with_empty_vault() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let vault = isolated_empty_vault(&temp_dir);

        let result = ticket_create_tool(&vault, unique_ticket_args("maloca")).await;

        assert!(
            result.is_ok(),
            "maloca backend must not require a vault secret, got: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_ticket_create_github_backend_fails_without_github_token() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let vault = isolated_empty_vault(&temp_dir);

        let result = ticket_create_tool(&vault, unique_ticket_args("github")).await;

        let err = result.expect_err("github backend must fail without GITHUB_TOKEN in the vault");
        assert!(
            err.to_string().contains("GITHUB_TOKEN"),
            "error must clearly name the missing secret, got: {err}"
        );
    }

    fn root_claims() -> crate::security::auth::Claims {
        crate::security::auth::Claims::new(
            "root".to_string(),
            "admin@swal.dev".to_string(),
            crate::security::auth::UserRole::Admin,
            chrono::Duration::hours(1),
        )
    }

    /// The espacio tools use the daemon's shared, persisted manager, need the
    /// root identity, and refuse unknown spaces.
    #[tokio::test]
    async fn espacio_tools_use_shared_persisted_manager_and_require_root() {
        use crate::espacio::{SpaceManager, StaticNodeKek};
        let tmp = tempfile::tempdir().expect("tempdir");
        let kek = || std::sync::Arc::new(StaticNodeKek::new([7u8; 32]));
        let manager = std::sync::Arc::new(SpaceManager::open_with_keys(tmp.path(), kek()));
        crate::adapters::inbound::http::routes::init_space_manager(manager.clone());
        manager
            .create(
                "esp_mcp".into(),
                "mcp".into(),
                "".into(),
                "owner".into(),
                false,
            )
            .await
            .expect("create space");

        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let root = root_claims();
        async fn call(
            st: &(AppState, WorkspaceContext),
            claims: Option<&crate::security::auth::Claims>,
            name: &str,
            args: Value,
        ) -> anyhow::Result<Value> {
            // Only the root fixture carries the root-credential marker, the way
            // the transports derive it from the `RootCredential` extension.
            let is_root = claims.is_some_and(|c| c.sub == "root");
            crate::server::mcp::server::with_root_credential(
                is_root,
                handle_core_tool(st.0.clone(), st.1.clone(), claims, name, args),
            )
            .await
        }
        let st = (state, workspace);

        // Not root: refused.
        let ro = crate::security::auth::Claims::new(
            "u".into(),
            "u@swal.dev".into(),
            crate::security::auth::UserRole::Readonly,
            chrono::Duration::hours(1),
        );
        // An Admin JWT is not root: role claims alone never open the tools.
        let admin_jwt = crate::security::auth::Claims::new(
            "jwt-admin".into(),
            "a@swal.dev".into(),
            crate::security::auth::UserRole::Admin,
            chrono::Duration::hours(1),
        );
        for c in [None, Some(&ro), Some(&admin_jwt)] {
            let err = call(
                &st,
                c,
                "espacio_channel_list",
                json!({"space_id": "esp_mcp"}),
            )
            .await
            .expect_err("non-root must be refused");
            assert!(err.to_string().contains("Forbidden"), "{err}");
        }
        // Unknown space: error, nothing created behind the manager's back.
        assert!(call(
            &st,
            Some(&root),
            "espacio_channel_create",
            json!({"space_id": "esp_ghost", "name": "x"})
        )
        .await
        .is_err());
        assert!(!tmp.path().join("spaces").join("esp_ghost").exists());

        // Persisted across calls.
        for n in ["one", "two"] {
            call(
                &st,
                Some(&root),
                "espacio_channel_create",
                json!({"space_id": "esp_mcp", "name": n}),
            )
            .await
            .expect("create");
        }
        let res = call(
            &st,
            Some(&root),
            "espacio_channel_list",
            json!({"space_id": "esp_mcp"}),
        )
        .await
        .expect("list");
        assert_eq!(res["structuredContent"]["count"], 2, "{res}");

        // And across a manager restart (reopen from the same dir).
        drop(manager);
        let reopened = SpaceManager::open_with_keys(tmp.path(), kek());
        let msgs = ChannelManager::with_stores(reopened.stores())
            .list_all("esp_mcp")
            .await;
        assert_eq!(msgs.len(), 2);
    }
}
