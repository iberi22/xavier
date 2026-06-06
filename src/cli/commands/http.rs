//! CLI commands for HTTP API calls (memory, recall, stats, session, export)
//!
//! These functions interact with the Xavier HTTP server to perform
//! memory operations and retrieve server statistics.

use crate::cli::config::{require_xavier_token, resolve_base_url, xavier_token};
use crate::cli::commands::enums::CLI_HTTP_CLIENT;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

use xavier::memory::qmd_memory::{MemoryDocument, QmdMemory};
use xavier::memory::sqlite_vec_store::VecSqliteMemoryStore;
use xavier::memory::store::{MemoryRecord, MemoryStore};

/// Search memories and display results with scores.
pub async fn recall_memories(query: &str, limit: usize) -> Result<()> {
    let token = xavier_token();
    let base_url = resolve_base_url();
    let url = format!("{}/memory/search", base_url);

    let body = serde_json::json!({
        "query": query,
        "limit": limit,
        "include_scores": true,
    });

    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .post(&url)
        .header("X-Xavier-Token", &token)
        .json(&body)
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let json: serde_json::Value = resp.json().await.unwrap_or_default();
            let results = json["results"].as_array().map(|r| r.len()).unwrap_or(0);
            println!("Found {} results for \"{}\":", results, query);
            if let Some(items) = json["results"].as_array() {
                for (i, item) in items.iter().enumerate() {
                    let content = item["content"].as_str().unwrap_or("(no content)");
                    let kind = item["metadata"]["kind"].as_str().unwrap_or("unknown");
                    let score = item["score"].as_f64().unwrap_or(0.0);
                    let preview = if content.len() > 120 {
                        format!("{}...", &content[..120])
                    } else {
                        content.to_string()
                    };
                    println!("{:>3}. [{:>12}] σ={:.3}  {}", i + 1, kind, score, preview);
                }
            }
        }
        _ => {
            println!("⚠️ Server offline or request failed. Falling back to local offline database index...");
            match load_spawn_memory().await {
                Ok(memory) => {
                    match memory.search(query, limit).await {
                        Ok(docs) => {
                            println!("Found {} results offline for \"{}\":", docs.len(), query);
                            for (i, doc) in docs.iter().enumerate() {
                                let content = &doc.content;
                                let kind = doc.metadata.get("kind").and_then(|v| v.as_str()).unwrap_or("unknown");
                                let score = doc.metadata.get("score").and_then(|v| v.as_f64()).unwrap_or(1.0);
                                let preview = if content.len() > 120 {
                                    format!("{}...", &content[..120])
                                } else {
                                    content.to_string()
                                };
                                println!("{:>3}. [{:>12}] σ={:.3}  {}", i + 1, kind, score, preview);
                            }
                        }
                        Err(e) => {
                            println!("❌ Local search failed: {}", e);
                        }
                    }
                }
                Err(e) => {
                    println!("❌ Failed to initialize local offline database store: {}", e);
                }
            }
        }
    }

    Ok(())
}

/// Fetch and display server statistics.
pub async fn show_stats() -> Result<()> {
    let token = xavier_token();
    let base_url = resolve_base_url();
    let url = format!("{}/memory/stats", base_url);

    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .get(&url)
        .header("X-Xavier-Token", &token)
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            println!("\nXavier Statistics:");
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        _ => {
            println!("⚠️ Server offline or request failed. Falling back to local offline database statistics...");
            match load_spawn_memory().await {
                Ok(memory) => {
                    let usage = memory.usage().await;
                    println!("\nXavier Offline Statistics:");
                    println!("  Workspace: {}", memory.workspace_id());
                    println!("  Document Count: {}", usage.document_count);
                    println!("  Storage (Estimated Bytes): {}", usage.storage_bytes);
                }
                Err(e) => {
                    println!("❌ Failed to initialize local offline database store: {}", e);
                }
            }
        }
    }

    Ok(())
}

/// Export a context pack (.xcp) for a given topic.
pub async fn export_context_pack(
    topic: &str,
    max_level: usize,
    out: &std::path::Path,
) -> Result<()> {
    let token = xavier_token();
    let base_url = resolve_base_url();
    let url = format!("{}/memory/export-pack", base_url);

    let body = serde_json::json!({
        "topic": topic,
        "max_level": max_level,
    });

    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .post(&url)
        .header("X-Xavier-Token", &token)
        .json(&body)
        .send()
        .await;

    match response {
        Ok(resp) => {
            if resp.status().is_success() {
                let json: serde_json::Value = resp.json().await.unwrap_or_default();
                let xml = json["xml"].as_str().unwrap_or_default();
                std::fs::write(out, xml)?;
                println!("✅ Context Pack exported to: {}", out.display());
            } else {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                println!("❌ Export failed ({}): {}", status, text);
            }
        }
        Err(e) => {
            println!("❌ Error connecting to Xavier server: {}", e);
        }
    }

    Ok(())
}

/// Save session context to Xavier memory.
pub async fn session_save(session_id: &str, content: &str) -> Result<()> {
    use crate::cli::security::secure_cli_input;

    let content = secure_cli_input("session content", content, 10_000_000)?;
    let token = require_xavier_token()?;
    let base_url = resolve_base_url();
    let url = format!("{}/memory/add", base_url);

    let body = serde_json::json!({
        "content": content,
        "path": format!("context/{}/save", session_id),
        "metadata": {
            "session_id": session_id,
            "kind": "session_save",
        }
    });

    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .post(&url)
        .header("X-Xavier-Token", &token)
        .json(&body)
        .send()
        .await;

    match response {
        Ok(resp) => {
            if resp.status().is_success() {
                println!("Session context saved successfully!");
                println!("Path: context/{}/save", session_id);
            } else {
                println!("Failed to save session context: {}", resp.status());
            }
        }
        Err(e) => {
            println!("Error connecting to Xavier server: {}", e);
            println!("Configured endpoint: {}", base_url);
            println!("Is the server running? (xavier http)");
        }
    }

    Ok(())
}

/// Load a spawn memory instance backed by SQLite.
async fn load_spawn_memory() -> Result<Arc<QmdMemory>> {
    let store = VecSqliteMemoryStore::from_env().await?;
    let workspace_id =
        std::env::var("XAVIER_DEFAULT_WORKSPACE_ID").unwrap_or_else(|_| "default".to_string());
    let durable_state = store.load_workspace_state(&workspace_id).await?;
    let docs = Arc::new(RwLock::new(
        durable_state
            .memories
            .iter()
            .map(MemoryRecord::to_document)
            .collect::<Vec<MemoryDocument>>(),
    ));
    let memory = Arc::new(QmdMemory::new_with_workspace(docs, workspace_id));
    memory.set_store(Arc::new(store)).await;
    memory.init().await?;
    Ok(memory)
}
