//! Agent handlers for registration, heartbeat, and lifecycle management,
//! as well as CLI handlers for OpenClaw integration.

use anyhow::{Result, Context};
use axum::{
    extract::{Path as AxumPath, State},
    Json,
};
use colored::*;
use std::sync::Arc;
use std::path::PathBuf;

use crate::cli::security::secure_external_input;
use crate::cli::state::CliState;
use crate::cli::types::*;
use crate::cli::commands::enums::AgentCommand;
use crate::settings::XavierSettings;
use crate::memory::openclaw_scanner::OpenClawAgentScanner;
use crate::memory::openclaw_indexer::OpenClawAgentIndexer;
use crate::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use crate::memory::store::{MemoryStore, MemoryRecord};
use crate::memory::cloud_sync::CloudMemorySync;
use crate::memory::supabase_store::SupabaseMemoryStore;
use crate::memory::schema::MemoryLevel;

// --- Axum Handlers (HTTP API) ---

pub async fn agent_register_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<AgentRegisterPayload>,
) -> impl axum::response::IntoResponse {
    let metadata = xavier::coordination::agent_registry::AgentMetadata {
        name: payload.name,
        capabilities: payload.capabilities.unwrap_or_default(),
        role: payload.role,
        endpoint: payload.endpoint,
    };
    let session_id = payload
        .session_id
        .unwrap_or_else(|| payload.agent_id.clone());

    let success = state
        .agent_registry
        .register(payload.agent_id.clone(), session_id.clone(), metadata)
        .await;

    axum::Json(serde_json::json!({
        "status": if success { "ok" } else { "error" },
        "agent_id": payload.agent_id,
        "session_id": session_id,
        "message": if success { "Agent registered successfully" } else { "Registration failed" },
    }))
}

pub async fn agent_heartbeat_handler(
    State(state): State<CliState>,
    AxumPath(agent_id): AxumPath<String>,
) -> impl axum::response::IntoResponse {
    let success = state.agent_registry.heartbeat(&agent_id).await;

    axum::Json(serde_json::json!({
        "status": if success { "ok" } else { "error" },
        "agent_id": agent_id,
        "message": if success { "Heartbeat recorded" } else { "Agent not found" },
    }))
}

pub async fn agent_active_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    let active = state.agent_registry.get_active_agents().await;

    axum::Json(serde_json::json!({
        "status": "ok",
        "active_agents": active.len(),
        "agents": active.iter().map(|a| serde_json::json!({
            "agent_id": a.agent_id,
            "session_id": a.session_id,
            "last_heartbeat": a.last_heartbeat.to_rfc3339(),
            "name": a.metadata.name,
            "capabilities": a.metadata.capabilities,
            "role": a.metadata.role,
            "endpoint": a.metadata.endpoint,
        })).collect::<Vec<_>>(),
    }))
}

pub async fn agent_push_context_handler(
    State(state): State<CliState>,
    AxumPath(agent_id): AxumPath<String>,
    axum::Json(payload): axum::Json<AgentPushContextPayload>,
) -> impl axum::response::IntoResponse {
    let agent = state.agent_registry.get(&agent_id).await;
    if agent.is_none() {
        return axum::Json(serde_json::json!({
            "status": "error",
            "message": "Agent not registered",
        }));
    }

    let context =
        match secure_external_input(state.security.as_ref(), "agent context", &payload.context)
            .await
        {
            Ok(context) => context,
            Err(response) => return axum::Json(response),
        };

    let path = format!("agents/{}/context", agent_id);
    let mut metadata = payload.metadata.unwrap_or(serde_json::json!({}));
    if let Some(obj) = metadata.as_object_mut() {
        obj.insert("agent_id".to_string(), serde_json::json!(agent_id));
        obj.insert(
            "pushed_at".to_string(),
            serde_json::json!(chrono::Utc::now().to_rfc3339()),
        );
    }

    let record = MemoryRecord {
        id: String::new(),
        workspace_id: state.workspace_id.clone(),
        path: path.clone(),
        content: context,
        metadata,
        embedding: vec![],
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        revision: 1,
        primary: true,
        parent_id: None,
        cluster_id: None,
        level: MemoryLevel::Raw,
        relation: None,
        clearance: Default::default(),
        revisions: vec![],
        encrypted_dek: None,
        content_iv: None,
        metadata_iv: None,
    };
    match state.memory.add(record).await {
        Ok(doc_id) => axum::Json(serde_json::json!({
            "status": "ok",
            "path": path,
            "document_id": doc_id,
            "message": "Context stored successfully",
        })),
        Err(e) => axum::Json(serde_json::json!({
            "status": "error",
            "message": format!("Failed to store context: {}", e),
        })),
    }
}

