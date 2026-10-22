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

/// Reads an optional string argument, rejecting a present value of another type.
fn optional_str_arg<'a>(arguments: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(format!("Invalid argument '{key}': expected string")),
    }
}

/// Reads a required string argument, telling a missing value from a wrong type.
fn required_str_arg<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, String> {
    optional_str_arg(arguments, key)?.ok_or_else(|| format!("Missing required parameter: {key}"))
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
                        "description": "Reserved: currently has no effect (response echoes depth_applied: 0)",
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
            description: "Post a message to a space's channel. Each call appends a new message (identical calls are not deduplicated); content over 4096 characters is rejected.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "space_id": {
                        "type": "string",
                        "description": "Space identifier"
                    },
                    "name": {
                        "type": "string",
                        "description": "Message text to post (max 4096 characters)"
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
            | "espacio_member_list"
            | "espacio_member_add"
            | "espacio_member_remove"
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

/// Tool count from the same registry `tools/list` serves.
fn served_tools_count() -> usize {
    super::server::get_xavier_tools().len()
}

/// Run the blocking health collection off the async workers.
async fn collect_health_blocking() -> crate::health::HealthResponse {
    tokio::task::spawn_blocking(crate::health::collect_health_sync)
        .await
        .unwrap_or_else(|_| crate::health::fast_degraded_health_fallback())
}

/// One embedding probe shared by `health_check` and `xavier_local_status`.
async fn shared_embedding_probe() -> bool {
    let health = crate::observability::health::HEALTH.get_status().await;
    embedding_probe_ok(&health.embedding)
}

/// An embedder that was never probed (default `provider: "unknown"`) is not OK.
fn embedding_probe_ok(embedding: &crate::observability::health::EmbeddingHealth) -> bool {
    embedding.provider != "unknown"
        && embedding.available
        && !matches!(
            embedding.status,
            crate::observability::health::HealthLevel::Unhealthy
        )
}

/// Serialize components; an unmeasured database (empty path) is `unknown`, not zeros.
fn components_json(health: &crate::health::HealthResponse) -> Value {
    let mut v = serde_json::to_value(health).unwrap_or(Value::Null);
    if health.database.path.is_empty() {
        if let Some(obj) = v.as_object_mut() {
            obj.insert("database".into(), json!({ "status": "unknown" }));
        }
    }
    v
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

/// Validate that the caller has sufficient clearance to write the memory document.
fn validate_memory_write_clearance(
    workspace: &WorkspaceContext,
    path: &str,
    metadata: &Value,
    typed: Option<&TypedMemoryPayload>,
) -> anyhow::Result<()> {
    let effective =
        crate::memory::schema::resolve_metadata(path, metadata, &workspace.workspace_id, typed)?
            .clearance;
    crate::security::clearance::check_write_clearance(
        effective,
        crate::server::mcp::tools_memory::caller_clearance(),
    )
    .map_err(anyhow::Error::msg)
}

/// Roots permitted for codebase and file sync access.
fn allowed_project_roots(workspace: &WorkspaceContext) -> Vec<std::path::PathBuf> {
    let mut roots = Vec::new();

    if let Some(parent) = workspace.workspace.usage_state_path.parent() {
        if let Ok(abs) = std::path::absolute(parent) {
            roots.push(abs.canonicalize().unwrap_or(abs));
        }
    }

    let ws_id_path = std::path::PathBuf::from(&workspace.workspace_id);
    if ws_id_path.exists() {
        if let Ok(abs) = std::path::absolute(&ws_id_path) {
            roots.push(abs.canonicalize().unwrap_or(abs));
        }
    }

    let settings_dir = std::path::PathBuf::from(
        crate::settings::XavierSettings::current()
            .memory
            .workspace_dir,
    );
    if let Ok(abs) = std::path::absolute(&settings_dir) {
        roots.push(abs.canonicalize().unwrap_or(abs));
    }

    for extra in crate::codebase::repo_identity::extra_code_roots() {
        roots.push(extra);
    }

    if let Ok(val) = std::env::var("XAVIER_SWAL_WORKSPACE") {
        if !val.trim().is_empty() {
            let p = std::path::PathBuf::from(val);
            if let Ok(abs) = std::path::absolute(&p) {
                roots.push(abs.canonicalize().unwrap_or(abs));
            }
        }
    }

    if let Ok(val) = std::env::var("SWAL_REPOS_DIR") {
        if !val.trim().is_empty() {
            let p = std::path::PathBuf::from(val);
            if let Ok(abs) = std::path::absolute(&p) {
                roots.push(abs.canonicalize().unwrap_or(abs));
            }
        }
    }

    // Only git repository roots may receive gitcore writes. A root that cannot be
    // canonicalized is dropped (fail closed) rather than used unresolved.
    let mut git_roots: Vec<std::path::PathBuf> = roots
        .into_iter()
        .filter_map(|r| r.canonicalize().ok())
        .filter(|r| r.join(".git").exists())
        .collect();
    git_roots.dedup();
    git_roots
}

/// Validates that project_path has no traversal and sits under allowed roots.
fn resolve_gitcore_project_path(
    workspace: &WorkspaceContext,
    project_path: &str,
) -> anyhow::Result<std::path::PathBuf> {
    if std::path::Path::new(project_path)
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        anyhow::bail!("Path traversal rejected: '..' is not allowed in project_path");
    }

    let raw_path = std::path::PathBuf::from(project_path);
    let workspace_root = workspace
        .workspace
        .usage_state_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let target = if raw_path.is_absolute() {
        raw_path
    } else {
        workspace_root.join(&raw_path)
    };

    #[cfg(test)]
    let is_mock_fixture = project_path == "/non/existent/path";
    #[cfg(not(test))]
    let is_mock_fixture = false;

    // Fail closed: a path that cannot be resolved cannot be proven to stay inside a root.
    let canonical = if is_mock_fixture {
        target.clone()
    } else {
        target
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("cannot resolve project path {project_path}: {e}"))?
    };
    let allowed_roots = allowed_project_roots(workspace);

    let is_allowed =
        is_mock_fixture || allowed_roots.iter().any(|root| canonical.starts_with(root));

    if !is_allowed {
        anyhow::bail!("Path outside allowed roots rejected: {}", project_path);
    }

    Ok(canonical)
}

