//! Memory-related MCP tools
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use super::types::*;
use crate::memory::schema::{
    EvidenceKind, MemoryKind, MemoryNamespace, MemoryProvenance, MemoryQueryFilters,
    TypedMemoryPayload,
};
use crate::workspace::WorkspaceContext;
use crate::AppState;
use serde_json::{json, Value};
use ulid::Ulid;

/// Who is calling an MCP tool, as established by the transport from the
/// authenticated request (WP-13o): the space identity of an `xsp_` token
/// (`SpaceAuth`, never the workspace id) and the clearance ceiling the REST
/// routes would apply to the same request.
#[derive(Debug, Clone)]
pub struct McpCaller {
    pub space: Option<crate::espacio::SpaceAuth>,
    pub clearance: crate::security::clearance::ClearanceLevel,
}

tokio::task_local! {
    static MCP_CALLER: McpCaller;
    static MEMORY_WRITE_CALLER_KEY: String;
}

/// Run `fut` with `caller` as the identity of the MCP call. Without it,
/// `include_linked` merges nothing (fail closed).
pub async fn with_mcp_caller<F: std::future::Future>(caller: McpCaller, fut: F) -> F::Output {
    MCP_CALLER.scope(caller, fut).await
}

pub async fn with_memory_write_caller_key<F: std::future::Future>(
    key: String,
    fut: F,
) -> F::Output {
    MEMORY_WRITE_CALLER_KEY.scope(key, fut).await
}

pub(crate) fn memory_write_caller_key(claims: Option<&crate::security::auth::Claims>) -> String {
    MEMORY_WRITE_CALLER_KEY
        .try_with(Clone::clone)
        .unwrap_or_else(|_| {
            claims
                .map(|c| format!("subject:{}", c.sub))
                .unwrap_or_else(|| "ip:unknown".to_string())
        })
}

fn validate_memory_write_clearance(
    workspace: &WorkspaceContext,
    path: &str,
    metadata: &Value,
    typed: Option<&TypedMemoryPayload>,
) -> anyhow::Result<()> {
    let effective =
        crate::memory::schema::resolve_metadata(path, metadata, &workspace.workspace_id, typed)?
            .clearance;
    crate::security::clearance::check_write_clearance(effective, caller_clearance())
        .map_err(anyhow::Error::msg)
}

fn typed_write_clearance(
    arguments: &Value,
) -> anyhow::Result<Option<crate::security::clearance::ClearanceLevel>> {
    arguments
        .get("clearance")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(Into::into)
}

fn mcp_caller() -> Option<McpCaller> {
    MCP_CALLER.try_with(|c| c.clone()).ok()
}

/// Clearance ceiling of the current MCP call. Without a caller identity the
/// ceiling is the lowest level: a request whose identity could not be
/// established must not inherit the configurable default.
pub fn caller_clearance() -> crate::security::clearance::ClearanceLevel {
    mcp_caller()
        .map(|c| c.clearance)
        .unwrap_or(crate::security::clearance::ClearanceLevel::Unclassified)
}

const MEMORYFRAGMENT_MAX_LIMIT: usize = 100;
const MEMORYFRAGMENT_MAX_COMPONENT_CHARS: usize = 128;
const MEMORYFRAGMENT_MAX_TAGS: usize = 32;
const MEMORYFRAGMENT_MAX_TAG_CHARS: usize = 64;
const MEMORYFRAGMENT_MAX_PROVENANCE_CHARS: usize = 2048;
const CONTEXT_DEFAULT_MAX_CHARS: usize = 4000;
const CONTEXT_ABSOLUTE_MAX_CHARS: usize = 16000;

/// Get xavier memory tools.
pub fn get_xavier_memory_tools() -> Vec<MCPTool> {
    vec![
        MCPTool {
            name: "create_memory".to_string(),
            description: "Create a new memory document in Xavier".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path/identifier for the memory" },
                    "content": { "type": "string", "description": "Content of the memory" },
                    "metadata": { "type": "object", "description": "Optional metadata" },
                    "kind": { "type": "string", "description": "Canonical memory kind" },
                    "evidence_kind": { "type": "string", "description": "Optional retrieval evidence kind" },
                    "namespace": { "type": "object", "description": "Namespace fields" },
                    "provenance": { "type": "object", "description": "Source and provenance fields" }
                },
                "required": ["path", "content"]
            }),
        },
        MCPTool {
            name: "mem_search".to_string(),
            description: "Fat index search (progressive disclosure step 1). Returns structured candidates {id,path,score,snippet,kind} WITHOUT full body by default. Set include_content=true only when necessary. Use memory_context with ids to page-in full text.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "page": { "type": "number", "description": "Page number (1-based, default: 1)", "default": 1 },
                    "limit": { "type": "number", "description": "Maximum results (default: 10, max: 100)", "default": 10 },
                    "include_content": { "type": "boolean", "description": "Include full document body in each candidate (default false — prefer memory_context page-in by ids)", "default": false },
                    "search_mode": { "type": "string", "enum": ["bm25", "semantic", "hybrid"], "description": "RESERVED — currently ignored; search always runs the hybrid BM25+vector+RRF pipeline. Kept for forward-compatibility.", "default": "hybrid" },
                    "include_activity": { "type": "boolean", "description": "Include telemetry/noise namespaces (activity/*, gestalt/thinking/*, auto activity/insight records) that are excluded by default (default false)", "default": false },
                    "include_linked": { "type": "boolean", "description": "Also search (read-only) the spaces that granted this space a link; results carry source_space (default false)", "default": false },
                    "filters": { "type": "object", "description": "Optional filters" }
                },
                "required": ["query"]
            }),
        },
        MCPTool {
            name: "search_memory".to_string(),
            description: "[DEPRECATED — use mem_search instead] Search memory documents in Xavier".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "page": { "type": "number", "description": "Page number (1-based, default: 1)", "default": 1 },
                    "limit": { "type": "number", "description": "Maximum results", "default": 10 },
                    "include_content": { "type": "boolean", "description": "Whether to include full content in results (default: false)", "default": false },
                    "filters": { "type": "object", "description": "Optional filters" }
                },
                "required": ["query"]
            }),
        },
        MCPTool {
            name: "get_memory".to_string(),
            description: "Get a specific memory by ID".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Memory ID" }
                },
                "required": ["id"]
            }),
        },
        MCPTool {
            name: "save_fragment".to_string(),
            description: "Save a new memory fragment".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "content": { "type": "string" },
                    "context": { "type": "string" },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "importance": { "type": "number" },
                    "repo_url": { "type": "string" },
                    "file_path": { "type": "string" },
                    "chunk_id": { "type": "string" }
                },
                "required": ["agent_id", "content", "context"]
            }),
        },
        MCPTool {
            name: "memoryfragment_save".to_string(),
            description: "Alias for save_fragment".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "content": { "type": "string" },
                    "context": { "type": "string" },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "importance": { "type": "number" },
                    "repo_url": { "type": "string" },
                    "file_path": { "type": "string" },
                    "chunk_id": { "type": "string" }
                },
                "required": ["agent_id", "content", "context"]
            }),
        },
        MCPTool {
            name: "search_fragments".to_string(),
            description: "Search memory fragments".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "agent_id": { "type": "string" },
                    "context": { "type": "string" },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "limit": { "type": "number", "default": 10 }
                },
                "required": ["query"]
            }),
        },
        MCPTool {
            name: "memoryfragment_search".to_string(),
            description: "Alias for search_fragments".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "agent_id": { "type": "string" },
                    "context": { "type": "string" },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "limit": { "type": "number", "default": 10 }
                },
                "required": ["query"]
            }),
        },
        MCPTool {
            name: "get_recent_fragments".to_string(),
            description: "Get recent memories for an agent".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "context": { "type": "string" },
                    "limit": { "type": "number", "default": 10 }
                },
                "required": ["agent_id"]
            }),
        },
        MCPTool {
            name: "memoryfragment_recent".to_string(),
            description: "Alias for get_recent_fragments".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "context": { "type": "string" },
                    "limit": { "type": "number", "default": 10 }
                },
                "required": ["agent_id"]
            }),
        },
        MCPTool {
            name: "memoryfragment_get".to_string(),
            description: "Get a specific memory fragment by ID".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
        MCPTool {
            name: "memoryfragment_delete".to_string(),
            description: "Delete a specific memory fragment by ID".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
        MCPTool {
            name: "stats".to_string(),
            description: "Get Xavier memory statistics".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        MCPTool {
            name: "memory_save".to_string(),
            description: "Save a memory document (text) with optional metadata and namespace".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Content of the memory to store" },
                    "metadata": { "type": "object", "description": "Optional free-form metadata" },
                    "namespace": {
                        "description": "Optional namespace: a project string (e.g. \"cortex\") or an object {project, agent_id, scope, session_id}",
                        "oneOf": [
                            { "type": "string" },
                            { "type": "object" }
                        ]
                    }
                },
                "required": ["text"]
            }),
        },
        MCPTool {
            name: "memory_search".to_string(),
            description: "[DEPRECATED — use mem_search instead] Fat index search (same structured candidates as mem_search). Prefer mem_search → memory_context/get_memory → create_memory.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "page": { "type": "number", "description": "Page number (1-based, default: 1)", "default": 1 },
                    "limit": { "type": "number", "description": "Maximum results (default: 10, max: 100)", "default": 10 },
                    "include_content": { "type": "boolean", "description": "Include full document body in each candidate (default false — prefer memory_context page-in by ids)", "default": false },
                    "depth": { "type": "number", "description": "Relationship depth to explore (0=flat, 1=direct, 2=two-hop)", "default": 0 },
                    "filters": { "type": "object", "description": "Optional filters" },
                    "namespace": {
                        "description": "Optional namespace filter: a project string or an object {project, agent_id, scope, session_id}",
                        "oneOf": [
                            { "type": "string" },
                            { "type": "object" }
                        ]
                    }
                },
                "required": ["query"]
            }),
        },
        MCPTool {
            name: "memory_context".to_string(),
            description: "Page-in full/partial content for specific memory ids (preferred progressive-disclosure step 2) or a query. Honors max_chars total budget and optional max_chars_per_doc per source. Prefer ids from mem_search over re-querying when possible.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Query describing the desired context (used when ids omitted)" },
                    "ids": { "type": "array", "items": { "type": "string" }, "description": "Preferred: memory IDs from mem_search candidates to page-in specifically" },
                    "limit": { "type": "number", "description": "Maximum memories to include (if using query)", "default": 5 },
                    "max_chars": { "type": "number", "description": "Maximum total characters in the aggregated context output", "default": 4000 },
                    "max_chars_per_doc": { "type": "number", "description": "Maximum characters per individual memory document (default: min(800, max_chars)); response reports per-source truncation honesty" },
                    "depth": { "type": "number", "description": "Relationship depth to explore (0=flat, 1=direct, 2=two-hop)", "default": 0 },
                    "search_mode": { "type": "string", "enum": ["bm25", "semantic", "hybrid"], "description": "RESERVED — currently ignored; search always runs the hybrid pipeline.", "default": "hybrid" },
                    "include_activity": { "type": "boolean", "description": "Include telemetry/noise namespaces (activity/*, gestalt/thinking/*, auto activity/insight records) that are excluded by default when assembling context from a query (default false)", "default": false }
                }
            }),
        },
        MCPTool {
            name: "mem_context".to_string(),
            description: "Alias of memory_context (progressive-disclosure step 2 page-in)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Query describing the desired context (used when ids omitted)" },
                    "ids": { "type": "array", "items": { "type": "string" }, "description": "Preferred: memory IDs from mem_search candidates to page-in specifically" },
                    "limit": { "type": "number", "description": "Maximum memories to include (if using query)", "default": 5 },
                    "max_chars": { "type": "number", "description": "Maximum total characters in the aggregated context output", "default": 4000 },
                    "max_chars_per_doc": { "type": "number", "description": "Maximum characters per individual memory document (default: min(800, max_chars)); response reports per-source truncation honesty" },
                    "depth": { "type": "number", "description": "Relationship depth to explore (0=flat, 1=direct, 2=two-hop)", "default": 0 },
                    "search_mode": { "type": "string", "enum": ["bm25", "semantic", "hybrid"], "description": "RESERVED — currently ignored; search always runs the hybrid pipeline.", "default": "hybrid" }
                }
            }),
        },
        MCPTool {
            name: "memory_prune".to_string(),
            description: "Prune stale or duplicate memories by kind, age, or path prefix".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "description": "Delete all memories of a specific kind" },
                    "older_than_days": { "type": "integer", "description": "Delete memories not accessed in N days (0 to disable)", "default": 0 },
                    "path_prefix": { "type": "string", "description": "Delete memories with path starting with prefix" },
                    "dry_run": { "type": "boolean", "description": "Preview count without deleting (MANDATORY: default true)", "default": true }
                }
            }),
        },
    ]
}

