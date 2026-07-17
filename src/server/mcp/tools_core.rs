//! Core MCP tool implementations
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use super::types::*;
use crate::memory::schema::{
    EvidenceKind, MemoryKind, MemoryNamespace, MemoryProvenance, MemoryQueryFilters,
    TypedMemoryPayload,
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
            | "xavier_local_status"
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
                    snippet: record.content.chars().take(200).collect(),
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
                    }),
                    false,
                ))?);
            }

            let content = content_parts.join("\n\n---\n\n");
            let total_records = content_parts.len();

            Ok(serde_json::to_value(MCPToolResult::structured(
                json!(MCPContextResult {
                    total_chars,
                    total_records,
                    truncated,
                    truncated_reason,
                    content,
                    sources,
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
                + super::tools_memory::get_xavier_memory_tools().len();

            let result = MCPHealthResult {
                status: health.status.clone(),
                tools_count,
                handshake_ok: true,
                memory_store_ok: health.database.size_mb > 0.0 || health.database.size_mb == 0.0, // store exists
                embedding_ok: health.embedding.connected,
                mcp_protocol: "2026-07-28".to_string(),
            };

            Ok(serde_json::to_value(MCPToolResult::structured(
                serde_json::to_value(&result)?,
                health.status != "healthy",
            ))?)
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
            let dump_path = _state
                .code_graph_dump_path
                .clone()
                .unwrap_or_else(|| std::path::PathBuf::from(".xavier/codegraph.json"));

            if !tokio::fs::try_exists(&dump_path).await.unwrap_or(false) {
                return Err(anyhow::anyhow!(
                    "Code graph dump not found at {}. Run 'xavier code scan' to generate it.",
                    dump_path.display()
                ));
            }

            let json_content = tokio::fs::read_to_string(&dump_path).await?;
            let dump: Value = serde_json::from_str(&json_content)?;

            Ok(serde_json::to_value(MCPToolResult {
                content: vec![MCPContent::Text(MCPTextContent {
                    content_type: "text".to_string(),
                    text: serde_json::to_string(&dump)?,
                })],
                is_error: Some(false),
            })?)
        }
        _ => Err(anyhow::anyhow!("Unknown core tool: {}", name)),
    }
}
