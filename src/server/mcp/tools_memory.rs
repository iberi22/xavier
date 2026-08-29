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
                    "limit": { "type": "number", "description": "Maximum results (default: 10, max: 100)", "default": 10 },
                    "include_content": { "type": "boolean", "description": "Include full document body in each candidate (default false — prefer memory_context page-in by ids)", "default": false },
                    "search_mode": { "type": "string", "enum": ["bm25", "semantic", "hybrid"], "description": "RESERVED — currently ignored; search always runs the hybrid BM25+vector+RRF pipeline. Kept for forward-compatibility.", "default": "hybrid" },
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
                    "search_mode": { "type": "string", "enum": ["bm25", "semantic", "hybrid"], "description": "RESERVED — currently ignored; search always runs the hybrid pipeline.", "default": "hybrid" }
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

async fn handle_mem_search(
    workspace: &WorkspaceContext,
    arguments: &Value,
) -> anyhow::Result<Value> {
    let query = arguments
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("");
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

    let has_filters = filters.project.is_some()
        || filters.agent_id.is_some()
        || filters.scope.is_some()
        || filters.session_id.is_some()
        || filters.user_id.is_some()
        || filters.kinds.is_some()
        || filters.path_prefix.is_some();
    let filter_ref = if has_filters { Some(&filters) } else { None };

    let results = workspace
        .workspace
        .memory
        .search_filtered(query, limit, filter_ref)
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

    // Progressive disclosure: fat index by default (structured candidates).
    let candidates: Vec<Value> = results
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

    let payload = json!({
        "query": query,
        "include_content": include_content,
        "count": candidates.len(),
        "candidates": candidates,
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

    let records = workspace
        .workspace
        .memory
        .export(false)
        .await?
        .into_iter()
        .take(limit)
        .collect::<Vec<_>>();

    let candidates: Vec<Value> = records
        .into_iter()
        .map(|doc| {
            let snippet: String = crate::memory::snippet::clip_chars(&doc.content, 200).to_string();
            json!({
                "id": doc.id,
                "path": doc.path,
                "snippet": snippet,
                "content": doc.content,
                "metadata": doc.metadata,
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

    workspace
        .workspace
        .ingest_typed(
            path.to_string(),
            content.to_string(),
            metadata,
            Some(TypedMemoryPayload {
                kind,
                evidence_kind,
                namespace,
                provenance,
                ..Default::default()
            }),
            None,
            false,
        )
        .await?;
    super::server::mcp_text_result(
        format!("Memory created successfully at path: {}", path),
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
        evidence_kind: Some(EvidenceKind::Observation),
        namespace: namespace.clone(),
        provenance: Some(MemoryProvenance {
            source_app: Some("mcp".to_string()),
            source_type: Some("tool:memory_save".to_string()),
            ..MemoryProvenance::default()
        }),
        ..Default::default()
    });

    let doc_id = workspace
        .workspace
        .ingest_typed(path, text.to_string(), metadata, typed, None, false)
        .await?;
    super::server::mcp_text_result(format!("Memory saved. id={doc_id}"), false)
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
            let record = workspace
                .workspace
                .get_memory_record(id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Memory not found: {id}"))?;

            Ok(serde_json::to_value(MCPToolResult {
                content: vec![MCPContent::Text(MCPTextContent {
                    content_type: "text".to_string(),
                    text: format!(
                        "Id: {}\nPath: {}\nRevision: {}\nPrimary: {}\nContent: {}\nMetadata: {}",
                        record.id,
                        record.path,
                        record.revision,
                        record.primary,
                        record.content,
                        serde_json::to_string_pretty(&record.metadata)?
                    ),
                })],
                is_error: Some(false),
            })?)
        }
        "memoryfragment_get" => {
            let id = arguments
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id"))?;
            let record = workspace
                .workspace
                .get_memory_record(id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Memory not found: {}", id))?;

            Ok(serde_json::to_value(MCPToolResult {
                content: vec![MCPContent::Text(MCPTextContent {
                    content_type: "text".to_string(),
                    text: format!("Id: {}\nPath: {}\nRevision: {}\nContent: {}\nContext: {:?}\nTags: {:?}\nMetadata: {}", record.id, record.path, record.revision, record.content, record.metadata.get("gestalt_context"), record.metadata.get("tags"), serde_json::to_string_pretty(&record.metadata)?),
                })],
                is_error: Some(false),
            })?)
        }
        "stats" => {
            let records = workspace.workspace.list_memory_records().await?;
            let projects = records
                .iter()
                .filter_map(|r| {
                    r.metadata
                        .get("namespace")
                        .and_then(|n| n.get("project"))
                        .and_then(|p| p.as_str())
                        .map(|p| p.to_string())
                })
                .collect::<std::collections::HashSet<_>>();
            let agents = records
                .iter()
                .filter_map(|r| {
                    r.metadata
                        .get("namespace")
                        .and_then(|n| n.get("agent_id"))
                        .and_then(|a| a.as_str())
                        .map(|a| a.to_string())
                })
                .collect::<std::collections::HashSet<_>>();
            let entity_count = workspace.workspace.entity_graph.all_entities().await.len();
            let semantic_stats = workspace.workspace.semantic_memory.stats().await;

            super::server::mcp_text_result(serde_json::json!({ "total_memories": records.len(), "projects": projects.len(), "agents": agents.len(), "total_entities": entity_count, "semantic_entities": semantic_stats.total_entities, "semantic_relations": semantic_stats.total_relations }).to_string(), false)
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
                .ok_or_else(|| anyhow::anyhow!("Missing text"))?;
            let metadata = arguments
                .get("metadata")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let namespace = parse_namespace_arg(&arguments)?;

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
                evidence_kind: Some(EvidenceKind::Observation),
                namespace: namespace.clone(),
                provenance: Some(MemoryProvenance {
                    source_app: Some("mcp".to_string()),
                    source_type: Some("tool:memory_save".to_string()),
                    ..MemoryProvenance::default()
                }),
                ..Default::default()
            });

            let doc_id = workspace
                .workspace
                .ingest_typed(path, id.to_string(), metadata, typed, None, false)
                .await?;
            super::server::mcp_text_result(format!("Memory saved. id={doc_id}"), false)
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

            let payload = MCPContextResult {
                total_chars: mem_ctx.total_chars,
                total_records: mem_ctx.total_records,
                truncated: mem_ctx.truncated,
                truncated_reason: mem_ctx.truncated_reason,
                content: mem_ctx.content,
                sources: mcp_sources,
                estimated_tokens: mem_ctx.estimated_tokens,
            };

            Ok(serde_json::to_value(MCPToolResult {
                content: vec![MCPContent::Structured(MCPStructuredContent {
                    content_type: "structuredContent".to_string(),
                    structured_content: serde_json::to_value(payload)?,
                })],
                is_error: Some(false),
            })?)
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
                results = workspace
                    .workspace
                    .memory
                    .search_filtered(q, limit, None)
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
                };
                return Ok(serde_json::to_value(MCPToolResult {
                    content: vec![MCPContent::Structured(MCPStructuredContent {
                        content_type: "structuredContent".to_string(),
                        structured_content: serde_json::to_value(payload)?,
                    })],
                    is_error: Some(false),
                })?);
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
            };
            Ok(serde_json::to_value(MCPToolResult {
                content: vec![MCPContent::Structured(MCPStructuredContent {
                    content_type: "structuredContent".to_string(),
                    structured_content: serde_json::to_value(payload)?,
                })],
                is_error: Some(false),
            })?)
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