/// Handle core tool.
pub async fn handle_core_tool(
    _state: AppState,
    workspace: WorkspaceContext,
    claims: Option<&crate::security::auth::Claims>,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    match name {
        "list_projects" => {
            let projects = workspace
                .workspace
                .durable_store()
                .project_counts(&workspace.workspace_id)
                .await?;

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
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| anyhow::anyhow!("'project_id' must be a non-empty string"))?;
            // Absent -> default; present but not a non-negative integer -> error.
            let int_arg = |key: &str, default: u64| -> anyhow::Result<u64> {
                match arguments.get(key) {
                    None | Some(Value::Null) => Ok(default),
                    Some(v) => v
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("'{key}' must be a non-negative integer")),
                }
            };
            let page = int_arg("page", 1)?.clamp(1, 1000) as usize;
            let max_records_key = if arguments.get("max_records").is_some() {
                "max_records"
            } else {
                "limit"
            };
            let max_records = int_arg(max_records_key, 10)?.clamp(1, 50) as usize;
            let max_chars = int_arg("max_chars", 8000)?.clamp(1, 32000) as usize;
            // Reserved: validated but has no effect.
            int_arg("depth", 0)?;

            const MAX_FETCH_RECORDS: usize = 1000;
            let wanted = page.saturating_mul(max_records).saturating_add(1);
            let fetch_capped = wanted > MAX_FETCH_RECORDS;
            let fetch_limit = wanted.min(MAX_FETCH_RECORDS);
            let finalize = |mut payload: Value| -> Value {
                payload["depth_applied"] = json!(0);
                if fetch_capped {
                    payload["truncated"] = json!(true);
                    if payload.get("truncatedReason").is_none_or(|v| v.is_null()) {
                        payload["truncatedReason"] =
                            json!(format!("fetch capped at {MAX_FETCH_RECORDS} records"));
                    }
                }
                payload
            };
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

            let offset = page.saturating_sub(1).saturating_mul(max_records);
            let total_fetched = all_records.len();
            let has_more = total_fetched > page.saturating_mul(max_records);
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
                        finalize(json!(MCPContextResult {
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
                        })),
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
                    finalize(json!(MCPContextResult {
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
                    })),
                    false,
                ))?);
            }

            let content = content_parts.join("\n\n---\n\n");
            let total_records = content_parts.len();
            let estimated_tokens = crate::context::estimate_tokens(&content);

            Ok(serde_json::to_value(MCPToolResult::structured(
                finalize(json!(MCPContextResult {
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
                })),
                false,
            ))?)
        }
        "sync_gitcore" => {
            let project_path = arguments
                .get("project_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing project_path"))?;

            let root = resolve_gitcore_project_path(&workspace, project_path)?;
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

                validate_memory_write_clearance(&workspace, &path, &metadata, typed.as_ref())?;

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
            let health = collect_health_blocking().await;
            let tools_count = served_tools_count();

            // The store is OK when it actually answers a query; a measured SQLite
            // integrity failure overrides that.
            let store_reachable = workspace.workspace.memory.count().await.is_ok();
            let mut result = mcp_health_result(&health, tools_count, store_reachable);
            result.embedding_ok = shared_embedding_probe().await;

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
            let in_process_health = collect_health_blocking().await;

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
                "components": components_json(&in_process_health),
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
            let parsed = (|| -> Result<crate::self_manage::LogScanArgs, String> {
                let since = optional_str_arg(&arguments, "since")?;
                if let Some(since) = since {
                    chrono::DateTime::parse_from_rfc3339(since).map_err(|e| {
                        format!("Invalid argument 'since': expected RFC3339 timestamp ({e})")
                    })?;
                }
                let pattern = optional_str_arg(&arguments, "pattern")?;
                if let Some(pattern) = pattern {
                    regex::Regex::new(pattern)
                        .map_err(|e| format!("Invalid argument 'pattern': invalid regex ({e})"))?;
                }
                let max_entries = match arguments.get("max_entries") {
                    None | Some(Value::Null) => 500,
                    Some(v) => match v.as_u64() {
                        Some(n) if n >= 1 => usize::try_from(n).unwrap_or(usize::MAX),
                        _ => {
                            return Err(
                                "Invalid argument 'max_entries': expected integer >= 1".to_string()
                            )
                        }
                    },
                };
                Ok(crate::self_manage::LogScanArgs {
                    since: since.map(str::to_string),
                    level_min: optional_str_arg(&arguments, "level_min")?.map(str::to_string),
                    pattern: pattern.map(str::to_string),
                    source: optional_str_arg(&arguments, "source")?.map(str::to_string),
                    max_entries,
                })
            })();
            let args = match parsed {
                Ok(args) => args,
                Err(msg) => return Ok(serde_json::to_value(MCPToolResult::text(msg, true))?),
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
            let parsed = (|| -> Result<TicketCreateArgs, String> {
                let labels = match arguments.get("labels") {
                    None | Some(Value::Null) => None,
                    Some(Value::Array(items)) => Some(
                        items
                            .iter()
                            .map(|item| item.as_str().map(str::to_string))
                            .collect::<Option<Vec<String>>>()
                            .ok_or("Invalid argument 'labels': expected array of strings")?,
                    ),
                    Some(_) => {
                        return Err("Invalid argument 'labels': expected array of strings".into())
                    }
                };
                let args = TicketCreateArgs {
                    title: required_str_arg(&arguments, "title")?.to_string(),
                    body: required_str_arg(&arguments, "body")?.to_string(),
                    labels,
                    severity: required_str_arg(&arguments, "severity")?.to_string(),
                    fingerprint: optional_str_arg(&arguments, "fingerprint")?.map(str::to_string),
                    backend: optional_str_arg(&arguments, "backend")?.map(str::to_string),
                };
                crate::self_manage::validate_ticket_args(&args)?;
                Ok(args)
            })();
            let args = match parsed {
                Ok(args) => args,
                Err(msg) => return Ok(serde_json::to_value(MCPToolResult::text(msg, true))?),
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
            let embedding_reachable = shared_embedding_probe().await;
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
            let code_db = _state.code_db.clone();
            let code_query = _state.code_query.clone();
            let (stats, hubs) = match tokio::time::timeout(
                std::time::Duration::from_secs(20),
                tokio::task::spawn_blocking(move || {
                    let stats = code_db.stats().unwrap_or(code_graph::types::IndexStats {
                        total_files: 0,
                        total_symbols: 0,
                        total_imports: 0,
                        languages: vec![],
                        duration_ms: 0,
                    });
                    let hubs = code_query.hubs(0, 20).unwrap_or_default();
                    (stats, hubs)
                }),
            )
            .await
            {
                Ok(res) => res.map_err(|e| anyhow::anyhow!("spawn_blocking failed: {e}"))?,
                Err(_) => anyhow::bail!("get_code_graph timed out after 20 seconds"),
            };

            let relative_dump_path = {
                let mut rel = None;
                if let Ok(cwd) = std::env::current_dir() {
                    if let Ok(p) = dump_path.strip_prefix(&cwd) {
                        rel = Some(p.to_path_buf());
                    }
                }
                if rel.is_none() {
                    for root in allowed_project_roots(&workspace) {
                        if let Ok(p) = dump_path.strip_prefix(&root) {
                            rel = Some(p.to_path_buf());
                            break;
                        }
                    }
                }
                match rel {
                    Some(p) => p.display().to_string(),
                    None => {
                        if dump_path.is_absolute() {
                            ".xavier/codegraph.json".to_string()
                        } else {
                            dump_path.display().to_string()
                        }
                    }
                }
            };

            let summary = json!({
                "source": "live_db",
                "dump_path": relative_dump_path,
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
            let query = match arguments.get("query") {
                Some(v) => v
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("'query' must be a string"))?,
                None => anyhow::bail!("Missing 'query'"),
            };
            let limit = match arguments.get("limit") {
                Some(v) => {
                    let n = v
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("'limit' must be a non-negative integer"))?;
                    n.min(100) as usize
                }
                None => 20,
            };

            if limit == 0 {
                let val = json!({
                    "returned": 0,
                    "symbols": [],
                });
                return Ok(serde_json::to_value(MCPToolResult::structured(val, false))?);
            }
            if query.trim().is_empty() {
                let val = json!({
                    "returned": 0,
                    "symbols": [],
                    "hint": "Provide a non-empty 'query' (symbol name or keyword).",
                });
                return Ok(serde_json::to_value(MCPToolResult::structured(val, false))?);
            }

            let code_query = _state.code_query.clone();
            let query_owned = query.to_string();
            let result = match tokio::time::timeout(
                std::time::Duration::from_secs(20),
                tokio::task::spawn_blocking(move || code_query.search(&query_owned, limit)),
            )
            .await
            {
                Ok(res) => res
                    .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {e}"))?
                    .map_err(|e| anyhow::anyhow!("{e}"))?,
                Err(_) => anyhow::bail!("codegraph_explore timed out after 20 seconds"),
            };

            let returned = result.symbols.len();
            let val = json!({
                "returned": returned,
                "symbols": result.symbols,
            });

            Ok(serde_json::to_value(MCPToolResult::structured(val, false))?)
        }
        "trace_path" => {
            let symbol = match arguments.get("symbol") {
                Some(v) => v
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("'symbol' must be a string"))?,
                None => anyhow::bail!("Missing 'symbol'"),
            };
            let max_depth = match arguments.get("max_depth") {
                Some(v) => {
                    let n = v
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("'max_depth' must be an integer"))?;
                    n.clamp(1, 8) as usize
                }
                None => 3,
            };
            let reverse = match arguments.get("reverse") {
                Some(v) => v
                    .as_bool()
                    .ok_or_else(|| anyhow::anyhow!("'reverse' must be a boolean"))?,
                None => false,
            };
            let limit = match arguments.get("limit") {
                Some(v) => {
                    let n = v
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("'limit' must be a non-negative integer"))?;
                    n.clamp(1, 1000) as usize
                }
                None => 100,
            };

            let edge_type_filter = match arguments.get("edge_type") {
                Some(v) => {
                    let s = v
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("'edge_type' must be a string"))?;
                    match s.to_lowercase().as_str() {
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
                        _ => anyhow::bail!(
                            "Unknown edge_type: '{s}'. Valid values are: calls, defines, uses, imports, exports, contains, references, extends, implements, typeof, returns, instantiates, overrides, decorates"
                        ),
                    }
                }
                None => None,
            };

            let code_query = _state.code_query.clone();
            let code_db = _state.code_db.clone();
            let symbol_owned = symbol.to_string();
            let structural_filter = reverse && edge_type_filter.is_none();
            let (edges, names, found, truncated) = match tokio::time::timeout(
                std::time::Duration::from_secs(20),
                tokio::task::spawn_blocking(move || {
                    // Fetch one extra edge so a cut can be detected.
                    let fetch = limit + 1;
                    let mut edges = if reverse {
                        code_query.reverse_dependencies(
                            &symbol_owned,
                            edge_type_filter,
                            max_depth,
                            fetch,
                        )?
                    } else {
                        code_query.dependencies(
                            &symbol_owned,
                            edge_type_filter,
                            max_depth,
                            fetch,
                        )?
                    };
                    if structural_filter {
                        // Callers traces skip file->symbol structural edges.
                        edges.retain(|e| {
                            !matches!(
                                e.edge_type,
                                code_graph::types::EdgeType::Contains
                                    | code_graph::types::EdgeType::Defines
                            )
                        });
                    }
                    let truncated = edges.len() > limit || (limit >= 1000 && edges.len() >= limit);
                    edges.truncate(limit);

                    let mut names = std::collections::HashMap::new();
                    for e in &edges {
                        for id in [&e.from_symbol, &e.to_symbol] {
                            if names.contains_key(id) {
                                continue;
                            }
                            if let Some(sym) = code_db.symbol_by_stable_id(id)? {
                                names.insert(id.clone(), sym.name);
                            }
                        }
                    }

                    let found = if !edges.is_empty() {
                        true
                    } else if symbol_owned.len() == 64
                        && symbol_owned.chars().all(|c| c.is_ascii_hexdigit())
                    {
                        code_db.symbol_by_stable_id(&symbol_owned)?.is_some()
                    } else {
                        !code_db.find_by_name(&symbol_owned, 1)?.is_empty()
                            || !code_db.find_symbols(&symbol_owned, 1)?.symbols.is_empty()
                    };
                    Ok::<_, code_graph::GraphError>((edges, names, found, truncated))
                }),
            )
            .await
            {
                Ok(res) => res
                    .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {e}"))?
                    .map_err(|e| anyhow::anyhow!("{e}"))?,
                Err(_) => anyhow::bail!("trace_path timed out after 20 seconds"),
            };

            let direction = if reverse { "callers" } else { "dependencies" };
            let edges_json: Vec<Value> = edges
                .iter()
                .map(|e| {
                    let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
                    if let Some(obj) = v.as_object_mut() {
                        for (key, id) in [("from_name", &e.from_symbol), ("to_name", &e.to_symbol)]
                        {
                            let name = names.get(id).cloned().unwrap_or_else(|| id.clone());
                            obj.insert(key.to_string(), Value::String(name));
                        }
                    }
                    v
                })
                .collect();
            let mut val = json!({
                "symbol": symbol,
                "direction": direction,
                "found": found,
                "edges": edges_json,
            });
            if truncated {
                val["truncated"] = json!(true);
            }
            if !found {
                val["hint"] = json!(
                    "No indexed symbol matches this name; run a code scan or use codegraph_explore to find the exact name."
                );
            }

            Ok(serde_json::to_value(MCPToolResult::structured(val, false))?)
        }
        "codegraph_route" => {
            let query = match arguments.get("query") {
                Some(v) => v
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("'query' must be a string"))?,
                None => anyhow::bail!("Missing 'query'"),
            };
            let limit = match arguments.get("limit") {
                Some(v) => {
                    let n = v
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("'limit' must be a non-negative integer"))?;
                    n.min(100) as usize
                }
                None => 10,
            };

            if limit == 0 {
                let val = json!({
                    "mode": "ExactName",
                    "margin": 0.0,
                    "confident": false,
                    "refusal": true,
                    "est_tokens": 0,
                    "total": 0,
                    "shown": 0,
                    "hits": [],
                });
                return Ok(serde_json::to_value(MCPToolResult::structured(val, false))?);
            }

            let route_refusal = |reason: &str| {
                json!({
                    "mode": "ExactName",
                    "margin": 0.0,
                    "confident": false,
                    "refusal": true,
                    "est_tokens": 0,
                    "total": 0,
                    "shown": 0,
                    "hits": [],
                    "reason": reason,
                })
            };
            if query.trim().is_empty() {
                let val = route_refusal("Empty query: provide a symbol name or keyword.");
                return Ok(serde_json::to_value(MCPToolResult::structured(val, false))?);
            }

            let code_query = _state.code_query.clone();
            let query_owned = query.to_string();
            let routed = match tokio::time::timeout(
                std::time::Duration::from_secs(20),
                tokio::task::spawn_blocking(move || code_query.route_query(&query_owned, limit)),
            )
            .await
            {
                Ok(res) => res.map_err(|e| anyhow::anyhow!("spawn_blocking failed: {e}"))?,
                Err(_) => anyhow::bail!("codegraph_route timed out after 20 seconds"),
            };
            let report = match routed {
                Ok(r) => r,
                Err(
                    code_graph::GraphError::UnknownSymbol { .. } | code_graph::GraphError::Query(_),
                ) => {
                    let val = route_refusal(&format!(
                        "No indexed symbol matches '{query}'. Try an exact symbol name."
                    ));
                    return Ok(serde_json::to_value(MCPToolResult::structured(val, false))?);
                }
                Err(e) => return Err(anyhow::anyhow!("{e}")),
            };

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
            let limit = match arguments.get("limit") {
                Some(v) => {
                    let n = v
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("'limit' must be a non-negative integer"))?;
                    n.min(100) as usize
                }
                None => 10,
            };

            if limit == 0 {
                let val = json!({
                    "returned": 0,
                    "god_nodes": [],
                    "reason": "Limit 0 requested."
                });
                return Ok(serde_json::to_value(MCPToolResult::structured(val, false))?);
            }

            let code_query = _state.code_query.clone();
            let fetch_limit = limit.saturating_mul(10).clamp(50, 200);
            let raw_hubs = match tokio::time::timeout(
                std::time::Duration::from_secs(20),
                tokio::task::spawn_blocking(move || code_query.god_nodes(fetch_limit)),
            )
            .await
            {
                Ok(res) => res
                    .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {e}"))?
                    .map_err(|e| anyhow::anyhow!("{e}"))?,
                Err(_) => anyhow::bail!("codegraph_gods timed out after 20 seconds"),
            };

            let mut unique_names = std::collections::HashSet::new();
            let mut hubs = Vec::new();
            for hub in raw_hubs {
                if matches!(
                    hub.symbol.kind,
                    code_graph::types::SymbolKind::Variable | code_graph::types::SymbolKind::Import
                ) {
                    continue;
                }
                let name = hub.symbol.name.to_lowercase();
                if [
                    "join",
                    "contains",
                    "collect",
                    "len",
                    "iter",
                    "map",
                    "unwrap",
                    "to_string",
                    "push",
                    "insert",
                    "get",
                    "clone",
                    "new",
                    "default",
                    "from",
                    "with_capacity",
                    "as_str",
                    "as_ref",
                    "into",
                    "ok",
                    "err",
                    "is_empty",
                    "clear",
                    "is_some",
                    "is_none",
                ]
                .contains(&name.as_str())
                {
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
            let space_id = match required_str_arg(&arguments, "space_id") {
                Ok(v) => v,
                Err(msg) => return Ok(serde_json::to_value(MCPToolResult::text(msg, true))?),
            };
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
            let space_id = match required_str_arg(&arguments, "space_id") {
                Ok(v) => v,
                Err(msg) => return Ok(serde_json::to_value(MCPToolResult::text(msg, true))?),
            };
            let name = match required_str_arg(&arguments, "name") {
                Ok(v) => v,
                Err(msg) => return Ok(serde_json::to_value(MCPToolResult::text(msg, true))?),
            };
            let len = name.chars().count();
            if len > crate::espacio::channel::MAX_MESSAGE_CHARS {
                return Ok(serde_json::to_value(MCPToolResult::text(
                    format!(
                        "Invalid argument 'name': {len} characters exceeds the limit of {}",
                        crate::espacio::channel::MAX_MESSAGE_CHARS
                    ),
                    true,
                ))?);
            }
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
                "channel_name": msg.content,
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
        "espacio_member_list" => {
            let manager = espacio_manager_for_root()?;
            let space_id = match required_str_arg(&arguments, "space_id") {
                Ok(v) => v,
                Err(msg) => return Ok(serde_json::to_value(MCPToolResult::text(msg, true))?),
            };
            let members = manager.members(space_id).await?;
            let val = json!({
                "space_id": space_id,
                "members": members,
            });
            Ok(serde_json::to_value(MCPToolResult::structured(val, false))?)
        }
        "espacio_member_add" => {
            let manager = espacio_manager_for_root()?;
            let space_id = match required_str_arg(&arguments, "space_id") {
                Ok(v) => v,
                Err(msg) => return Ok(serde_json::to_value(MCPToolResult::text(msg, true))?),
            };
            let node_id = match required_str_arg(&arguments, "node_id") {
                Ok(v) => v,
                Err(msg) => return Ok(serde_json::to_value(MCPToolResult::text(msg, true))?),
            };
            let role_str = match required_str_arg(&arguments, "role") {
                Ok(v) => v,
                Err(msg) => return Ok(serde_json::to_value(MCPToolResult::text(msg, true))?),
            };
            let role =
                match role_str {
                    "admin" => crate::espacio::invite::SpaceRole::Admin,
                    "moderator" => crate::espacio::invite::SpaceRole::Moderator,
                    "member" => crate::espacio::invite::SpaceRole::Member,
                    "reader" => crate::espacio::invite::SpaceRole::Reader,
                    _ => return Ok(serde_json::to_value(MCPToolResult::text(
                        "Invalid argument 'role': expected one of admin, moderator, member, reader",
                        true,
                    ))?),
                };

            match manager.add_member(space_id, node_id, role).await {
                Ok(membership) => {
                    let val = json!({
                        "space_id": space_id,
                        "membership": membership,
                    });
                    Ok(serde_json::to_value(MCPToolResult::structured(val, false))?)
                }
                Err(err) => {
                    if let Some(_cap_err) =
                        err.downcast_ref::<crate::espacio::manager::MembersCapReached>()
                    {
                        Ok(serde_json::to_value(MCPToolResult::text(
                            err.to_string(),
                            true,
                        ))?)
                    } else {
                        Err(err)
                    }
                }
            }
        }
        "espacio_member_remove" => {
            let manager = espacio_manager_for_root()?;
            let space_id = match required_str_arg(&arguments, "space_id") {
                Ok(v) => v,
                Err(msg) => return Ok(serde_json::to_value(MCPToolResult::text(msg, true))?),
            };
            let node_id = match required_str_arg(&arguments, "node_id") {
                Ok(v) => v,
                Err(msg) => return Ok(serde_json::to_value(MCPToolResult::text(msg, true))?),
            };
            let removed = manager.remove_member(space_id, node_id).await?;
            let val = json!({
                "space_id": space_id,
                "node_id": node_id,
                "removed": removed,
            });
            Ok(serde_json::to_value(MCPToolResult::structured(val, false))?)
        }
        "recovery_status" => {
            let crypt = arguments
                .get("crypt_passphrase_backed_up")
                .and_then(|v| v.as_bool());
            use crate::security::auth::Permission;
            if !claims.is_some_and(|c| c.role.can_edit_config()) {
                return Err(anyhow::anyhow!(
                    "Forbidden: Insufficient permissions to execute tool 'recovery_status'"
                ));
            }
            let store = crate::recovery::RecoveryStore::from_env_or_default();

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
pub(crate) mod tests {
    use super::*;

    #[test]
    fn health_check_tools_count_matches_tools_list() {
        assert_eq!(
            served_tools_count(),
            super::super::server::get_xavier_tools().len()
        );
        let partial = get_xavier_core_tools().len()
            + crate::server::mcp::tools_memory::get_xavier_memory_tools().len()
            + super::super::tools_context::get_xavier_context_tools().len();
        assert!(served_tools_count() > partial);
    }

    #[test]
    fn health_check_unprobed_embedding_is_not_ok() {
        let mut e = crate::observability::health::HealthStatus::default().embedding;
        assert!(
            !embedding_probe_ok(&e),
            "default (never probed) must not be OK"
        );
        e.provider = "ollama".into();
        assert!(embedding_probe_ok(&e));
        e.available = false;
        assert!(!embedding_probe_ok(&e));
        e.available = true;
        e.status = crate::observability::health::HealthLevel::Unhealthy;
        assert!(!embedding_probe_ok(&e));
    }

    #[test]
    fn health_check_unmeasured_database_is_unknown() {
        let mut h = crate::health::fast_degraded_health_fallback();
        h.database.path = String::new();
        let v = components_json(&h);
        assert_eq!(v["database"]["status"], "unknown");
        assert!(v["database"].get("size_mb").is_none());
        h.database.path = "/x/db.sqlite3".into();
        assert_eq!(components_json(&h)["database"]["path"], "/x/db.sqlite3");
    }

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
    async fn ticket_create_rejects_invalid_args() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let claims = admin_claims();
        let valid = json!({"title": "t", "body": "b", "severity": "warn"});
        let with = |key: &str, value: Value| {
            let mut args = valid.clone();
            args[key] = value;
            args
        };
        let mut no_title = valid.clone();
        no_title.as_object_mut().unwrap().remove("title");
        let cases = vec![
            (no_title, "Missing required parameter: title"),
            (with("title", json!(7)), "'title': expected string"),
            (with("title", json!("  ")), "'title': must not be empty"),
            (with("title", json!("t".repeat(121))), "'title': exceeds"),
            (with("body", json!(["b"])), "'body': expected string"),
            (
                with("body", json!("b".repeat(8 * 1024 + 1))),
                "'body': exceeds",
            ),
            (with("severity", json!("info")), "'severity'"),
            (with("severity", json!(1)), "'severity': expected string"),
            (with("backend", json!("jira")), "'backend'"),
            (with("backend", json!(true)), "'backend': expected string"),
            (with("labels", json!("x")), "'labels'"),
            (with("labels", json!(["a", 1])), "'labels'"),
            (with("fingerprint", json!(3)), "'fingerprint'"),
        ];
        for (args, needle) in cases {
            let res = handle_core_tool(
                state.clone(),
                workspace.clone(),
                Some(&claims),
                "ticket_create",
                args.clone(),
            )
            .await
            .expect("validation failures are tool results");
            assert_eq!(res["isError"], true, "{args} -> {res}");
            let text = res["content"][0]["text"].as_str().unwrap();
            assert!(
                text.contains(needle),
                "{args}: expected {needle:?}, got {text}"
            );
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

    #[tokio::test]
    async fn huge_page_returns_empty_without_panicking() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let claims = admin_claims();
        let output = handle_core_tool(
            state,
            workspace,
            Some(&claims),
            "get_project_context",
            json!({
                "project_id": "huge_page_test",
                "page": u64::MAX
            }),
        )
        .await
        .expect("should not panic and should succeed");

        let content = output["structuredContent"].as_object().unwrap();
        // `MCPContextResult` renames its fields to camelCase on the wire, so the
        // record count arrives as `totalRecords` (see `server::mcp::types`);
        // indexing it by the Rust field name panics instead of asserting.
        let total_records = content
            .get("totalRecords")
            .or_else(|| content.get("total_records"))
            .unwrap_or_else(|| panic!("no record count in structuredContent: {output}"));
        assert_eq!(total_records, 0, "Huge page should return 0 records");
        assert_eq!(
            content.get("page").and_then(|page| page.as_u64()),
            Some(1000),
            "Huge page should be clamped to the maximum supported page"
        );
    }

    async fn project_context_call(args: Value) -> anyhow::Result<Value> {
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let claims = admin_claims();
        handle_core_tool(state, workspace, Some(&claims), "get_project_context", args).await
    }

    #[tokio::test]
    async fn project_context_rejects_bad_project_id() {
        crate::isolate_test_process!();
        for bad in [json!(7), json!(""), json!("   "), json!(null)] {
            let err = project_context_call(json!({ "project_id": bad }))
                .await
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("'project_id' must be a non-empty string"),
                "{bad}: {err}"
            );
        }
        let err = project_context_call(json!({}))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("'project_id' must be a non-empty string"));
    }

    #[tokio::test]
    async fn project_context_rejects_non_integer_numeric_args() {
        crate::isolate_test_process!();
        for key in ["page", "max_records", "max_chars", "depth"] {
            for bad in [json!("2"), json!(1.5), json!(-1)] {
                let err = project_context_call(json!({ "project_id": "p", key: bad }))
                    .await
                    .unwrap_err()
                    .to_string();
                assert!(err.contains(&format!("'{key}'")), "{key}={bad}: {err}");
            }
        }
        // Null means "not provided" for the optional numeric arguments.
        let out = project_context_call(json!({
            "project_id": "p", "page": null, "max_records": null, "max_chars": null, "depth": null
        }))
        .await
        .expect("nulls default");
        assert_eq!(out["structuredContent"]["limit"], 10);
        // Absent and out-of-range positive values still succeed (clamped).
        let out = project_context_call(json!({
            "project_id": "p", "page": 5000, "max_records": 9999, "max_chars": 999999
        }))
        .await
        .expect("clamped");
        assert_eq!(out["structuredContent"]["limit"], 50);
    }

    #[tokio::test]
    async fn project_context_echoes_depth_applied_zero() {
        crate::isolate_test_process!();
        let out = project_context_call(json!({ "project_id": "p", "depth": 2 }))
            .await
            .expect("ok");
        assert_eq!(out["structuredContent"]["depth_applied"], 0);
        assert_eq!(out["structuredContent"]["truncated"], false);
    }

    #[tokio::test]
    async fn project_context_reports_fetch_cap() {
        crate::isolate_test_process!();
        let out = project_context_call(json!({
            "project_id": "p", "page": 1000, "max_records": 50
        }))
        .await
        .expect("ok");
        assert_eq!(out["structuredContent"]["truncated"], true);
        assert!(out["structuredContent"]["truncatedReason"]
            .as_str()
            .unwrap()
            .contains("1000"));
    }

    fn root_claims() -> crate::security::auth::Claims {
        crate::security::auth::Claims::new(
            "root".to_string(),
            "admin@swal.dev".to_string(),
            crate::security::auth::UserRole::Admin,
            chrono::Duration::hours(1),
        )
    }

    /// Puts `XAVIER_RECOVERY_DIR` where a test says, and puts the previous value
    /// back when it drops — including when the test body unwinds after a failed
    /// assertion. A hand-rolled restore at the end of the body silently leaks a
    /// tempdir path into the next test of the serialised suite.
    pub(crate) struct RecoveryDirGuard {
        previous: Option<std::ffi::OsString>,
    }

    impl RecoveryDirGuard {
        pub(crate) fn at(dir: &std::path::Path) -> Self {
            let previous = std::env::var_os("XAVIER_RECOVERY_DIR");
            std::env::set_var("XAVIER_RECOVERY_DIR", dir);
            Self { previous }
        }
    }

    impl Drop for RecoveryDirGuard {
        fn drop(&mut self) {
            match self.previous.as_ref() {
                Some(previous) => std::env::set_var("XAVIER_RECOVERY_DIR", previous),
                None => std::env::remove_var("XAVIER_RECOVERY_DIR"),
            }
        }
    }

    fn admin_claims() -> crate::security::auth::Claims {
        crate::security::auth::Claims::new(
            "operator".to_string(),
            "local@xavier".to_string(),
            crate::security::auth::UserRole::Admin,
            chrono::Duration::hours(1),
        )
    }

    /// `recovery_status` reports the CONFIGURED recovery directory, never a
    /// caller-supplied one: the override argument is gone from the schema and
    /// the store is resolved from `XAVIER_RECOVERY_DIR`.
    #[tokio::test]
    #[serial_test::serial]
    async fn recovery_status_uses_configured_recovery_dir() {
        crate::isolate_test_process!();
        let configured = tempfile::tempdir().expect("tempdir");
        let caller = tempfile::tempdir().expect("tempdir");
        let _guard = RecoveryDirGuard::at(configured.path());
        crate::recovery::RecoveryStore::at(configured.path())
            .write_kcv(&[7; 32])
            .expect("seed kcv");

        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let claims = admin_claims();
        let output = handle_core_tool(
            state,
            workspace,
            Some(&claims),
            "recovery_status",
            json!({
                "recovery_dir": caller.path(),
                "crypt_passphrase_backed_up": true
            }),
        )
        .await
        .expect("admin status");

        assert_eq!(output["structuredContent"]["kcvPresent"], true);
        assert_eq!(output["structuredContent"]["cryptPassphraseBackedUp"], true);

        let schema = crate::server::mcp::server::get_xavier_tools()
            .into_iter()
            .find(|t| t.name == "recovery_status")
            .expect("recovery_status advertised")
            .input_schema;
        assert!(schema["properties"].get("recovery_dir").is_none());
    }

    /// The guard's whole point: an unwinding body still restores the variable.
    #[tokio::test]
    #[serial_test::serial]
    async fn recovery_dir_guard_restores_on_panic() {
        crate::isolate_test_process!();
        let configured = tempfile::tempdir().expect("tempdir");
        let scratch = tempfile::tempdir().expect("tempdir");
        let _guard = RecoveryDirGuard::at(configured.path());
        let previous = std::env::var_os("XAVIER_RECOVERY_DIR");

        fn set_then_unwind(dir: &std::path::Path) {
            let _guard = RecoveryDirGuard::at(dir);
            panic!("the guard must still be dropped on unwind");
        }

        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            set_then_unwind(scratch.path());
        }));
        std::panic::set_hook(hook);

        assert!(outcome.is_err(), "the body did unwind");
        assert_eq!(
            std::env::var_os("XAVIER_RECOVERY_DIR"),
            previous,
            "guard must restore the previous value even when the body panics"
        );
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

    #[tokio::test]
    async fn log_scan_rejects_invalid_args() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let claims = admin_claims();
        let cases = [
            (json!({"pattern": "(unclosed"}), "'pattern': invalid regex"),
            (json!({"pattern": 5}), "'pattern': expected string"),
            (json!({"since": "yesterday"}), "'since': expected RFC3339"),
            (json!({"since": 1}), "'since': expected string"),
            (json!({"max_entries": "10"}), "'max_entries'"),
            (json!({"max_entries": -1}), "'max_entries'"),
            (json!({"max_entries": 2.5}), "'max_entries'"),
            (json!({"max_entries": 0}), "'max_entries'"),
            (
                json!({"level_min": ["warn"]}),
                "'level_min': expected string",
            ),
            (json!({"source": false}), "'source': expected string"),
        ];
        for (args, needle) in cases {
            let res = handle_core_tool(
                state.clone(),
                workspace.clone(),
                Some(&claims),
                "log_scan",
                args.clone(),
            )
            .await
            .expect("validation failures are tool results");
            assert_eq!(res["isError"], true, "{args} -> {res}");
            let text = res["content"][0]["text"].as_str().unwrap();
            assert!(
                text.contains(needle),
                "{args}: expected {needle:?}, got {text}"
            );
        }
    }

    async fn espacio_root_call(
        st: &(AppState, WorkspaceContext),
        name: &str,
        args: Value,
    ) -> anyhow::Result<Value> {
        let root = root_claims();
        crate::server::mcp::server::with_root_credential(
            true,
            handle_core_tool(st.0.clone(), st.1.clone(), Some(&root), name, args),
        )
        .await
    }

    async fn espacio_fixture(space_id: &str) -> (tempfile::TempDir, (AppState, WorkspaceContext)) {
        use crate::espacio::{SpaceManager, StaticNodeKek};
        let tmp = tempfile::tempdir().expect("tempdir");
        let kek = std::sync::Arc::new(StaticNodeKek::new([7u8; 32]));
        let manager = std::sync::Arc::new(SpaceManager::open_with_keys(tmp.path(), kek));
        crate::adapters::inbound::http::routes::init_space_manager(manager.clone());
        manager
            .create(
                space_id.into(),
                "mcp".into(),
                "".into(),
                "owner".into(),
                false,
            )
            .await
            .expect("create space");
        (tmp, crate::server::mcp::tests::test_state().await)
    }

    #[tokio::test]
    async fn espacio_channel_wrong_type_message() {
        let (_tmp, st) = espacio_fixture("esp_types").await;
        let cases = [
            (
                "espacio_channel_list",
                json!({}),
                "Missing required parameter: space_id",
            ),
            (
                "espacio_channel_list",
                json!({"space_id": 3}),
                "'space_id': expected string",
            ),
            (
                "espacio_channel_create",
                json!({"space_id": "esp_types"}),
                "Missing required parameter: name",
            ),
            (
                "espacio_channel_create",
                json!({"space_id": "esp_types", "name": 9}),
                "'name': expected string",
            ),
            (
                "espacio_channel_create",
                json!({"space_id": ["esp_types"], "name": "x"}),
                "'space_id': expected string",
            ),
            (
                "espacio_member_list",
                json!({"space_id": {}}),
                "'space_id': expected string",
            ),
            (
                "espacio_member_add",
                json!({"space_id": "esp_types", "node_id": 1, "role": "member"}),
                "'node_id': expected string",
            ),
            (
                "espacio_member_add",
                json!({"space_id": "esp_types", "node_id": "n", "role": true}),
                "'role': expected string",
            ),
            (
                "espacio_member_add",
                json!({"space_id": "esp_types", "node_id": "n", "role": "owner"}),
                "'role': expected one of",
            ),
            (
                "espacio_member_remove",
                json!({"space_id": "esp_types"}),
                "Missing required parameter: node_id",
            ),
        ];
        for (tool, args, needle) in cases {
            let res = espacio_root_call(&st, tool, args.clone())
                .await
                .expect("validation failures are tool results");
            assert_eq!(res["isError"], true, "{tool} {args} -> {res}");
            let text = res["content"][0]["text"].as_str().unwrap();
            assert!(
                text.contains(needle),
                "{tool} {args}: expected {needle:?}, got {text}"
            );
        }
    }

    #[tokio::test]
    async fn espacio_channel_create_rejects_over_limit_content() {
        let (_tmp, st) = espacio_fixture("esp_limit").await;
        let limit = crate::espacio::channel::MAX_MESSAGE_CHARS;
        let res = espacio_root_call(
            &st,
            "espacio_channel_create",
            json!({"space_id": "esp_limit", "name": "x".repeat(limit + 1)}),
        )
        .await
        .expect("tool result");
        assert_eq!(res["isError"], true, "{res}");
        assert!(res["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("'name'"));

        let exact = "y".repeat(limit);
        let res = espacio_root_call(
            &st,
            "espacio_channel_create",
            json!({"space_id": "esp_limit", "name": exact}),
        )
        .await
        .expect("tool result");
        assert_eq!(res["isError"], false, "{res}");
        assert_eq!(res["structuredContent"]["channel_name"], exact);
        assert_eq!(res["structuredContent"]["message"]["content"], exact);

        let list = espacio_root_call(
            &st,
            "espacio_channel_list",
            json!({"space_id": "esp_limit"}),
        )
        .await
        .expect("list");
        assert_eq!(list["structuredContent"]["count"], 1, "{list}");
    }

    #[tokio::test]
    async fn channel_post_description_matches_behaviour() {
        let tool = get_xavier_core_tools()
            .into_iter()
            .find(|t| t.name == "espacio_channel_create")
            .expect("tool listed");
        assert!(
            tool.description.contains("Post a message"),
            "{}",
            tool.description
        );
        assert!(
            tool.description.contains("not deduplicated"),
            "{}",
            tool.description
        );

        let (_tmp, st) = espacio_fixture("esp_post").await;
        let mut seqs = Vec::new();
        for _ in 0..2 {
            let res = espacio_root_call(
                &st,
                "espacio_channel_create",
                json!({"space_id": "esp_post", "name": "same"}),
            )
            .await
            .expect("post");
            assert_eq!(res["isError"], false, "{res}");
            seqs.push(res["structuredContent"]["message"]["seq"].as_u64().unwrap());
        }
        assert_eq!(seqs, vec![0, 1], "each call appends a message");
        let list = espacio_root_call(&st, "espacio_channel_list", json!({"space_id": "esp_post"}))
            .await
            .expect("list");
        assert_eq!(list["structuredContent"]["count"], 2, "{list}");
    }

    #[tokio::test]
    async fn sync_gitcore_rejects_path_outside_allowed_roots() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let claims = admin_claims();
        let outside_dir = tempfile::tempdir().expect("tempdir");
        let outside_path = outside_dir.path().to_string_lossy().into_owned();

        let result = handle_core_tool(
            state,
            workspace,
            Some(&claims),
            "sync_gitcore",
            json!({
                "project_path": outside_path
            }),
        )
        .await;

        let err = result.expect_err("sync_gitcore must reject path outside allowed roots");
        assert!(
            err.to_string().contains("outside allowed roots")
                || err.to_string().contains("allowed roots"),
            "error should indicate outside allowed roots, got: {err}"
        );
    }

    #[tokio::test]
    async fn sync_gitcore_applies_write_clearance() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let ws_root = workspace
            .workspace
            .usage_state_path
            .parent()
            .expect("workspace parent")
            .to_path_buf();

        // The workspace root must be a git repository to be an allowed gitcore root.
        std::fs::create_dir_all(ws_root.join(".git")).expect("mark workspace root as git repo");
        let project_dir = ws_root.join("sample_gitcore_proj");
        std::fs::create_dir_all(&project_dir).expect("create sample project dir");
        std::fs::write(
            project_dir.join("README.md"),
            "# Sample Project\nDocumentation content for gitcore.",
        )
        .expect("write README");

        // 1. Caller with Unclassified clearance (below default Internal clearance) must be rejected
        let caller_low = crate::server::mcp::tools_memory::McpCaller {
            clearance: crate::security::clearance::ClearanceLevel::Unclassified,
            space: None,
        };
        let err = crate::server::mcp::tools_memory::with_mcp_caller(
            caller_low,
            handle_core_tool(
                state.clone(),
                workspace.clone(),
                None,
                "sync_gitcore",
                json!({
                    "project_path": project_dir.to_string_lossy().to_string()
                }),
            ),
        )
        .await
        .expect_err("sync_gitcore must reject when caller clearance is below document clearance");

        assert!(
            err.to_string().contains("clearance"),
            "error message must mention clearance, got: {err}"
        );

        // 2. Caller with Admin / TopSecret clearance must succeed
        let caller_high = crate::server::mcp::tools_memory::McpCaller {
            clearance: crate::security::clearance::ClearanceLevel::TopSecret,
            space: None,
        };
        let res = crate::server::mcp::tools_memory::with_mcp_caller(
            caller_high,
            handle_core_tool(
                state,
                workspace,
                None,
                "sync_gitcore",
                json!({
                    "project_path": project_dir.to_string_lossy().to_string()
                }),
            ),
        )
        .await
        .expect("sync_gitcore must succeed when caller clearance is sufficient");

        assert!(res.to_string().contains("created=1") || res.to_string().contains("Synced"));
    }

    #[tokio::test]
    async fn member_tools_are_root_only() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;

        // Ensure the manager exists for the node
        use crate::espacio::{SpaceManager, StaticNodeKek};
        let tmp = tempfile::tempdir().expect("tempdir");
        let kek = || std::sync::Arc::new(StaticNodeKek::new([7u8; 32]));
        let manager = std::sync::Arc::new(SpaceManager::open_with_keys(tmp.path(), kek()));
        crate::adapters::inbound::http::routes::init_space_manager(manager.clone());

        let ro = crate::security::auth::Claims::new(
            "u".into(),
            "u@swal.dev".into(),
            crate::security::auth::UserRole::Readonly,
            chrono::Duration::hours(1),
        );

        for name in [
            "espacio_member_list",
            "espacio_member_add",
            "espacio_member_remove",
        ] {
            let call_args = if name == "espacio_member_add" {
                json!({"space_id": "esp_mcp", "node_id": "node", "role": "reader"})
            } else if name == "espacio_member_remove" {
                json!({"space_id": "esp_mcp", "node_id": "node"})
            } else {
                json!({"space_id": "esp_mcp"})
            };

            let err = crate::server::mcp::server::with_root_credential(
                false,
                handle_core_tool(state.clone(), workspace.clone(), Some(&ro), name, call_args),
            )
            .await
            .expect_err("non-root must be refused");
            assert!(err.to_string().contains("Forbidden"), "{err}");
        }
    }

    #[tokio::test]
    async fn member_add_tool_reports_cap_as_error() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        use crate::espacio::{SpaceManager, StaticNodeKek};
        let tmp = tempfile::tempdir().expect("tempdir");
        let kek = || std::sync::Arc::new(StaticNodeKek::new([7u8; 32]));
        let manager = std::sync::Arc::new(SpaceManager::open_with_keys(tmp.path(), kek()));
        crate::adapters::inbound::http::routes::init_space_manager(manager.clone());

        use crate::workspace::protocol::SpaceProtocol;
        fn with_protocol(protocol: SpaceProtocol) -> crate::espacio::manager::KeyOptions {
            crate::espacio::manager::KeyOptions {
                protocol: Some(protocol),
                ..Default::default()
            }
        }

        let p = SpaceProtocol {
            members_cap: 1,
            ..Default::default()
        };

        manager
            .create_with_keys(
                "esp_cap".into(),
                "app".into(),
                "".into(),
                "owner".into(),
                false,
                with_protocol(p),
            )
            .await
            .unwrap();

        let root = root_claims();
        let res = crate::server::mcp::server::with_root_credential(
            true,
            handle_core_tool(
                state.clone(),
                workspace.clone(),
                Some(&root),
                "espacio_member_add",
                json!({"space_id": "esp_cap", "node_id": "intruder", "role": "reader"}),
            ),
        )
        .await
        .unwrap();

        assert_eq!(res["isError"], true, "{res}");
        assert!(res["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("reached members_cap"));
    }

    #[tokio::test]
    async fn member_list_tool_returns_added_member() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        use crate::espacio::{SpaceManager, StaticNodeKek};
        let tmp = tempfile::tempdir().expect("tempdir");
        let kek = || std::sync::Arc::new(StaticNodeKek::new([7u8; 32]));
        let manager = std::sync::Arc::new(SpaceManager::open_with_keys(tmp.path(), kek()));
        crate::adapters::inbound::http::routes::init_space_manager(manager.clone());

        manager
            .create(
                "esp_list".into(),
                "app".into(),
                "".into(),
                "owner".into(),
                false,
            )
            .await
            .unwrap();

        let root = root_claims();

        // Add
        let res_add = crate::server::mcp::server::with_root_credential(
            true,
            handle_core_tool(
                state.clone(),
                workspace.clone(),
                Some(&root),
                "espacio_member_add",
                json!({"space_id": "esp_list", "node_id": "friend", "role": "member"}),
            ),
        )
        .await
        .unwrap();
        assert_eq!(res_add["isError"], false);

        // List
        let res_list = crate::server::mcp::server::with_root_credential(
            true,
            handle_core_tool(
                state.clone(),
                workspace.clone(),
                Some(&root),
                "espacio_member_list",
                json!({"space_id": "esp_list"}),
            ),
        )
        .await
        .unwrap();

        let members = res_list["structuredContent"]["members"].as_array().unwrap();
        // owner + friend = 2
        assert_eq!(members.len(), 2);

        let friend = members.iter().find(|m| m["node_id"] == "friend").unwrap();
        assert_eq!(friend["role"], "member");

        // Remove
        crate::server::mcp::server::with_root_credential(
            true,
            handle_core_tool(
                state.clone(),
                workspace.clone(),
                Some(&root),
                "espacio_member_remove",
                json!({"space_id": "esp_list", "node_id": "friend"}),
            ),
        )
        .await
        .unwrap();

        // List again
        let res_list2 = crate::server::mcp::server::with_root_credential(
            true,
            handle_core_tool(
                state.clone(),
                workspace.clone(),
                Some(&root),
                "espacio_member_list",
                json!({"space_id": "esp_list"}),
            ),
        )
        .await
        .unwrap();
        assert_eq!(
            res_list2["structuredContent"]["members"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn codegraph_gods_excludes_variables_and_std_names() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let claims = admin_claims();

        let mk_sym = |name: &str, file: &str, kind: code_graph::types::SymbolKind| {
            let mut s = code_graph::types::Symbol {
                name: name.to_string(),
                kind,
                lang: code_graph::types::Language::Rust,
                file_path: file.to_string(),
                start_line: 1,
                end_line: 10,
                ..Default::default()
            };
            s.stable_id = Some(format!("{file}:{name}"));
            s
        };

        let var_sym = mk_sym(
            "no_string",
            "/src/sec.rs",
            code_graph::types::SymbolKind::Variable,
        );
        let import_sym = mk_sym(
            "MyImport",
            "/src/lib.rs",
            code_graph::types::SymbolKind::Import,
        );
        let join_sym = mk_sym(
            "join",
            "/src/str.rs",
            code_graph::types::SymbolKind::Function,
        );
        let len_sym = mk_sym(
            "len",
            "/src/vec.rs",
            code_graph::types::SymbolKind::Function,
        );
        let real_god = mk_sym(
            "RealGodComponent",
            "/src/core.rs",
            code_graph::types::SymbolKind::Struct,
        );

        state.code_db.insert_symbol(&var_sym).expect("insert var");
        state
            .code_db
            .insert_symbol(&import_sym)
            .expect("insert import");
        state.code_db.insert_symbol(&join_sym).expect("insert join");
        state.code_db.insert_symbol(&len_sym).expect("insert len");
        state
            .code_db
            .insert_symbol(&real_god)
            .expect("insert real_god");

        for i in 0..5 {
            let caller = mk_sym(
                &format!("caller_{i}"),
                &format!("/src/caller_{i}.rs"),
                code_graph::types::SymbolKind::Function,
            );
            state.code_db.insert_symbol(&caller).expect("insert caller");
            for target in [
                "/src/sec.rs:no_string",
                "/src/lib.rs:MyImport",
                "/src/str.rs:join",
                "/src/vec.rs:len",
                "/src/core.rs:RealGodComponent",
            ] {
                state
                    .code_db
                    .insert_edge(&code_graph::types::CodeEdge {
                        id: None,
                        from_symbol: format!("/src/caller_{i}.rs:caller_{i}"),
                        to_symbol: target.to_string(),
                        edge_type: code_graph::types::EdgeType::Calls,
                        file_path: format!("/src/caller_{i}.rs"),
                        line: 1,
                        confidence: 1.0,
                        metadata: None,
                    })
                    .expect("insert edge");
            }
        }

        let output = handle_core_tool(
            state,
            workspace,
            Some(&claims),
            "codegraph_gods",
            json!({"limit": 10}),
        )
        .await
        .expect("codegraph_gods succeeds");

        let god_nodes = output["structuredContent"]["god_nodes"]
            .as_array()
            .expect("god_nodes array");
        let names: Vec<&str> = god_nodes
            .iter()
            .filter_map(|n| n["symbol"]["name"].as_str())
            .collect();

        assert!(
            names.contains(&"RealGodComponent"),
            "Real god node must be included"
        );
        assert!(
            !names.contains(&"no_string"),
            "Variable kind must be excluded"
        );
        assert!(!names.contains(&"MyImport"), "Import kind must be excluded");
        assert!(
            !names.contains(&"join"),
            "Common std method 'join' must be excluded"
        );
        assert!(
            !names.contains(&"len"),
            "Common std method 'len' must be excluded"
        );
    }

    #[tokio::test]
    async fn trace_path_rejects_unknown_edge_type() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let claims = admin_claims();

        let err = handle_core_tool(
            state,
            workspace,
            Some(&claims),
            "trace_path",
            json!({
                "symbol": "handle_tool_call",
                "edge_type": "invalid_unknown_edge"
            }),
        )
        .await
        .expect_err("unknown edge_type must be rejected");

        let err_msg = err.to_string();
        assert!(
            err_msg.contains("Unknown edge_type: 'invalid_unknown_edge'"),
            "Error should mention unknown edge_type, got: {err_msg}"
        );
        assert!(
            err_msg.contains("calls") && err_msg.contains("defines"),
            "Error should list valid edge types, got: {err_msg}"
        );
    }

    #[tokio::test]
    async fn get_code_graph_no_absolute_path() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let claims = admin_claims();

        let output = handle_core_tool(state, workspace, Some(&claims), "get_code_graph", json!({}))
            .await
            .expect("get_code_graph succeeds");

        let dump_path = output["structuredContent"]["dump_path"]
            .as_str()
            .expect("dump_path in structuredContent");

        assert!(
            !std::path::Path::new(dump_path).is_absolute(),
            "dump_path must not be absolute: {dump_path}"
        );
        assert!(
            !dump_path.starts_with('/'),
            "dump_path must not start with /: {dump_path}"
        );
    }

    #[tokio::test]
    async fn codegraph_wrong_type_message() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let claims = admin_claims();

        // 1. Wrong type for query in explore: number instead of string
        let err_explore_type = handle_core_tool(
            state.clone(),
            workspace.clone(),
            Some(&claims),
            "codegraph_explore",
            json!({"query": 123}),
        )
        .await
        .expect_err("explore with number query must fail");
        assert!(
            err_explore_type
                .to_string()
                .contains("'query' must be a string"),
            "Expected 'query' must be a string, got: {err_explore_type}"
        );

        // 2. Missing query in explore
        let err_explore_missing = handle_core_tool(
            state.clone(),
            workspace.clone(),
            Some(&claims),
            "codegraph_explore",
            json!({}),
        )
        .await
        .expect_err("explore without query must fail");
        assert!(
            err_explore_missing.to_string().contains("Missing 'query'"),
            "Expected Missing 'query', got: {err_explore_missing}"
        );

        // 3. Wrong type for query in route
        let err_route_type = handle_core_tool(
            state.clone(),
            workspace.clone(),
            Some(&claims),
            "codegraph_route",
            json!({"query": 123}),
        )
        .await
        .expect_err("route with number query must fail");
        assert!(
            err_route_type
                .to_string()
                .contains("'query' must be a string"),
            "Expected 'query' must be a string, got: {err_route_type}"
        );

        // 4. Wrong type for symbol in trace_path
        let err_trace_type = handle_core_tool(
            state.clone(),
            workspace.clone(),
            Some(&claims),
            "trace_path",
            json!({"symbol": 123}),
        )
        .await
        .expect_err("trace with number symbol must fail");
        assert!(
            err_trace_type
                .to_string()
                .contains("'symbol' must be a string"),
            "Expected 'symbol' must be a string, got: {err_trace_type}"
        );

        // 5. Limit 0 is honoured for explore and route
        let out_explore_zero = handle_core_tool(
            state.clone(),
            workspace.clone(),
            Some(&claims),
            "codegraph_explore",
            json!({"query": "test", "limit": 0}),
        )
        .await
        .expect("explore with limit 0 must succeed");
        assert_eq!(out_explore_zero["structuredContent"]["returned"], 0);
        assert_eq!(
            out_explore_zero["structuredContent"]["symbols"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        let out_route_zero = handle_core_tool(
            state.clone(),
            workspace.clone(),
            Some(&claims),
            "codegraph_route",
            json!({"query": "test", "limit": 0}),
        )
        .await
        .expect("route with limit 0 must succeed");
        assert_eq!(
            out_route_zero["structuredContent"]["hits"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    async fn cg_call(
        state: &AppState,
        workspace: &WorkspaceContext,
        tool: &str,
        args: Value,
    ) -> anyhow::Result<Value> {
        let claims = admin_claims();
        handle_core_tool(state.clone(), workspace.clone(), Some(&claims), tool, args).await
    }

    fn cg_sym(name: &str, file: &str) -> code_graph::types::Symbol {
        code_graph::types::Symbol {
            name: name.to_string(),
            kind: code_graph::types::SymbolKind::Function,
            lang: code_graph::types::Language::Rust,
            file_path: file.to_string(),
            start_line: 1,
            end_line: 5,
            stable_id: Some(format!("{file}:{name}")),
            ..Default::default()
        }
    }

    fn cg_edge(
        from: &str,
        to: &str,
        ty: code_graph::types::EdgeType,
    ) -> code_graph::types::CodeEdge {
        code_graph::types::CodeEdge {
            id: None,
            from_symbol: from.to_string(),
            to_symbol: to.to_string(),
            edge_type: ty,
            file_path: "/src/a.rs".to_string(),
            line: 1,
            confidence: 1.0,
            metadata: None,
        }
    }

    #[tokio::test]
    async fn codegraph_route_unknown_symbol_is_refusal_result() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        for q in ["NoSuchSymbolAnywhere", "where is the auth handler defined"] {
            let out = cg_call(&state, &workspace, "codegraph_route", json!({"query": q}))
                .await
                .expect("unknown symbol must not be an error");
            let sc = &out["structuredContent"];
            assert_eq!(sc["refusal"], true);
            assert_eq!(sc["confident"], false);
            assert!(sc["hits"].as_array().unwrap().is_empty());
            assert!(sc["reason"].as_str().is_some_and(|r| !r.is_empty()));
        }
        let blank = cg_call(
            &state,
            &workspace,
            "codegraph_route",
            json!({"query": "  "}),
        )
        .await
        .expect("blank route ok");
        assert_eq!(blank["structuredContent"]["refusal"], true);
        assert!(blank["structuredContent"]["hits"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn codegraph_negative_limit_message_and_blank_explore() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        state
            .code_db
            .insert_symbol(&cg_sym("arbitrary_row", "/src/z.rs"))
            .unwrap();
        for (tool, args) in [
            ("codegraph_explore", json!({"query": "x", "limit": -1})),
            ("codegraph_gods", json!({"limit": -3})),
            ("codegraph_route", json!({"query": "x", "limit": -1})),
        ] {
            let err = cg_call(&state, &workspace, tool, args)
                .await
                .expect_err("negative limit rejected");
            assert!(
                err.to_string()
                    .contains("'limit' must be a non-negative integer"),
                "{tool}: {err}"
            );
        }
        let out = cg_call(
            &state,
            &workspace,
            "codegraph_explore",
            json!({"query": ""}),
        )
        .await
        .expect("blank explore ok");
        let sc = &out["structuredContent"];
        assert_eq!(sc["returned"], 0);
        assert!(sc["hint"].as_str().is_some());
    }

    #[tokio::test]
    async fn trace_path_names_truncated_found_and_structural_filter() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let db = &state.code_db;
        for (n, f) in [
            ("tp_root", "/src/r.rs"),
            ("tp_a", "/src/a.rs"),
            ("tp_b", "/src/b.rs"),
        ] {
            db.insert_symbol(&cg_sym(n, f)).unwrap();
        }
        use code_graph::types::EdgeType::{Calls, Contains};
        db.insert_edge(&cg_edge("/src/r.rs:tp_root", "/src/a.rs:tp_a", Calls))
            .unwrap();
        db.insert_edge(&cg_edge("/src/r.rs:tp_root", "/src/b.rs:tp_b", Calls))
            .unwrap();
        db.insert_edge(&cg_edge("file:/src/a.rs", "/src/a.rs:tp_a", Contains))
            .unwrap();
        db.insert_edge(&cg_edge("/src/r.rs:tp_root", "/src/a.rs:tp_a", Calls))
            .ok();

        // Names resolved, no truncation.
        let out = cg_call(
            &state,
            &workspace,
            "trace_path",
            json!({"symbol": "tp_root"}),
        )
        .await
        .unwrap();
        let sc = &out["structuredContent"];
        assert_eq!(sc["found"], true);
        assert!(sc.get("truncated").is_none());
        let edges = sc["edges"].as_array().unwrap();
        assert!(edges
            .iter()
            .any(|e| e["from_name"] == "tp_root" && e["to_name"] == "tp_a"));

        // Limit cut flagged.
        let out = cg_call(
            &state,
            &workspace,
            "trace_path",
            json!({"symbol": "tp_root", "limit": 1}),
        )
        .await
        .unwrap();
        assert_eq!(out["structuredContent"]["truncated"], true);
        assert_eq!(
            out["structuredContent"]["edges"].as_array().unwrap().len(),
            1
        );

        // Unknown symbol is distinguishable from a leaf.
        let out = cg_call(
            &state,
            &workspace,
            "trace_path",
            json!({"symbol": "tp_nope_zz"}),
        )
        .await
        .unwrap();
        assert_eq!(out["structuredContent"]["found"], false);
        assert!(out["structuredContent"]["hint"].as_str().is_some());
        let leaf = cg_call(&state, &workspace, "trace_path", json!({"symbol": "tp_b"}))
            .await
            .unwrap();
        assert_eq!(leaf["structuredContent"]["found"], true);

        // Reverse trace drops Contains/Defines unless asked for.
        let out = cg_call(
            &state,
            &workspace,
            "trace_path",
            json!({"symbol": "tp_a", "reverse": true}),
        )
        .await
        .unwrap();
        let edges = out["structuredContent"]["edges"].as_array().unwrap();
        assert!(!edges.is_empty());
        assert!(
            edges.iter().all(|e| e["edge_type"] != "Contains"),
            "{edges:?}"
        );
        let out = cg_call(
            &state,
            &workspace,
            "trace_path",
            json!({"symbol": "tp_a", "reverse": true, "edge_type": "contains"}),
        )
        .await
        .unwrap();
        assert!(!out["structuredContent"]["edges"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