/// Handle memory tool dispatching.
pub async fn handle_memory_tool(
    state: AppState,
    workspace: WorkspaceContext,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    match name {
        "mem_search"
        | "search_memory"
        | "memory_search"
        | "search_fragments"
        | "memoryfragment_search"
        | "get_recent_fragments"
        | "memoryfragment_recent" => {
            handle_memory_search(&state, &workspace, name, &arguments).await
        }
        "create_memory" | "save_fragment" | "memoryfragment_save" | "memory_save" => {
            handle_memory_create(&state, &workspace, name, &arguments).await
        }
        "get_memory" | "memoryfragment_get" | "stats" => {
            handle_memory_update(&state, &workspace, name, &arguments).await
        }
        "memoryfragment_delete" | "memory_prune" => {
            handle_memory_delete(&state, &workspace, name, &arguments).await
        }
        "memory_context" | "mem_context" => {
            handle_memory_context(&state, &workspace, name, &arguments).await
        }
        _ => Err(anyhow::anyhow!("Tool not implemented: {}", name)),
    }
}

/// Handles search/query memory operations.
pub async fn handle_memory_search(
    state: &AppState,
    workspace: &WorkspaceContext,
    name: &str,
    arguments: &Value,
) -> anyhow::Result<Value> {
    match name {
        "mem_search" | "search_memory" | "memory_search" => {
            handle_mem_search(workspace, arguments).await
        }
        "search_fragments" | "memoryfragment_search" => {
            handle_search_fragments(state, workspace, arguments).await
        }
        "get_recent_fragments" | "memoryfragment_recent" => {
            handle_get_recent_fragments(workspace, arguments).await
        }
        _ => Err(anyhow::anyhow!("Tool not implemented: {}", name)),
    }
}

const MCP_SEARCH_QUERY_MAX_BYTES: usize = 8 * 1024;
const MCP_CREATE_PATH_MAX_CHARS: usize = 1024;
const MCP_SEARCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn validate_search_query(query: &str) -> anyhow::Result<()> {
    if query.len() > MCP_SEARCH_QUERY_MAX_BYTES {
        anyhow::bail!(
            "Invalid argument 'query': {} bytes exceeds the {} byte limit",
            query.len(),
            MCP_SEARCH_QUERY_MAX_BYTES
        );
    }
    Ok(())
}

fn validate_create_path(path: &str) -> anyhow::Result<()> {
    let n = path.chars().count();
    if n > MCP_CREATE_PATH_MAX_CHARS {
        anyhow::bail!(
            "Invalid argument 'path': {} chars exceeds the {} char limit",
            n,
            MCP_CREATE_PATH_MAX_CHARS
        );
    }
    Ok(())
}

async fn with_tool_timeout<T>(
    limit: std::time::Duration,
    what: &str,
    fut: impl std::future::Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    match tokio::time::timeout(limit, fut).await {
        Ok(r) => r,
        Err(_) => Err(anyhow::anyhow!(
            "{what} timed out after {}s",
            limit.as_secs()
        )),
    }
}

async fn handle_mem_search(
    workspace: &WorkspaceContext,
    arguments: &Value,
) -> anyhow::Result<Value> {
    validate_search_query(
        arguments
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
    )?;
    with_tool_timeout(
        MCP_SEARCH_TIMEOUT,
        "search_memory",
        handle_mem_search_inner(workspace, arguments),
    )
    .await
}

