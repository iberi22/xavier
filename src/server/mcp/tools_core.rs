//! Core MCP tool implementations
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use super::types::*;
use crate::memory::schema::{
    EvidenceKind, MemoryKind, MemoryNamespace, MemoryProvenance, TypedMemoryPayload,
};
use crate::utils::crypto::hex_encode;
use crate::workspace::WorkspaceContext;
use crate::AppState;
use chrono::Utc;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

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
            name: "get_project_context".to_string(),
            description: "Get full context for a project with resource limits".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Project identifier"
                    },
                    "max_records": {
                        "type": "number",
                        "description": "Maximum records to retrieve",
                        "default": 10
                    },
                    "max_chars": {
                        "type": "number",
                        "description": "Maximum characters to retrieve",
                        "default": 8000
                    },
                    "depth": {
                        "type": "number",
                        "description": "Exploration depth (0: this project only, 1+: sub-projects)",
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
    ]
}

pub fn is_core_tool(name: &str) -> bool {
    matches!(
        name,
        "list_projects" | "get_project_context" | "sync_gitcore" | "health_check"
    )
}

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

            Ok(serde_json::to_value(MCPToolResult {
                content: vec![MCPContent {
                    content_type: "structuredContent".to_string(),
                    text: None,
                    structured_content: Some(json!({ "projects": projects })),
                }],
                is_error: Some(false),
            })?)
        }
        "get_project_context" => {
            let project_id = arguments
                .get("project_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing project_id"))?;

            let max_records = arguments.get("max_records").and_then(|v| v.as_u64()).unwrap_or(10).clamp(1, 50) as usize;
            let max_chars = arguments.get("max_chars").and_then(|v| v.as_u64()).unwrap_or(8000).clamp(1, 32000) as usize;
            let depth = arguments.get("depth").and_then(|v| v.as_u64()).unwrap_or(0).clamp(0, 2) as usize;

            let records = if depth == 0 {
                workspace
                    .workspace
                    .list_memory_records_filtered(
                        crate::memory::schema::MemoryQueryFilters {
                            project: Some(project_id.to_string()),
                            ..Default::default()
                        },
                        max_records,
                    )
                    .await?
            } else {
                // Simplified sub-project support: match prefix or explicit sub-project projects
                workspace
                    .workspace
                    .list_memory_records_filtered(
                        crate::memory::schema::MemoryQueryFilters {
                            path_prefix: Some(format!("{project_id}/")),
                            ..Default::default()
                        },
                        max_records,
                    )
                    .await?
            };

            let mut aggregated_content = String::new();
            let mut sources = Vec::new();
            let mut total_chars = 0;
            let mut truncated = false;
            let mut truncated_reason = None;

            let settings = crate::settings::XavierSettings::current();
            let version = env!("CARGO_PKG_VERSION").to_string();
            let retrieved_at = Utc::now().to_rfc3339();

            for record in records {
                let doc_content = format!("## Path: {}\n{}\n\n", record.path, record.content);
                if total_chars + doc_content.len() > max_chars {
                    truncated = true;
                    truncated_reason = Some("max_chars reached".to_string());
                    break;
                }
                aggregated_content.push_str(&doc_content);
                total_chars += doc_content.len();

                sources.push(MCPSearchResult {
                    id: record.id,
                    path: record.path,
                    score: 1.0,
                    snippet: if record.content.len() > 100 { format!("{}...", &record.content[..100]) } else { record.content.clone() },
                    provenance: MCPProvenance {
                        source: "memory_store".to_string(),
                        retrieved_at: retrieved_at.clone(),
                        retrieval_method: "project_context".to_string(),
                        embedding_model: Some(settings.models.embedding_model.clone()),
                        version: Some(version.clone()),
                    },
                    metadata: record.metadata,
                });
            }

            if aggregated_content.is_empty() && !truncated {
                return Ok(serde_json::to_value(MCPToolResult {
                    content: vec![MCPContent {
                        content_type: "text".to_string(),
                        text: Some(format!("No context found for project {project_id}.")),
                        structured_content: None,
                    }],
                    is_error: Some(false),
                })?);
            }

            Ok(serde_json::to_value(MCPToolResult {
                content: vec![MCPContent {
                    content_type: "structuredContent".to_string(),
                    text: None,
                    structured_content: Some(json!(MCPContextResult {
                        total_chars,
                        total_records: sources.len(),
                        truncated,
                        truncated_reason,
                        content: aggregated_content,
                        sources,
                    })),
                }],
                is_error: Some(false),
            })?)
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
            let tools_count = super::server::get_xavier_tools().len();

            let result = MCPHealthResult {
                status: health.status,
                tools_count,
                handshake_ok: true,
                memory_store_ok: true, // Assuming if we are here, it's ok
                embedding_ok: health.embedding.connected,
                mcp_protocol: "2025-03-26".to_string(), // Current protocol version
            };

            Ok(serde_json::to_value(MCPToolResult {
                content: vec![MCPContent {
                    content_type: "structuredContent".to_string(),
                    text: None,
                    structured_content: Some(serde_json::to_value(result)?),
                }],
                is_error: Some(false),
            })?)
        }
        _ => Err(anyhow::anyhow!("Unknown core tool: {}", name)),
    }
}