pub async fn agent_unregister_handler(
    State(state): State<CliState>,
    AxumPath(agent_id): AxumPath<String>,
) -> impl axum::response::IntoResponse {
    let success = state.agent_registry.unregister(&agent_id).await;

    axum::Json(serde_json::json!({
        "status": if success { "ok" } else { "error" },
        "agent_id": agent_id,
        "message": if success { "Agent unregistered" } else { "Agent not found or already unregistered" },
    }))
}

pub async fn agent_list_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    let agents = state.agent_registry.get_active_agents().await;
    Json(serde_json::json!({
        "agents": agents.iter().map(|a| serde_json::json!({
            "id": a.agent_id,
            "session_id": a.session_id,
            "last_heartbeat": a.last_heartbeat,
        })).collect::<Vec<_>>(),
        "count": agents.len()
    }))
}

// --- CLI Handlers ---

/// Default agents directory if not configured.
const DEFAULT_AGENTS_DIR: &str = "C:\\Users\\belal\\clawd\\agents";

/// Handle agent commands.
pub async fn handle_agent_command(cmd: AgentCommand) -> Result<()> {
    match cmd {
        AgentCommand::Scan { agent } => handle_agent_scan(agent).await,
        AgentCommand::Index { agent } => handle_agent_index(agent).await,
        AgentCommand::Push { agent } => handle_agent_push(agent).await,
        AgentCommand::Pull { agent } => handle_agent_pull(agent).await,
        AgentCommand::Status { agent } => handle_agent_status(agent).await,
        AgentCommand::Sync { agent } => handle_agent_sync(agent).await,
    }
}

async fn get_scanner() -> Result<OpenClawAgentScanner> {
    let settings = XavierSettings::current();
    let agents_dir = settings.agents.agents_dir.clone()
        .unwrap_or_else(|| DEFAULT_AGENTS_DIR.to_string());
    Ok(OpenClawAgentScanner::new(agents_dir))
}

async fn get_local_store() -> Result<Arc<dyn MemoryStore>> {
    let config = VecSqliteStoreConfig::from_env();
    let store = VecSqliteMemoryStore::new(config).await?;
    Ok(Arc::new(store))
}

async fn get_cloud_sync(local: Arc<dyn MemoryStore>) -> Result<CloudMemorySync> {
    let cloud = Arc::new(SupabaseMemoryStore::from_env().await?);
    let settings = XavierSettings::current();
    let data_dir = PathBuf::from(&settings.memory.data_dir);

    // Using a default node_id or one from settings if available
    let node_id = "xavier-cli".to_string();

    Ok(CloudMemorySync::new(local, cloud, node_id, data_dir))
}

async fn handle_agent_scan(agent_id: Option<String>) -> Result<()> {
    let scanner = get_scanner().await?;

    println!("\n🔍 Scanning OpenClaw agents...");

    let results = if let Some(id) = agent_id {
        vec![scanner.scan_agent(&id).await?]
    } else {
        scanner.scan_all_agents().await?
    };

    if results.is_empty() {
        println!("No agents found.");
        return Ok(());
    }

    println!("\n{:<20} {:<10} {:<20}", "AGENT ID".bold(), "FILES".bold(), "LAST UPDATED".bold());
    println!("{}", "─".repeat(50));

    for res in results {
        println!("{:<20} {:<10} {:<20}",
            res.agent_id.cyan(),
            res.files.len(),
            res.last_updated.to_rfc3339()
        );
    }
    println!();

    Ok(())
}

async fn handle_agent_index(agent_id: Option<String>) -> Result<()> {
    let scanner = get_scanner().await?;
    let store = get_local_store().await?;

    let mut indexer = OpenClawAgentIndexer::new(scanner, store);

    // Attempt to load embedder from env/settings
    if let Ok(embedder) = xavier::embedding::build_embedder_from_env().await {
        indexer = indexer.with_embedder(embedder);
    } else {
        println!("{}", "⚠️ Warning: Could not initialize embedder. Indexing without embeddings.".yellow());
    }

    println!("\n🤖 Indexing OpenClaw agent sessions...");

    let report = if let Some(id) = agent_id {
        indexer.index_agent(&id).await?
    } else {
        indexer.index_all().await?
    };

    println!("\nIndexing complete:");
    println!("  {:<15} {}", "Total Files:", report.total_files.to_string().cyan());
    println!("  {:<15} {}", "Total Chunks:", report.total_chunks.to_string().cyan());
    println!("  {:<15} {}", "Records Created:", report.records_created.to_string().green().bold());
    println!();

    Ok(())
}