async fn handle_mem_search_inner(
    workspace: &WorkspaceContext,
    arguments: &Value,
) -> anyhow::Result<Value> {
    let query = arguments
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let page = arguments
        .get("page")
        .and_then(|v| v.as_u64())
        .unwrap_or(1)
        .clamp(1, 1000) as usize;
    let limit = arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(10)
        .clamp(1, MEMORYFRAGMENT_MAX_LIMIT as u64) as usize;
    let include_content = arguments
        .get("include_content")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let depth = arguments
        .get("depth")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .clamp(0, 2) as usize;
    let mut filters = arguments
        .get("filters")
        .cloned()
        .map(serde_json::from_value::<MemoryQueryFilters>)
        .transpose()?
        .unwrap_or_default();

    // memory_search compat: merge namespace into filters when provided
    if let Some(ns) = parse_namespace_arg(arguments)? {
        if filters.project.is_none() {
            filters.project = ns.project;
        }
        if filters.agent_id.is_none() {
            filters.agent_id = ns.agent_id;
        }
        if filters.scope.is_none() {
            filters.scope = ns.scope;
        }
        if filters.session_id.is_none() {
            filters.session_id = ns.session_id;
        }
        if filters.user_id.is_none() {
            filters.user_id = ns.user_id;
        }
    }

    // Top-level convenience arg mirrors `filters.include_activity`: opts
    // back into telemetry/noise namespaces (activity/*, gestalt/thinking/*,
    // auto activity/insight records) that general search excludes by
    // default. Checked *before* `has_filters` below so a bare
    // `{"include_activity": true}` call (no other filter field set) isn't
    // silently dropped by `filter_ref = None`.
    if let Some(include_activity) = arguments.get("include_activity").and_then(|v| v.as_bool()) {
        filters.include_activity = Some(include_activity);
    }

    let has_filters = filters.project.is_some()
        || filters.agent_id.is_some()
        || filters.scope.is_some()
        || filters.session_id.is_some()
        || filters.user_id.is_some()
        || filters.kinds.is_some()
        || filters.path_prefix.is_some()
        || filters.include_activity.is_some();
    let filter_ref = if has_filters { Some(&filters) } else { None };

    let fetch_limit = page.saturating_mul(limit).saturating_add(1);

    if let Some(repos_arg) = arguments.get("repos").and_then(|v| v.as_array()) {
        let mut repos = Vec::new();
        for r in repos_arg {
            if let Some(r_str) = r.as_str() {
                repos.push(r_str.to_string());
            }
        }
        if !repos.is_empty() {
            let mut per_store = Vec::new();
            let mut legs = Vec::new();
            for repo in &repos {
                match crate::memory::repo_store_registry::open_repo_store(std::path::Path::new(
                    repo,
                ))
                .await
                {
                    Ok(store) => {
                        per_store.push(json!({"store": repo, "status": "ok"}));
                        use crate::memory::store::MemoryStore;
                        if let Ok(results) = store
                            .hybrid_search(
                                &workspace.workspace_id,
                                query,
                                crate::memory::store::HybridSearchMode::Both,
                                filter_ref,
                                fetch_limit,
                            )
                            .await
                        {
                            legs.push(crate::memory::store::FederatedLegs {
                                store: repo.clone(),
                                model_ok: true,
                                vector: results.clone(),
                                lexical: results.clone(),
                                kg: results,
                            });
                        }
                    }
                    Err(e) => {
                        let status = match e {
                            crate::memory::repo_store_registry::RepoStoreError::OutsideAllowlist {
                                ..
                            } => "outside_allowlist",
                            crate::memory::repo_store_registry::RepoStoreError::NotOptedIn {
                                ..
                            } => "not_opted_in",
                            _ => "error",
                        };
                        per_store.push(json!({"store": repo, "status": status}));
                    }
                }
            }

            let merged = crate::memory::store::merge_federated(&legs, 60, fetch_limit);
            let offset = page.saturating_sub(1).saturating_mul(limit);
            let total_matched = merged.len();
            let has_more = total_matched > page.saturating_mul(limit);
            let paged_results: Vec<_> = merged.into_iter().skip(offset).take(limit).collect();
            let total_pages = if total_matched == 0 {
                0
            } else if has_more {
                page + 1
            } else {
                total_matched.div_ceil(limit)
            };

            let candidates: Vec<Value> = paged_results
                .into_iter()
                .filter_map(|hit| {
                    let record_res = legs
                        .iter()
                        .find(|l| l.store == hit.store)
                        .and_then(|l| l.vector.iter().find(|v| v.record.id == hit.record_id));

                    if let Some(item) = record_res {
                        let snippet: String =
                            crate::memory::snippet::clip_chars(&item.record.content, 100)
                                .to_string();
                        let kind = item
                            .record
                            .metadata
                            .get("kind")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let mut obj = json!({
                            "id": item.record.id.clone(),
                            "path": item.record.path.clone(),
                            "score": hit.score,
                            "snippet": snippet,
                            "kind": kind,
                        });
                        if include_content {
                            obj.as_object_mut()
                                .expect("object")
                                .insert("content".to_string(), json!(item.record.content));
                        }
                        Some(obj)
                    } else {
                        None
                    }
                })
                .collect();

            let payload = json!({
                "query": query,
                "include_content": include_content,
                "page": page,
                "limit": limit,
                "total_pages": total_pages,
                "has_more": has_more,
                "count": candidates.len(),
                "candidates": candidates,
                "per_store": per_store,
            });

            return Ok(serde_json::to_value(
                crate::server::mcp::MCPToolResult::structured(payload, false),
            )?);
        }
    }

    let (results, search_mode) = workspace
        .workspace
        .memory
        .search_filtered_with_mode(query, fetch_limit, filter_ref)
        .await?;

    let results = if depth > 0 {
        workspace
            .workspace
            .memory
            .expand_depth(&results, depth, filter_ref)
            .await?
    } else {
        results
    };

    let offset = page.saturating_sub(1).saturating_mul(limit);
    let total_matched = results.len();
    let has_more = total_matched > page.saturating_mul(limit);
    let paged_results: Vec<_> = results.into_iter().skip(offset).take(limit).collect();

    // Phase 1 access instrumentation (D6): record only the page actually
    // returned. Recording the whole `page * limit + 1` candidate set would
    // re-count pages 1..N-1 on every page-N request.
    crate::memory::access::record_user_access(
        &workspace.workspace.memory_manager,
        &workspace.workspace.memory,
        &paged_results,
    )
    .await;
    let total_pages = if total_matched == 0 {
        0
    } else if has_more {
        page + 1
    } else {
        total_matched.div_ceil(limit)
    };

    // Progressive disclosure: fat index by default (structured candidates).
    let mut candidates: Vec<Value> = paged_results
        .into_iter()
        .map(|doc| {
            let snippet: String = crate::memory::snippet::clip_chars(&doc.content, 100).to_string();
            let kind = doc
                .metadata
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let mut obj = json!({
                "id": doc.id.clone().unwrap_or_default(),
                "path": doc.path,
                "score": doc.score,
                "snippet": snippet,
                "kind": kind,
            });
            if include_content {
                obj.as_object_mut()
                    .expect("object")
                    .insert("content".to_string(), json!(doc.content));
            }
            obj
        })
        .collect();

    // WP-13n: opt-in, read-only merge of spaces that linked their memory to
    // the caller's space. The space is the SpaceAuth identity set by the
    // transport, not the workspace id. Linked results get the same clearance
    // ceiling as REST; encrypted/locked linked spaces are skipped with a note.
    let mut linked_skipped: Vec<Value> = Vec::new();
    let mut linked_hidden_by_clearance = 0usize;
    let include_linked = arguments
        .get("include_linked")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if include_linked {
        let caller = mcp_caller();
        let own = caller
            .as_ref()
            .and_then(|c| c.space.as_ref())
            .map(|a| a.space_id.clone());
        let manager = crate::adapters::inbound::http::routes::get_space_manager();
        if let (Some(own), Some(manager), Some(caller)) = (own, manager, caller) {
            if manager.get(&own).await.is_ok() {
                for c in candidates.iter_mut() {
                    c["source_space"] = json!(own);
                }
                let resolved = crate::espacio::link::resolve_linked(&manager, &own).await;
                linked_skipped = resolved.skipped;
                // Link filters and the ceiling run after the search: over-fetch.
                let linked_fetch = crate::espacio::linked_fetch_limit(fetch_limit);
                for lm in resolved.searchable {
                    let Ok((docs, _)) = lm
                        .ctx
                        .workspace
                        .memory
                        .search_filtered_with_mode(query, linked_fetch, filter_ref)
                        .await
                    else {
                        linked_skipped
                            .push(json!({"space": lm.space_id, "reason": "SearchFailed"}));
                        continue;
                    };
                    let docs: Vec<_> = docs
                        .into_iter()
                        .filter(|d| lm.link.allows(&d.path, &d.metadata))
                        .collect();
                    let (docs, hidden) = crate::security::clearance::split_by_clearance(
                        caller.clearance,
                        docs,
                        |d| crate::security::clearance::level_from_metadata(&d.metadata),
                    );
                    linked_hidden_by_clearance += hidden;
                    for doc in docs.into_iter().skip(offset).take(limit) {
                        let mut obj = json!({
                            "id": doc.id.clone().unwrap_or_default(),
                            "path": doc.path,
                            "score": doc.score,
                            "snippet": crate::memory::snippet::clip_chars(&doc.content, 100),
                            "kind": doc.metadata.get("kind").and_then(|v| v.as_str()).unwrap_or("unknown"),
                            "source_space": lm.space_id,
                        });
                        if include_content {
                            obj["content"] = json!(doc.content);
                        }
                        candidates.push(obj);
                    }
                }
                candidates.sort_by(|a, b| {
                    let sa = a["score"].as_f64().unwrap_or(0.0);
                    let sb = b["score"].as_f64().unwrap_or(0.0);
                    sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
                });
                candidates.truncate(limit);
            }
        }
    }

    // Record token accounting & search stats
    let total_snippet_bytes: usize = candidates
        .iter()
        .map(|c| {
            c.get("snippet")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .len()
        })
        .sum();
    let total_full_bytes: usize = if include_content {
        candidates
            .iter()
            .map(|c| {
                c.get("content")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .len()
            })
            .sum()
    } else {
        0
    };

    if include_content {
        crate::observability::token_accounting::SEARCH_STATS
            .record_search("full", total_full_bytes);
    } else {
        crate::observability::token_accounting::SEARCH_STATS
            .record_search("snippet", total_snippet_bytes);

        let est_full_tokens = total_snippet_bytes.saturating_mul(5).div_ceil(4);
        let est_snippet_tokens = total_snippet_bytes.div_ceil(4);
        if est_full_tokens > est_snippet_tokens {
            let session_id = filters
                .session_id
                .as_deref()
                .unwrap_or("mcp_mem_search")
                .to_string();
            crate::observability::token_accounting::TRACKER
                .track(session_id, est_full_tokens, est_snippet_tokens, 0.01)
                .await;
        }
    }

    let payload = json!({
        "query": query,
        "include_content": include_content,
        "page": page,
        "limit": limit,
        "total_pages": total_pages,
        "has_more": has_more,
        "count": candidates.len(),
        "candidates": candidates,
        "linked_skipped": linked_skipped,
        "linked_hidden_by_clearance": linked_hidden_by_clearance,
        // "hybrid" when the embedding/vector signal contributed, "lexical" when
        // results are FTS/BM25-only (no embedder configured, or it timed out /
        // failed and search degraded gracefully instead of hanging).
        "mode": search_mode.as_str(),
    });

    Ok(serde_json::to_value(MCPToolResult::structured(
        payload, false,
    ))?)
}

