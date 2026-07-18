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
            description: "Search memory and return candidates with scores, snippets, and provenance. Use this to FIND relevant memories. Use mem_context to retrieve full content.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "limit": { "type": "number", "description": "Maximum results (default: 10, max: 100)", "default": 10 },
                    "include_content": { "type": "boolean", "description": "Whether to include full content in results (default: false)", "default": false },
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
            input_schema: json!({ "type": "object" }),
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
            input_schema: json!({ "type": "object" }),
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
            input_schema: json!({ "type": "object" }),
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
            description: "Hybrid search over memory documents, optionally scoped to a namespace".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "limit": { "type": "number", "description": "Maximum results", "default": 10 },
                    "depth": { "type": "number", "description": "Relationship depth to explore (0=flat, 1=direct, 2=two-hop)", "default": 0 },
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
            description: "Build an aggregated context block from the most relevant memories for a query or specific IDs. Returns full content bounded by max_chars. Use AFTER mem_search to identify the right memories.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Query describing the desired context" },
                    "ids": { "type": "array", "items": { "type": "string" }, "description": "Optional list of memory IDs to retrieve specifically" },
                    "limit": { "type": "number", "description": "Maximum memories to include (if using query)", "default": 5 },
                    "max_chars": { "type": "number", "description": "Maximum characters to include in context output", "default": 4000 },
                    "depth": { "type": "number", "description": "Relationship depth to explore (0=flat, 1=direct, 2=two-hop)", "default": 0 },
                    "search_mode": { "type": "string", "enum": ["bm25", "semantic", "hybrid"], "description": "RESERVED — currently ignored; search always runs the hybrid pipeline.", "default": "hybrid" }
                }
            }),
        },
    ]
}