async fn handle_agent_push(agent_id: Option<String>) -> Result<()> {
    let local = get_local_store().await?;
    let sync = get_cloud_sync(local).await?;

    println!("\n📤 Pushing agent memory to cloud...");

    if let Some(id) = agent_id {
        let workspace_id = format!("agent:{}", id);
        let report = sync.push_to_cloud(&workspace_id).await?;
        print_sync_report(&report);
    } else {
        // Push all agent workspaces
        let scanner = get_scanner().await?;
        let agents = scanner.scan_all_agents().await?;
        for agent in agents {
            let workspace_id = format!("agent:{}", agent.agent_id);
            println!("Pushing {}...", workspace_id.cyan());
            let report = sync.push_to_cloud(&workspace_id).await?;
            print_sync_report(&report);
        }
    }

    Ok(())
}

async fn handle_agent_pull(agent_id: Option<String>) -> Result<()> {
    let local = get_local_store().await?;
    let sync = get_cloud_sync(local).await?;

    println!("\n📥 Pulling agent memory from cloud...");

    if let Some(id) = agent_id {
        let workspace_id = format!("agent:{}", id);
        let report = sync.pull_from_cloud(&workspace_id).await?;
        print_sync_report(&report);
    } else {
        // This is a bit tricky as we don't know all remote agent workspaces easily
        // without listing them from Supabase. For now, let's sync all workspaces.
        let reports = sync.sync_all_workspaces().await?;
        for report in reports {
            if report.workspace_id.starts_with("agent:") {
                print_sync_report(&report);
            }
        }
    }

    Ok(())
}

async fn handle_agent_status(agent_id: Option<String>) -> Result<()> {
    let local = get_local_store().await?;
    let cloud = Arc::new(SupabaseMemoryStore::from_env().await?);

    println!("\n📊 Agent Sync Status");

    let agent_ids = if let Some(id) = agent_id {
        vec![id]
    } else {
        let scanner = get_scanner().await?;
        scanner.scan_all_agents().await?.into_iter().map(|a| a.agent_id).collect()
    };

    println!("\n{:<20} {:<12} {:<12} {:<10}", "AGENT ID".bold(), "LOCAL".bold(), "REMOTE".bold(), "STATUS".bold());
    println!("{}", "─".repeat(60));

    for id in agent_ids {
        let workspace_id = format!("agent:{}", id);
        let local_count = local.list(&workspace_id).await?.len();
        let remote_count = cloud.list(&workspace_id).await.map(|l| l.len()).unwrap_or(0);

        let status = if local_count == remote_count {
            "Synced".green()
        } else if local_count > remote_count {
            "Needs Push".yellow()
        } else {
            "Needs Pull".blue()
        };

        println!("{:<20} {:<12} {:<12} {:<10}",
            id.cyan(),
            local_count,
            remote_count,
            status
        );
    }
    println!();

    Ok(())
}

async fn handle_agent_sync(agent_id: Option<String>) -> Result<()> {
    println!("\n🔄 Full Agent Sync Sequence initiated...");

    handle_agent_scan(agent_id.clone()).await?;
    handle_agent_index(agent_id.clone()).await?;
    handle_agent_push(agent_id).await?;

    println!("✅ Full sync sequence complete.");
    Ok(())
}

fn print_sync_report(report: &crate::memory::cloud_sync::SyncReport) {
    if report.success {
        println!("  Workspace: {}", report.workspace_id.cyan());
        println!("  Pushed:    {}", report.pushed.to_string().green());
        println!("  Pulled:    {}", report.pulled.to_string().blue());
        println!("  Conflicts: {}", report.conflicts);
        println!("  Duration:  {}ms", report.duration_ms);
    } else {
        println!("  Workspace: {}", report.workspace_id.red());
        println!("  Error:     {}", report.error.as_deref().unwrap_or("Unknown error"));
    }
    println!();
}