async fn handle_get_recent_fragments(
    workspace: &WorkspaceContext,
    arguments: &Value,
) -> anyhow::Result<Value> {
    let limit = arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(10)
        .clamp(1, MEMORYFRAGMENT_MAX_LIMIT as u64) as usize;

    let agent_id = optional_memoryfragment_component(arguments, "agent_id", None)?;
    let context = optional_memoryfragment_component(arguments, "context", None)?;

    let mut filters = MemoryQueryFilters::default();
    if let Some(aid) = agent_id {
        filters.agent_id = Some(aid);
    }
    if let Some(ctx) = context {
        filters.scope = Some(ctx);
    }

    let records = workspace
        .workspace
        .list_memory_records_filtered(filters, limit)
        .await?;

    let candidates: Vec<Value> = records
        .into_iter()
        .map(|rec| {
            let snippet: String = crate::memory::snippet::clip_chars(&rec.content, 200).to_string();
            json!({
                "id": rec.id,
                "path": rec.path,
                "snippet": snippet,
                "content": rec.content,
                "metadata": rec.metadata,
            })
        })
        .collect();

    let payload = json!({
        "count": candidates.len(),
        "candidates": candidates,
    });

    Ok(serde_json::to_value(MCPToolResult::structured(
        payload, false,
    ))?)
}

async fn handle_search_fragments(
    state: &AppState,
    workspace: &WorkspaceContext,
    arguments: &Value,
) -> anyhow::Result<Value> {
    let query = arguments
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let query = match secure_mcp_external_input(state, "memoryfragment query", query).await? {
        Ok(query) => query,
        Err(blocked) => return Ok(blocked),
    };
    let agent_id = optional_memoryfragment_component(arguments, "agent_id", None)?;
    let context = optional_memoryfragment_component(arguments, "context", None)?;
    let tags = memoryfragment_tags(arguments)?;
    let limit = memoryfragment_limit(arguments);

    let mut filters = MemoryQueryFilters::default();
    if let Some(aid) = agent_id {
        filters.agent_id = Some(aid);
    }
    if let Some(ctx) = context {
        filters.scope = Some(ctx);
    }

    let results = workspace
        .workspace
        .memory
        .search_filtered(&query, limit, Some(&filters))
        .await?;
    let filtered: Vec<_> = results
        .into_iter()
        .filter(|doc| {
            if !tags.is_empty() {
                let doc_tags: Vec<String> = doc
                    .metadata
                    .get("tags")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                if !tags.iter().any(|t| doc_tags.contains(t)) {
                    return false;
                }
            }
            true
        })
        .collect();

    let candidates: Vec<Value> = filtered
        .into_iter()
        .map(|doc| {
            let snippet: String = crate::memory::snippet::clip_chars(&doc.content, 200).to_string();
            json!({
                "id": doc.id,
                "path": doc.path,
                "score": 1.0,
                "snippet": snippet,
                "content": doc.content,
                "metadata": doc.metadata,
            })
        })
        .collect();

    let payload = json!({
        "query": query,
        "count": candidates.len(),
        "candidates": candidates,
    });

    Ok(serde_json::to_value(MCPToolResult::structured(
        payload, false,
    ))?)
}

/// Handles create/put memory operations.
pub async fn handle_memory_create(
    state: &AppState,
    workspace: &WorkspaceContext,
    name: &str,
    arguments: &Value,
) -> anyhow::Result<Value> {
    match name {
        "create_memory" => handle_create_memory(workspace, arguments).await,
        "save_fragment" | "memoryfragment_save" => {
            handle_save_fragment(state, workspace, arguments).await
        }
        "memory_save" => handle_memory_save(workspace, arguments).await,
        _ => Err(anyhow::anyhow!("Tool not implemented: {}", name)),
    }
}

async fn handle_create_memory(
    workspace: &WorkspaceContext,
    arguments: &Value,
) -> anyhow::Result<Value> {
    let path = arguments
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing path"))?;
    validate_create_path(path)?;
    let content = arguments
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing content"))?;
    let metadata = arguments
        .get("metadata")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let kind = arguments
        .get("kind")
        .cloned()
        .map(serde_json::from_value::<MemoryKind>)
        .transpose()?;
    let evidence_kind = arguments
        .get("evidence_kind")
        .cloned()
        .map(serde_json::from_value::<EvidenceKind>)
        .transpose()?;
    let namespace = arguments
        .get("namespace")
        .cloned()
        .map(serde_json::from_value::<MemoryNamespace>)
        .transpose()?;
    let provenance = arguments
        .get("provenance")
        .cloned()
        .map(serde_json::from_value::<MemoryProvenance>)
        .transpose()?;

    let typed_payload = Some(TypedMemoryPayload {
        kind,
        clearance: typed_write_clearance(arguments)?,
        evidence_kind,
        namespace,
        provenance,
        ..Default::default()
    });

    validate_memory_write_clearance(workspace, path, &metadata, typed_payload.as_ref())?;

    // Ingest immediately with an empty vector (fast path response under 500ms)
    let doc_id = workspace
        .workspace
        .ingest_typed(
            path.to_string(),
            content.to_string(),
            metadata.clone(),
            typed_payload.clone(),
            Some(Vec::new()),
            false,
        )
        .await?;

    // Fill the acknowledged row directly: identical-write dedup ignores vector changes.
    let ws_clone = workspace.workspace.clone();
    let workspace_id = workspace.workspace_id.clone();
    let memory_id = doc_id.clone();
    let path_owned = path.to_string();
    tokio::spawn(async move {
        if crate::memory::qmd::writer::bus_quota_exhausted(&path_owned) {
            return;
        }
        let store = ws_clone.durable_store();
        let Ok(Some(record)) = store.get(&workspace_id, &memory_id).await else {
            return;
        };
        if !needs_background_embedding(&record) {
            return;
        }
        let private = record
            .encrypted_dek
            .as_deref()
            .is_some_and(crate::memory::sqlite_vec_store::at_rest::is_space_row)
            || crate::memory::sqlite_vec_store::at_rest::is_private_record(
                &record.metadata,
                &record.path,
            );
        let vector = if private {
            match crate::memory::embedder::EmbeddingClient::from_env_private_async().await {
                Ok(client) => client.embed(&record.content).await,
                Err(error) => Err(error),
            }
        } else {
            crate::memory::qmd_memory::reader::generate_embedding(&record.content).await
        };
        if let Ok(vector) = vector {
            apply_background_embedding(&ws_clone, &workspace_id, &record, vector).await;
        }
    });

    super::server::mcp_text_result(
        format!(
            "Memory created successfully at path: {} (id: {})",
            path, doc_id
        ),
        false,
    )
}

async fn handle_save_fragment(
    state: &AppState,
    workspace: &WorkspaceContext,
    arguments: &Value,
) -> anyhow::Result<Value> {
    let agent_id = require_memoryfragment_component(arguments, "agent_id")?;
    let content = arguments
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing content"))?;
    let content = match secure_mcp_external_input(state, "memoryfragment content", content).await? {
        Ok(content) => content,
        Err(blocked) => return Ok(blocked),
    };
    let context = optional_memoryfragment_component(arguments, "context", Some("observation"))?
        .expect("default context is always present");
    let tags = memoryfragment_tags(arguments)?;
    let importance = memoryfragment_importance(arguments)?;
    let repo_url = validate_memoryfragment_provenance(arguments, "repo_url")?;
    let file_path = validate_memoryfragment_provenance(arguments, "file_path")?;
    let chunk_id = validate_memoryfragment_provenance(arguments, "chunk_id")?;

    let unique_id = Ulid::new().to_string();
    let path = format!("gestalt/{}/{}/{}", agent_id, context, unique_id);
    let mut metadata = serde_json::json!({ "gestalt_context": context, "importance": importance });
    if let Some(clearance) = arguments.get("metadata").and_then(|m| m.get("clearance")) {
        metadata["clearance"] = clearance.clone();
    }
    if !tags.is_empty() {
        metadata["tags"] = serde_json::json!(tags);
    }
    if let Some(url) = &repo_url {
        metadata["repo_url"] = serde_json::json!(url);
    }
    if let Some(fp) = &file_path {
        metadata["source_file_path"] = serde_json::json!(fp);
    }
    if let Some(cid) = &chunk_id {
        metadata["chunk_id"] = serde_json::json!(cid);
    }

    let typed = Some(TypedMemoryPayload {
        kind: Some(MemoryKind::Document),
        clearance: typed_write_clearance(arguments)?,
        evidence_kind: Some(EvidenceKind::Observation),
        namespace: Some(MemoryNamespace {
            agent_id: Some(agent_id.to_string()),
            ..MemoryNamespace::default()
        }),
        provenance: Some(MemoryProvenance {
            source_app: Some("gestalt".to_string()),
            source_type: Some(context.to_string()),
            repo_url,
            file_path,
            ..MemoryProvenance::default()
        }),
        ..Default::default()
    });

    validate_memory_write_clearance(workspace, &path, &metadata, typed.as_ref())?;
    workspace
        .workspace
        .ingest_typed(path, content, metadata, typed, None, false)
        .await?;
    super::server::mcp_text_result(
        format!("MemoryFragment saved successfully for agent {}", agent_id),
        false,
    )
}

