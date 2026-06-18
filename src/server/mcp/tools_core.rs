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
            description: "Get full context for a project".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Project identifier"
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
            name: "get_code_graph".to_string(),
            description: "Get the portable code graph dump (.xavier/codegraph.json)".to_string(),
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
        "list_projects"
            | "get_project_context"
            | "sync_gitcore"
            | "health_check"
            | "get_code_graph"
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
            let records = workspace
                .workspace
                .list_memory_records_filtered(
                    crate::memory::schema::MemoryQueryFilters {
                        project: Some(project_id.to_string()),
                        ..Default::default()
                    },
                    20,
                )
                .await?;
            let matching = records
                .into_iter()
                .map(|record| {
                    format!(
                        "Id: {}\nPath: {}\nRevision: {}\nContent: {}",
                        record.id, record.path, record.revision, record.content
                    )
                })
                .collect::<Vec<_>>();

            super::server::mcp_text_result(
                if matching.is_empty() {
                    format!("No context found for project {project_id}.")
                } else {
                    matching.join("\n\n---\n\n")
                },
                false,
            )
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
            let payload = serde_json::json!({
                "status": health.status,
                "version": health.version,
                "uptime_secs": health.uptime_secs,
                "system": {
                    "cpu_usage_pct": health.system.cpu_usage_pct,
                    "memory_used_mb": health.system.memory_used_mb,
                    "memory_total_mb": health.system.memory_total_mb,
                    "disk_usage_pct": health.system.disk_usage_pct,
                },
                "database": {
                    "size_mb": health.database.size_mb,
                    "needs_vacuum": health.database.needs_vacuum,
                    "fragmentation_pct": health.database.fragmentation_pct,
                },
                "embedding": {
                    "provider": health.embedding.provider,
                    "connected": health.embedding.connected,
                    "latency_ms": health.embedding.latency_ms,
                },
                "mesh": {
                    "connectivity": health.mesh.connectivity,
                    "peers_count": health.mesh.peers_count,
                    "connected_peers": health.mesh.connected_peers,
                },
                "checks": health.checks.iter().map(|c| serde_json::json!({
                    "name": c.name,
                    "status": format!("{:?}", c.status),
                    "detail": c.detail,
                })).collect::<Vec<_>>(),
            });
            super::server::mcp_text_result(payload.to_string(), false)
        }
        "get_code_graph" => {
            let dump_path = std::path::PathBuf::from(".xavier/codegraph.json");
            if !tokio::fs::try_exists(&dump_path).await.unwrap_or(false) {
                return Err(anyhow::anyhow!(
                    "Code graph dump not found at {}. Run 'xavier code scan' to generate it.",
                    dump_path.display()
                ));
            }

            let json_content = tokio::fs::read_to_string(&dump_path).await?;
            let dump: Value = serde_json::from_str(&json_content)?;

            Ok(serde_json::to_value(MCPToolResult {
                content: vec![MCPTextContent {
                    content_type: "text".to_string(),
                    text: serde_json::to_string(&dump)?,
                }],
                is_error: Some(false),
            })?)
        }
        _ => Err(anyhow::anyhow!("Unknown core tool: {}", name)),
    }
}