pub async fn handle_memory_tool(
    state: AppState,
    workspace: WorkspaceContext,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    match name {
        "mem_search" | "search_memory" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10) as usize;
            let include_content = arguments
                .get("include_content")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let filters = arguments
                .get("filters")
                .cloned()
                .map(serde_json::from_value::<MemoryQueryFilters>)
                .transpose()?;

            let results = workspace
                .workspace
                .memory
                .search_filtered(query, limit, filters.as_ref())
                .await?;

            // Progressive disclosure (Ola 5 · 01 / #497): fat index by default.
            // Structured candidates avoid Debug-dumping full Metadata (token bloat).
            let candidates: Vec<Value> = results
                .into_iter()
                .map(|doc| {
                    let snippet: String = doc.content.chars().take(100).collect();
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

            Ok(serde_json::to_value(MCPToolResult::structured(payload, false))?)
        }
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
        "create_memory" => {
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
        "save_fragment" | "memoryfragment_save" => {
            let agent_id = require_memoryfragment_component(&arguments, "agent_id")?;
            let content = arguments
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing content"))?;
            let content =
                match secure_mcp_external_input(&state, "memoryfragment content", content).await? {
                    Ok(content) => content,
                    Err(blocked) => return Ok(blocked),
                };
            let context =
                optional_memoryfragment_component(&arguments, "context", Some("observation"))?
                    .expect("default context is always present");
            let tags = memoryfragment_tags(&arguments)?;
            let importance = memoryfragment_importance(&arguments)?;
            let repo_url = validate_memoryfragment_provenance(&arguments, "repo_url")?;
            let file_path = validate_memoryfragment_provenance(&arguments, "file_path")?;
            let chunk_id = validate_memoryfragment_provenance(&arguments, "chunk_id")?;

            let unique_id = Ulid::new().to_string();
            let path = format!("gestalt/{}/{}/{}", agent_id, context, unique_id);
            let mut metadata =
                serde_json::json!({ "gestalt_context": context, "importance": importance });
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
        "search_fragments" | "memoryfragment_search" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let query =
                match secure_mcp_external_input(&state, "memoryfragment query", query).await? {
                    Ok(query) => query,
                    Err(blocked) => return Ok(blocked),
                };
            let agent_id = optional_memoryfragment_component(&arguments, "agent_id", None)?;
            let context = optional_memoryfragment_component(&arguments, "context", None)?;
            let tags = memoryfragment_tags(&arguments)?;
            let limit = memoryfragment_limit(&arguments);

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

            let content = filtered
                .into_iter()
                .map(|doc| {
                    MCPContent::Text(MCPTextContent {
                        content_type: "text".to_string(),
                        text: format!(
                            "Id: {}\nPath: {}\nContent: {}\nContext: {:?}\nTags: {:?}",
                            doc.id.as_deref().unwrap_or("none"),
                            doc.path,
                            doc.content,
                            doc.metadata.get("gestalt_context"),
                            doc.metadata.get("tags")
                        ),
                    })
                })
                .collect();

            Ok(serde_json::to_value(MCPToolResult {
                content,
                is_error: Some(false),
            })?)
        }
        "get_recent_fragments" | "memoryfragment_recent" => {
            let agent_id = require_memoryfragment_component(&arguments, "agent_id")?;
            let context = optional_memoryfragment_component(&arguments, "context", None)?;
            let limit = memoryfragment_limit(&arguments);

            let records = workspace
                .workspace
                .list_memory_records_filtered(
                    MemoryQueryFilters {
                        agent_id: Some(agent_id.to_string()),
                        scope: context,
                        ..Default::default()
                    },
                    limit,
                )
                .await?;
            let content = records
                .into_iter()
                .map(|record| {
                    MCPContent::Text(MCPTextContent {
                        content_type: "text".to_string(),
                        text: format!(
                            "Id: {}\nPath: {}\nContent: {}\nContext: {:?}\nTags: {:?}",
                            record.id,
                            record.path,
                            record.content,
                            record.metadata.get("gestalt_context"),
                            record.metadata.get("tags")
                        ),
                    })
                })
                .collect();

            Ok(serde_json::to_value(MCPToolResult {
                content,
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
        "memoryfragment_delete" => {
            let id = arguments
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id"))?;
            let record = workspace.workspace.delete_memory_record(id).await?;
            let message = if let Some(r) = record {
                format!("Deleted memory fragment: {} (path: {})", r.id, r.path)
            } else {
                format!("Memory fragment not found: {}", id)
            };
            super::server::mcp_text_result(message, false)
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
        "memory_save" => {
            let text = arguments
                .get("text")
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
                .ingest_typed(path, text.to_string(), metadata, typed, None, false)
                .await?;
            super::server::mcp_text_result(format!("Memory saved. id={doc_id}"), false)
        }
        "memory_search" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10)
                .clamp(1, MEMORYFRAGMENT_MAX_LIMIT as u64) as usize;
            let depth = arguments
                .get("depth")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                .clamp(0, 2) as usize;
            let namespace = parse_namespace_arg(&arguments)?;

            let mut filters = MemoryQueryFilters::default();
            if let Some(ns) = &namespace {
                filters.project = ns.project.clone();
                filters.agent_id = ns.agent_id.clone();
                filters.scope = ns.scope.clone();
                filters.session_id = ns.session_id.clone();
                filters.user_id = ns.user_id.clone();
            }

            let results = workspace
                .workspace
                .memory
                .search_filtered(query, limit, Some(&filters))
                .await?;

            let results = if depth > 0 {
                workspace
                    .workspace
                    .memory
                    .expand_depth(&results, depth, Some(&filters))
                    .await?
            } else {
                results
            };

            let content = results
                .into_iter()
                .map(|doc| {
                    MCPContent::Text(MCPTextContent {
                        content_type: "text".to_string(),
                        text: format!(
                            "Path: {}\nContent: {}\nMetadata: {:?}",
                            doc.path, doc.content, doc.metadata
                        ),
                    })
                })
                .collect();

            Ok(serde_json::to_value(MCPToolResult {
                content,
                is_error: Some(false),
            })?)
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
                    content: format!("No relevant context found for query/ids"),
                    sources: Vec::new(),
                };
                Ok(serde_json::to_value(MCPToolResult {
                    content: vec![MCPContent::Structured(MCPStructuredContent {
                        content_type: "structuredContent".to_string(),
                        structured_content: serde_json::to_value(payload)?,
                    })],
                    is_error: Some(false),
                })?)
            } else {
                let expanded = if depth > 0 {
                    workspace
                        .workspace
                        .memory
                        .expand_depth(&results, depth, None)
                        .await?
                } else {
                    results.to_vec()
                };

                // We need to preserve the "depth" information for the context formatter
                // Let's use a simpler approach since expand_depth doesn't return depth info
                // For memory_context we might want to keep the manual loop if we need depth labels,
                // or we can just list them.

                let mut sources: Vec<MCPSearchResult> = Vec::new();
                for doc in &expanded {
                    sources.push(MCPSearchResult {
                        id: doc.id.clone().unwrap_or_default(),
                        path: doc.path.clone(),
                        score: 0.0,
                        snippet: doc.content.chars().take(200).collect(),
                        provenance: MCPProvenance {
                            source: "search_filtered".to_string(),
                            retrieved_at: chrono::Utc::now().to_rfc3339(),
                            retrieval_method: "context_depth_search".to_string(),
                            embedding_model: None,
                            version: None,
                        },
                        metadata: doc.metadata.clone(),
                    });
                }

                // Phase 3: build context string
                let mut context = String::from("# Relevant Memory Context\n\n");
                let per_doc_limit = if expanded.is_empty() { 0 } else { max_chars / expanded.len() };

                for record in &expanded {
                    let doc_content = if record.content.chars().count() > per_doc_limit {
                        let mut truncated: String = record.content.chars().take(per_doc_limit).collect();
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
                }

                // Phase 4: enforce absolute max_chars truncation
                let truncated;
                let truncated_reason;
                let total_chars = context.chars().count();
                if total_chars > max_chars {
                    truncated = true;
                    truncated_reason = Some(format!(
                        "Context truncated from {} to {} characters",
                        total_chars, max_chars
                    ));
                    // Truncate at character boundary
                    let mut truncated_text: String = context.chars().take(max_chars).collect();
                    truncated_text.push_str("\n[... truncated ...]");
                    context = truncated_text;
                } else {
                    truncated = false;
                    truncated_reason = None;
                }

                let final_total_chars = context.chars().count();
                let payload = MCPContextResult {
                    total_chars: final_total_chars,
                    total_records: expanded.len(),
                    truncated,
                    truncated_reason,
                    content: context,
                    sources,
                };
                Ok(serde_json::to_value(MCPToolResult {
                    content: vec![MCPContent::Structured(MCPStructuredContent {
                        content_type: "structuredContent".to_string(),
                        structured_content: serde_json::to_value(payload)?,
                    })],
                    is_error: Some(false),
                })?)
            }
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