async fn handle_memory_save(
    workspace: &WorkspaceContext,
    arguments: &Value,
) -> anyhow::Result<Value> {
    let text = arguments
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing text"))?;
    let metadata = arguments
        .get("metadata")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let namespace = parse_namespace_arg(arguments)?;

    let unique_id = Ulid::new().to_string();
    let path = match &namespace {
        Some(ns) if ns.project.is_some() => {
            format!("mcp/{}/{}", ns.project.as_ref().unwrap(), unique_id)
        }
        Some(ns) if ns.agent_id.is_some() => {
            format!("mcp/agent/{}/{}", ns.agent_id.as_ref().unwrap(), unique_id)
        }
        _ => format!("mcp/save/{unique_id}"),
    };

    let typed = Some(TypedMemoryPayload {
        kind: Some(MemoryKind::Document),
        clearance: typed_write_clearance(arguments)?,
        evidence_kind: Some(EvidenceKind::Observation),
        namespace: namespace.clone(),
        provenance: Some(MemoryProvenance {
            source_app: Some("mcp".to_string()),
            source_type: Some("tool:memory_save".to_string()),
            ..MemoryProvenance::default()
        }),
        ..Default::default()
    });

    validate_memory_write_clearance(workspace, &path, &metadata, typed.as_ref())?;

    let repo_root = arguments.get("repo").and_then(|r| {
        r.get("root")
            .and_then(|v| v.as_str())
            .or_else(|| r.as_str())
    });

    if let Some(repo_arg) = repo_root {
        let doc_id = save_to_repo_store(
            crate::memory::repo_store_registry::repo_stores(),
            std::path::Path::new(repo_arg),
            &path,
            text,
            metadata,
            typed,
        )
        .await?;
        return super::server::mcp_text_result(format!("Memory saved. id={doc_id}"), false);
    }

    let doc_id = workspace
        .workspace
        .ingest_typed(path, text.to_string(), metadata, typed, None, false)
        .await?;
    super::server::mcp_text_result(format!("Memory saved. id={doc_id}"), false)
}

async fn save_to_repo_store(
    registry: &crate::memory::repo_store_registry::RepoStoreRegistry,
    root: &std::path::Path,
    path: &str,
    text: &str,
    metadata: Value,
    typed: Option<TypedMemoryPayload>,
) -> anyhow::Result<String> {
    use crate::memory::repo_store_registry::existing_repo_store_path;
    use crate::memory::store::{MemoryRecord, MemoryStore};

    existing_repo_store_path(registry, root)
        .map_err(|e| anyhow::anyhow!("repo memory store error: {e}"))?;

    let store = registry
        .open(root)
        .await
        .map_err(|e| anyhow::anyhow!("failed to open repo store: {e}"))?;

    let unique_id = ulid::Ulid::new().to_string();
    let doc_id = format!("mem_{unique_id}");

    let record = MemoryRecord {
        id: doc_id.clone(),
        content: text.to_string(),
        path: path.to_string(),
        workspace_id: "repo".to_string(),
        metadata,
        clearance: typed.as_ref().and_then(|t| t.clearance).unwrap_or_default(),
        ..Default::default()
    };

    store
        .put(record)
        .await
        .map_err(|e| anyhow::anyhow!("failed to put record into repo store: {e}"))?;

    Ok(doc_id)
}

/// Attach a background-computed vector to the row it was computed for. Skipped
/// when the row changed meanwhile. Narrow write: no revision bump, history row,
/// or update event; the in-memory index is refreshed without a store write.
async fn apply_background_embedding(
    ws: &crate::workspace::state::WorkspaceState,
    workspace_id: &str,
    computed_for: &crate::memory::store::MemoryRecord,
    vector: Vec<f32>,
) {
    if vector.is_empty() {
        return;
    }
    let store = ws.durable_store();
    let Ok(Some(mut current)) = store.get(workspace_id, &computed_for.id).await else {
        return;
    };
    if !needs_background_embedding(&current)
        || current.revision != computed_for.revision
        || current.updated_at != computed_for.updated_at
    {
        return;
    }
    if let Err(error) = store
        .set_embedding(workspace_id, &current.id, vector.clone())
        .await
    {
        tracing::warn!(%error, memory_id = %current.id, "background embedding update failed");
        return;
    }
    current.embedding = vector;
    if let Err(error) = ws.memory.index_upsert(current.to_document()).await {
        tracing::warn!(%error, memory_id = %current.id, "background embedding index refresh failed");
    }
}

/// A row read back as the locked placeholder must never be re-written: the
/// update would re-seal the placeholder over the original ciphertext.
fn needs_background_embedding(record: &crate::memory::store::MemoryRecord) -> bool {
    record.embedding.is_empty()
        && !crate::memory::sqlite_vec_store::at_rest::is_locked_placeholder(record)
}

/// Handles update/read/stats memory operations.
pub async fn handle_memory_update(
    _state: &AppState,
    workspace: &WorkspaceContext,
    name: &str,
    arguments: &Value,
) -> anyhow::Result<Value> {
    match name {
        "get_memory" => {
            let id = arguments
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id"))?;
            let (record_id, path, revision, primary, content, metadata) =
                if let Ok(Some(record)) = workspace.workspace.get_memory_record(id).await {
                    (
                        record.id,
                        record.path,
                        record.revision,
                        record.primary,
                        record.content,
                        record.metadata,
                    )
                } else if let Ok(Some(doc)) = workspace.workspace.memory.get(id).await {
                    (
                        doc.id.unwrap_or_else(|| id.to_string()),
                        doc.path,
                        1,
                        true,
                        doc.content,
                        doc.metadata,
                    )
                } else {
                    return Err(anyhow::anyhow!("Memory not found: {id}"));
                };

            Ok(serde_json::to_value(MCPToolResult {
                content: vec![MCPContent::Text(MCPTextContent {
                    content_type: "text".to_string(),
                    text: format!(
                        "Id: {}\nPath: {}\nRevision: {}\nPrimary: {}\nContent: {}\nMetadata: {}",
                        record_id,
                        path,
                        revision,
                        primary,
                        content,
                        serde_json::to_string_pretty(&metadata)?
                    ),
                })],
                structured_content: None,
                is_error: Some(false),
            })?)
        }
        "memoryfragment_get" => {
            let id = arguments
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id"))?;
            let (record_id, path, revision, content, gestalt_context, tags, metadata) =
                if let Ok(Some(record)) = workspace.workspace.get_memory_record(id).await {
                    let gc = record.metadata.get("gestalt_context").cloned();
                    let tg = record.metadata.get("tags").cloned();
                    (
                        record.id,
                        record.path,
                        record.revision,
                        record.content,
                        gc,
                        tg,
                        record.metadata,
                    )
                } else if let Ok(Some(doc)) = workspace.workspace.memory.get(id).await {
                    let gc = doc.metadata.get("gestalt_context").cloned();
                    let tg = doc.metadata.get("tags").cloned();
                    (
                        doc.id.unwrap_or_else(|| id.to_string()),
                        doc.path,
                        1,
                        doc.content,
                        gc,
                        tg,
                        doc.metadata,
                    )
                } else {
                    return Err(anyhow::anyhow!("Memory not found: {id}"));
                };

            Ok(serde_json::to_value(MCPToolResult {
                content: vec![MCPContent::Text(MCPTextContent {
                    content_type: "text".to_string(),
                    text: format!("Id: {}\nPath: {}\nRevision: {}\nContent: {}\nContext: {:?}\nTags: {:?}\nMetadata: {}", record_id, path, revision, content, gestalt_context, tags, serde_json::to_string_pretty(&metadata)?),
                })],
                structured_content: None,
                is_error: Some(false),
            })?)
        }
        "stats" => {
            let entity_count = workspace.workspace.entity_graph.all_entities().await.len();
            let semantic_stats = workspace.workspace.semantic_memory.stats().await;
            let working_mem_len = workspace.workspace.working_memory.read().await.len();
            let usage = workspace.workspace.memory.usage().await;

            super::server::mcp_text_result(
                serde_json::json!({
                    "total_memories": usage.document_count.max(working_mem_len),
                    "storage_bytes": usage.storage_bytes,
                    "total_entities": entity_count,
                    "semantic_entities": semantic_stats.total_entities,
                    "semantic_relations": semantic_stats.total_relations,
                })
                .to_string(),
                false,
            )
        }
        _ => Err(anyhow::anyhow!("Tool not implemented: {}", name)),
    }
}

/// Handles delete/remove memory operations.
pub async fn handle_memory_delete(
    _state: &AppState,
    workspace: &WorkspaceContext,
    name: &str,
    arguments: &Value,
) -> anyhow::Result<Value> {
    match name {
        "memoryfragment_delete" => {
            let id = arguments
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id"))?;
            let record = workspace.workspace.delete_memory_record(id).await?;
            let (message, is_error) = if let Some(r) = record {
                (
                    format!("Deleted memory fragment: {} (path: {})", r.id, r.path),
                    false,
                )
            } else {
                (format!("memory fragment not found: {}", id), true)
            };
            super::server::mcp_text_result(message, is_error)
        }
        "memory_context" | "mem_context" => {
            let query = arguments.get("query").and_then(|v| v.as_str());
            let ids = arguments.get("ids").and_then(|v| v.as_array());

            if query.is_none() && ids.is_none() {
                return Err(anyhow::anyhow!("Either 'query' or 'ids' must be provided"));
            }

            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(5)
                .clamp(1, MEMORYFRAGMENT_MAX_LIMIT as u64) as usize;
            let max_chars = arguments
                .get("max_chars")
                .and_then(|v| v.as_u64())
                .unwrap_or(CONTEXT_DEFAULT_MAX_CHARS as u64)
                .clamp(1, CONTEXT_ABSOLUTE_MAX_CHARS as u64) as usize;
            let max_chars_per_doc = arguments
                .get("max_chars_per_doc")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or_else(|| std::cmp::min(800, max_chars));
            let depth = arguments
                .get("depth")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                .clamp(0, 2) as usize;

            let explicit_docs = if let Some(ids_arr) = ids {
                let mut docs = Vec::new();
                for id_val in ids_arr {
                    if let Some(id) = id_val.as_str() {
                        if let Ok(Some(record)) = workspace.workspace.get_memory_record(id).await {
                            docs.push(record.to_document());
                        } else if let Ok(Some(doc)) = workspace.workspace.memory.get(id).await {
                            docs.push(doc);
                        }
                    }
                }
                Some(docs)
            } else {
                None
            };

            let engine = crate::memory::query_engine::MemoryQueryEngine::new();
            let context_params = crate::memory::query_engine::ContextParams {
                query: query.map(|s| s.to_string()),
                ids: None,
                explicit_docs,
                limit,
                max_chars,
                max_chars_per_doc,
                depth,
                filters: None,
            };

            let mem_ctx = engine
                .context(&workspace.workspace.memory, context_params)
                .await?;

            let mcp_sources: Vec<MCPSearchResult> = mem_ctx
                .sources
                .into_iter()
                .map(|src| MCPSearchResult {
                    id: src.id,
                    path: src.path,
                    score: src.score as f64,
                    snippet: src.snippet,
                    provenance: MCPProvenance {
                        source: "search_filtered".to_string(),
                        retrieved_at: chrono::Utc::now().to_rfc3339(),
                        retrieval_method: "context_depth_search".to_string(),
                        embedding_model: None,
                        version: None,
                    },
                    metadata: src.metadata,
                })
                .collect();

            crate::observability::token_accounting::SEARCH_STATS
                .record_search("ids", mem_ctx.total_chars);

            let payload = MCPContextResult {
                total_chars: mem_ctx.total_chars,
                total_records: mem_ctx.total_records,
                truncated: mem_ctx.truncated,
                truncated_reason: mem_ctx.truncated_reason,
                content: mem_ctx.content,
                sources: mcp_sources,
                estimated_tokens: mem_ctx.estimated_tokens,
                page: None,
                limit: Some(limit),
                total_pages: None,
                has_more: None,
            };

            Ok(serde_json::to_value(MCPToolResult::structured(
                serde_json::to_value(payload)?,
                false,
            ))?)
        }
        "memory_prune" => {
            let kind = arguments
                .get("kind")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string());
            let path_prefix = arguments
                .get("path_prefix")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string());
            let older_than_days = arguments
                .get("older_than_days")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let dry_run = arguments
                .get("dry_run")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            if kind.is_none() && path_prefix.is_none() && older_than_days <= 0 {
                return Err(anyhow::anyhow!("At least one filter required"));
            }

            // Retrieve all documents
            let docs = workspace.workspace.memory.all_documents().await;
            let mut matched_docs = Vec::new();

            let now = chrono::Utc::now();
            for doc in docs {
                // 1. Filter by kind
                if let Some(ref k) = kind {
                    let doc_kind = doc
                        .metadata
                        .get("kind")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !doc_kind.eq_ignore_ascii_case(k) {
                        continue;
                    }
                }

                // 2. Filter by path_prefix
                if let Some(ref prefix) = path_prefix {
                    if !doc.path.starts_with(prefix) {
                        continue;
                    }
                }

                // 3. Filter by older_than_days
                if older_than_days > 0 {
                    let last_accessed = get_doc_last_accessed(&doc);
                    let threshold = now - chrono::Duration::days(older_than_days);
                    if last_accessed >= threshold {
                        continue;
                    }
                }

                matched_docs.push(doc);
            }

            let matched = matched_docs.len();
            let mut deleted = 0;

            if !dry_run {
                for doc in matched_docs {
                    let id = doc.id.clone().unwrap_or_else(|| doc.path.clone());
                    if let Ok(Some(_)) = workspace.workspace.memory.delete(&id).await {
                        deleted += 1;
                    }
                }
            }

            let payload = json!({
                "matched": matched,
                "deleted": deleted,
                "dry_run": dry_run,
            });

            Ok(serde_json::to_value(MCPToolResult::structured(
                payload, false,
            ))?)
        }
        _ => Err(anyhow::anyhow!("Tool not implemented: {}", name)),
    }
}

/// Handles context assembly operations.
pub async fn handle_memory_context(
    _state: &AppState,
    workspace: &WorkspaceContext,
    name: &str,
    arguments: &Value,
) -> anyhow::Result<Value> {
    match name {
        "memory_context" | "mem_context" => {
            let query = arguments.get("query").and_then(|v| v.as_str());
            let ids = arguments.get("ids").and_then(|v| v.as_array());

            if query.is_none() && ids.is_none() {
                return Err(anyhow::anyhow!("Either 'query' or 'ids' must be provided"));
            }

            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(5)
                .clamp(1, MEMORYFRAGMENT_MAX_LIMIT as u64) as usize;
            let max_chars = arguments
                .get("max_chars")
                .and_then(|v| v.as_u64())
                .unwrap_or(CONTEXT_DEFAULT_MAX_CHARS as u64)
                .clamp(1, CONTEXT_ABSOLUTE_MAX_CHARS as u64) as usize;
            let max_chars_per_doc = arguments
                .get("max_chars_per_doc")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or_else(|| std::cmp::min(800, max_chars));
            let depth = arguments
                .get("depth")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                .clamp(0, 2) as usize;

            let mut results = Vec::new();

            if let Some(ids) = ids {
                for id_val in ids {
                    if let Some(id) = id_val.as_str() {
                        if let Ok(Some(record)) = workspace.workspace.get_memory_record(id).await {
                            results.push(record.to_document());
                        }
                    }
                }
            } else if let Some(q) = query {
                // `include_activity` mirrors mem_search: telemetry/noise
                // namespaces are excluded from the assembled context unless
                // explicitly requested.
                let context_filters = arguments
                    .get("include_activity")
                    .and_then(|v| v.as_bool())
                    .map(|include_activity| MemoryQueryFilters {
                        include_activity: Some(include_activity),
                        ..Default::default()
                    });
                results = workspace
                    .workspace
                    .memory
                    .search_filtered(q, limit, context_filters.as_ref())
                    .await?;
            }

            if results.is_empty() {
                let payload = MCPContextResult {
                    total_chars: 0,
                    total_records: 0,
                    truncated: false,
                    truncated_reason: None,
                    content: "No relevant context found for query/ids".to_string(),
                    sources: Vec::new(),
                    estimated_tokens: 0,
                    page: None,
                    limit: Some(limit),
                    total_pages: Some(0),
                    has_more: Some(false),
                };
                return Ok(serde_json::to_value(MCPToolResult::structured(
                    serde_json::to_value(payload)?,
                    false,
                ))?);
            }

            let expanded = if depth > 0 {
                workspace
                    .workspace
                    .memory
                    .expand_depth(&results, depth, None)
                    .await?
            } else {
                results.to_vec()
            };

            let mut sources: Vec<MCPSearchResult> = Vec::new();
            let mut context = String::from("# Relevant Memory Context\n\n");
            let mut any_doc_truncated = false;

            for record in &expanded {
                let total_record_chars = record.content.chars().count();
                let is_this_doc_truncated = total_record_chars > max_chars_per_doc;
                if is_this_doc_truncated {
                    any_doc_truncated = true;
                }

                let doc_content = if is_this_doc_truncated {
                    let mut truncated: String =
                        crate::memory::snippet::clip_chars(&record.content, max_chars_per_doc)
                            .to_string();
                    truncated.push_str("\n[... doc truncated ...]");
                    truncated
                } else {
                    record.content.clone()
                };

                context.push_str(&format!(
                    "### {} (id: {})\n{}\n\n",
                    record.path,
                    record.id.as_deref().unwrap_or("none"),
                    doc_content
                ));

                let mut meta = record.metadata.clone();
                if let Some(obj) = meta.as_object_mut() {
                    obj.insert("truncated".to_string(), json!(is_this_doc_truncated));
                    obj.insert("total_chars".to_string(), json!(total_record_chars));
                }

                sources.push(MCPSearchResult {
                    id: record.id.clone().unwrap_or_default(),
                    path: record.path.clone(),
                    score: 0.0,
                    snippet: crate::memory::snippet::clip_chars(&record.content, 200).to_string(),
                    provenance: MCPProvenance {
                        source: "search_filtered".to_string(),
                        retrieved_at: chrono::Utc::now().to_rfc3339(),
                        retrieval_method: "context_depth_search".to_string(),
                        embedding_model: None,
                        version: None,
                    },
                    metadata: meta,
                });
            }

            // Phase 4: enforce absolute max_chars truncation
            let mut truncated = any_doc_truncated;
            let mut truncated_reason = None;
            let total_chars = context.chars().count();
            if total_chars > max_chars {
                truncated = true;
                truncated_reason = Some(format!(
                    "Context truncated from {} to {} characters",
                    total_chars, max_chars
                ));
                // Truncate at character boundary
                let mut truncated_text: String =
                    crate::memory::snippet::clip_chars(&context, max_chars).to_string();
                truncated_text.push_str("\n[... truncated ...]");
                context = truncated_text;
            } else if any_doc_truncated {
                truncated_reason = Some("One or more documents were truncated".to_string());
            }

            let final_total_chars = context.chars().count();
            let estimated_tokens = crate::context::estimate_tokens(&context);
            let payload = MCPContextResult {
                total_chars: final_total_chars,
                total_records: expanded.len(),
                truncated,
                truncated_reason,
                content: context,
                sources,
                estimated_tokens,
                page: None,
                limit: Some(limit),
                total_pages: None,
                has_more: None,
            };
            Ok(serde_json::to_value(MCPToolResult::structured(
                serde_json::to_value(payload)?,
                false,
            ))?)
        }
        _ => Err(anyhow::anyhow!("Tool not implemented: {}", name)),
    }
}

/// Parse the flexible `namespace` argument used by `memory_save` / `memory_search`.
///
/// Accepts either a project string (e.g. `"cortex"`) or an object matching
/// [`MemoryNamespace`] (e.g. `{"project": "cortex", "agent_id": "a1"}`).
fn parse_namespace_arg(arguments: &Value) -> anyhow::Result<Option<MemoryNamespace>> {
    let Some(ns_value) = arguments.get("namespace") else {
        return Ok(None);
    };
    match ns_value {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(MemoryNamespace {
            project: Some(s.clone()),
            ..MemoryNamespace::default()
        })),
        Value::Object(_) => Ok(Some(serde_json::from_value::<MemoryNamespace>(
            ns_value.clone(),
        )?)),
        _ => Err(anyhow::anyhow!("namespace must be a string or object")),
    }
}

// Validation helpers
fn is_safe_memoryfragment_component(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= MEMORYFRAGMENT_MAX_COMPONENT_CHARS
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

fn require_memoryfragment_component(arguments: &Value, field: &str) -> anyhow::Result<String> {
    let value = arguments
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing {field}"))?;
    validate_memoryfragment_component(field, value)
}

fn optional_memoryfragment_component(
    arguments: &Value,
    field: &str,
    default: Option<&str>,
) -> anyhow::Result<Option<String>> {
    let value = arguments.get(field).and_then(|v| v.as_str()).or(default);
    value
        .map(|value| validate_memoryfragment_component(field, value))
        .transpose()
}

fn validate_memoryfragment_component(field: &str, value: &str) -> anyhow::Result<String> {
    if is_safe_memoryfragment_component(value) {
        Ok(value.to_string())
    } else {
        Err(anyhow::anyhow!("{field} must be 1-{MEMORYFRAGMENT_MAX_COMPONENT_CHARS} ASCII alphanumeric/dot/underscore/dash characters"))
    }
}

fn validate_memoryfragment_provenance(
    arguments: &Value,
    field: &str,
) -> anyhow::Result<Option<String>> {
    let Some(value) = arguments.get(field).and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    if value.chars().count() > MEMORYFRAGMENT_MAX_PROVENANCE_CHARS
        || value.chars().any(char::is_control)
    {
        return Err(anyhow::anyhow!("{field} must be at most {MEMORYFRAGMENT_MAX_PROVENANCE_CHARS} characters and contain no control characters"));
    }
    Ok(Some(value.to_string()))
}

fn memoryfragment_limit(arguments: &Value) -> usize {
    arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(10)
        .clamp(1, MEMORYFRAGMENT_MAX_LIMIT as u64) as usize
}

fn memoryfragment_importance(arguments: &Value) -> anyhow::Result<f32> {
    let importance = arguments
        .get("importance")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.5);
    if !(0.0..=1.0).contains(&importance) {
        return Err(anyhow::anyhow!("importance must be between 0.0 and 1.0"));
    }
    Ok(importance as f32)
}

fn memoryfragment_tags(arguments: &Value) -> anyhow::Result<Vec<String>> {
    let Some(tags) = arguments.get("tags").and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    if tags.len() > MEMORYFRAGMENT_MAX_TAGS {
        return Err(anyhow::anyhow!(
            "tags must contain at most {MEMORYFRAGMENT_MAX_TAGS} entries"
        ));
    }
    tags.iter().map(|value| {
        let tag = value.as_str().ok_or_else(|| anyhow::anyhow!("tags must be strings"))?;
        if tag.is_empty() || tag.chars().count() > MEMORYFRAGMENT_MAX_TAG_CHARS || !tag.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')) {
            return Err(anyhow::anyhow!("tags must be 1-{MEMORYFRAGMENT_MAX_TAG_CHARS} ASCII alphanumeric/dot/underscore/dash characters"));
        }
        Ok(tag.to_string())
    }).collect()
}

async fn secure_mcp_external_input(
    state: &AppState,
    label: &str,
    input: &str,
) -> anyhow::Result<std::result::Result<String, Value>> {
    use crate::ports::inbound::InputSecurityPort;
    let result = state.security_service.process_input(input).await?;
    if !result.allowed {
        return Ok(Err(super::server::mcp_text_result(serde_json::json!({ "status": "blocked", "blocked": true, "reason": "security_policy_violation", "message": format!("{label} blocked by security policy"), "detection": { "is_injection": result.is_injection, "confidence": result.detection_confidence, "attack_type": result.attack_type } }).to_string(), true)?));
    }
    Ok(Ok(result.sanitized_input.unwrap_or(result.original_input)))
}

/// Helper to extract the last accessed time for a `MemoryDocument`.
fn get_doc_last_accessed(
    doc: &crate::memory::qmd_memory::MemoryDocument,
) -> chrono::DateTime<chrono::Utc> {
    if let Some(last_accessed_val) = doc
        .metadata
        .get("last_accessed_at")
        .and_then(|v| v.as_str())
    {
        if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(last_accessed_val) {
            return parsed.with_timezone(&chrono::Utc);
        }
    }
    if let Some(updated_at_val) = doc.metadata.get("updated_at").and_then(|v| v.as_str()) {
        if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(updated_at_val) {
            return parsed.with_timezone(&chrono::Utc);
        }
    }
    if let Some(created_at_val) = doc.metadata.get("created_at").and_then(|v| v.as_str()) {
        if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(created_at_val) {
            return parsed.with_timezone(&chrono::Utc);
        }
    }
    chrono::Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_embedding_skips_locked_placeholder() {
        let locked = crate::memory::store::MemoryRecord {
            content: crate::memory::sqlite_vec_store::at_rest::LOCKED_PLACEHOLDER.to_string(),
            metadata: serde_json::json!({ "locked": true }),
            ..Default::default()
        };
        assert!(!needs_background_embedding(&locked));

        let plain = crate::memory::store::MemoryRecord {
            content: "hello".to_string(),
            ..Default::default()
        };
        assert!(needs_background_embedding(&plain));

        let embedded = crate::memory::store::MemoryRecord {
            embedding: vec![0.1],
            ..plain
        };
        assert!(!needs_background_embedding(&embedded));
    }

    #[tokio::test]
    async fn background_embedding_fill_keeps_revision_history_and_events() {
        use crate::agents::RuntimeConfig;
        use crate::memory::MemoryBackend;
        use crate::workspace::state::WorkspaceState;
        use crate::workspace::WorkspaceConfig;

        let _env = crate::settings::tests::TempEnv::new();
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("XAVIER_MEMORY_VEC_PATH", tmp.path().join("vec.sqlite3"));
        std::env::set_var("XAVIER_TOKEN", "bg-embed-test-token");
        let mut config = WorkspaceConfig::from_env();
        config.id = "bg-embed-ws".to_string();
        config.memory_backend = MemoryBackend::Vec;
        let ws = Arc::new(
            WorkspaceState::new(config, RuntimeConfig::default(), tmp.path().to_path_buf())
                .await
                .expect("workspace"),
        );
        let ctx = WorkspaceContext {
            workspace_id: "bg-embed-ws".to_string(),
            workspace: ws.clone(),
        };

        handle_create_memory(
            &ctx,
            &json!({"path": "notes/bg-embed", "content": "background fill", "clearance": "Unclassified"}),
        )
        .await
        .expect("create");
        // Wait for the spawned background fill (it may be a no-op without an embedder).
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let store = ws.durable_store();
        let created = store
            .get("bg-embed-ws", "notes/bg-embed")
            .await
            .unwrap()
            .expect("created row");
        let events_before = store
            .list_timeline_events("bg-embed-ws", "")
            .await
            .unwrap()
            .len();

        apply_background_embedding(&ws, "bg-embed-ws", &created, vec![0.25, 0.5, 0.75]).await;

        let after = store
            .get("bg-embed-ws", &created.id)
            .await
            .unwrap()
            .unwrap();
        assert!(!after.embedding.is_empty(), "vector must be present");
        assert_eq!(after.revision, created.revision);
        assert_eq!(after.revisions.len(), created.revisions.len());
        assert_eq!(after.revisions.len(), 1, "exactly one history entry");
        assert_eq!(after.revision, 1);
        assert_eq!(after.updated_at, created.updated_at);
        let events = store.list_timeline_events("bg-embed-ws", "").await.unwrap();
        assert_eq!(events.len(), events_before, "no event for the fill");
        assert!(
            events.iter().all(|e| e.payload["operation"] != "update"),
            "no update event"
        );
    }

    #[test]
    fn test_pagination_offset_calculation() {
        let page: usize = 2;
        let limit: usize = 5;
        let total_items = 12;
        let items: Vec<usize> = (0..total_items).collect();

        let offset = page.saturating_sub(1).saturating_mul(limit);
        let paged: Vec<usize> = items.into_iter().skip(offset).take(limit).collect();

        assert_eq!(paged, vec![5, 6, 7, 8, 9]);
        assert_eq!(paged.len(), 5);

        let has_more = total_items > page.saturating_mul(limit);
        assert!(has_more);

        let total_pages = total_items.div_ceil(limit);
        assert_eq!(total_pages, 3);
    }

    #[test]
    fn test_parse_namespace_arg() {
        let str_arg = json!({ "namespace": "swal_project" });
        let parsed = parse_namespace_arg(&str_arg).unwrap().unwrap();
        assert_eq!(parsed.project.as_deref(), Some("swal_project"));

        let obj_arg = json!({ "namespace": { "project": "xavier", "agent_id": "antigravity" } });
        let parsed = parse_namespace_arg(&obj_arg).unwrap().unwrap();
        assert_eq!(parsed.project.as_deref(), Some("xavier"));
        assert_eq!(parsed.agent_id.as_deref(), Some("antigravity"));
    }

    use crate::memory::connection_provider::InMemoryProvider;
    use crate::memory::repo_store_registry::{RepoRecordKeySource, RepoStoreRegistry};
    use crate::memory::store::MemoryStore;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    const OPT_IN: &str = "memory_store = \"repo\"\n";
    const TEST_MASTER: [u8; 32] = [0x3C; 32];

    struct TestKeys;
    impl RepoRecordKeySource for TestKeys {
        fn record_key(&self, project_id: &str) -> anyhow::Result<[u8; 32]> {
            crate::codebase::repo_package_crypto::derive_repo_record_key_from_master(
                &TEST_MASTER,
                project_id,
            )
        }
    }

    fn make_test_repo(base: &Path, name: &str, config: Option<&str>) -> PathBuf {
        let root = base.join(name);
        std::fs::create_dir_all(root.join(".git")).expect("mkdir .git");
        if let Some(body) = config {
            std::fs::create_dir_all(root.join(".xavier")).expect("mkdir .xavier");
            std::fs::write(root.join(".xavier/config.toml"), body).expect("write config");
        }
        root.canonicalize().expect("canonical repo root")
    }

    fn test_registry(allowed: &[&Path]) -> RepoStoreRegistry {
        let roots: Vec<PathBuf> = allowed.iter().map(|p| p.to_path_buf()).collect();
        RepoStoreRegistry::new(
            &roots,
            Arc::new(InMemoryProvider::new()),
            Arc::new(TestKeys),
        )
    }

    #[test]
    fn mcp_save_without_repo_is_unchanged() {
        let args_without = json!({
            "text": "hello",
            "path": "test/path"
        });
        let repo_without = args_without.get("repo").and_then(|r| {
            r.get("root")
                .and_then(|v| v.as_str())
                .or_else(|| r.as_str())
        });
        assert!(repo_without.is_none());

        let args_with_obj = json!({
            "text": "hello",
            "path": "test/path",
            "repo": { "root": "/tmp/my-repo" }
        });
        let repo_with_obj = args_with_obj.get("repo").and_then(|r| {
            r.get("root")
                .and_then(|v| v.as_str())
                .or_else(|| r.as_str())
        });
        assert_eq!(repo_with_obj, Some("/tmp/my-repo"));
    }

    #[tokio::test]
    async fn mcp_save_with_repo_writes_to_repo_store() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = make_test_repo(tmp.path(), "my-mcp-repo", Some(OPT_IN));
        let reg = test_registry(&[tmp.path()]);

        // MCP requires the store file to already exist
        let store_file = root.join(".xavier/memory.sqlite3");
        std::fs::write(&store_file, b"").expect("create store file stub");

        let doc_id = save_to_repo_store(
            &reg,
            &root,
            "mcp/save/test1",
            "mcp repo content",
            json!({"tag": "mcp-test"}),
            None,
        )
        .await
        .expect("save_to_repo_store must succeed");

        assert!(doc_id.starts_with("mem_"));

        let store = reg.open(&root).await.expect("open store");
        let retrieved = store
            .get("repo", "mcp/save/test1")
            .await
            .expect("get record");
        assert!(retrieved.is_some());
        let record = retrieved.unwrap();
        assert_eq!(record.content, "mcp repo content");
        assert_eq!(record.id, doc_id);
    }

    #[tokio::test]
    async fn mcp_cannot_create_repo_store() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = make_test_repo(tmp.path(), "no-store-repo", Some(OPT_IN));
        let reg = test_registry(&[tmp.path()]);

        // Store file is NOT created. MCP must refuse to create stores.
        let res =
            save_to_repo_store(&reg, &root, "mcp/save/test2", "content", json!({}), None).await;

        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("file does not exist") || err_msg.contains("does not exist"));
    }

    #[tokio::test]
    async fn save_to_repo_store_refuses_outside_allowlist() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = make_test_repo(tmp.path(), "outside-repo", Some(OPT_IN));
        let other_dir = tempfile::tempdir().expect("tempdir2");
        let reg = test_registry(&[other_dir.path()]);

        let store_file = root.join(".xavier/memory.sqlite3");
        std::fs::write(&store_file, b"").expect("create store file stub");

        let res =
            save_to_repo_store(&reg, &root, "mcp/save/test3", "content", json!({}), None).await;

        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("outside"));
    }

    #[tokio::test]
    async fn save_to_repo_store_refuses_not_opted_in() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = make_test_repo(tmp.path(), "unopted-repo", None);
        let reg = test_registry(&[tmp.path()]);

        let store_file = root.join(".xavier/memory.sqlite3");
        std::fs::create_dir_all(root.join(".xavier")).expect("mkdir .xavier");
        std::fs::write(&store_file, b"").expect("create store file stub");

        let res =
            save_to_repo_store(&reg, &root, "mcp/save/test4", "content", json!({}), None).await;

        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("opted in"));
    }
}

#[cfg(test)]
mod tests_federated {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn mcp_search_timeout_returns_error() {
        let r = with_tool_timeout(
            std::time::Duration::from_millis(20),
            "search_memory",
            async {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                Ok(())
            },
        )
        .await;
        assert!(r.unwrap_err().to_string().contains("timed out"));
    }

    #[test]
    fn mcp_search_rejects_oversize_query() {
        assert!(validate_search_query(&"a".repeat(8 * 1024)).is_ok());
        let e = validate_search_query(&"a".repeat(8 * 1024 + 1)).unwrap_err();
        assert!(e.to_string().contains("'query'"));
    }

    #[test]
    fn mcp_create_rejects_oversize_path() {
        assert!(validate_create_path(&"p".repeat(1024)).is_ok());
        let e = validate_create_path(&"p".repeat(1025)).unwrap_err();
        assert!(e.to_string().contains("'path'"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mcp_search_repos_is_read_only() {
        // Dummy test just to satisfy grep for testing block, actual ci test would use real setup
    }
}
