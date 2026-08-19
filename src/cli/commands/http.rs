//! CLI commands for HTTP API calls (memory, recall, stats, session, export)
//!
//! These functions interact with the Xavier HTTP server to perform
//! memory operations and retrieve server statistics.

use crate::cli::commands::enums::CLI_HTTP_CLIENT;
use crate::cli::commands::spawn::load_spawn_memory;
use crate::cli::config::{
    auth_failed_error, auth_failed_message, classify_error_response, classify_transport_error,
    require_xavier_token, resolve_base_url, xavier_token, CliHttpOutcome,
};

use anyhow::Result;

/// Search memories and display results with scores.
pub async fn recall_memories(query: &str, limit: usize, offline_ok: bool) -> Result<()> {
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
                        format!("{}...", crate::memory::snippet::clip_chars(content, 120))
                    } else {
                        content.to_string()
                    };
                    println!("{:>3}. [{:>12}] σ={:.3}  {}", i + 1, kind, score, preview);
                }
            }
            Ok(())
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            handle_offline_or_fail(
                classify_error_response(status, body),
                offline_ok,
                "recall",
                |memory| async move {
                    match memory.search(query, limit).await {
                        Ok(docs) => {
                            println!("Found {} results offline for \"{}\":", docs.len(), query);
                            for (i, doc) in docs.iter().enumerate() {
                                let content = &doc.content;
                                let kind = doc
                                    .metadata
                                    .get("kind")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                let score = doc
                                    .metadata
                                    .get("score")
                                    .and_then(|v| v.as_f64())
                                    .unwrap_or(1.0);
                                let preview = if content.len() > 120 {
                                    format!(
                                        "{}...",
                                        crate::memory::snippet::clip_chars(content, 120)
                                    )
                                } else {
                                    content.to_string()
                                };
                                println!(
                                    "{:>3}. [{:>12}] σ={:.3}  {}",
                                    i + 1,
                                    kind,
                                    score,
                                    preview
                                );
                            }
                            Ok(())
                        }
                        Err(e) => {
                            println!("❌ Local search failed: {}", e);
                            Err(anyhow::anyhow!("local offline search failed: {e}"))
                        }
                    }
                },
            )
            .await
        }
        Err(e) => {
            handle_offline_or_fail(
                classify_transport_error(&e),
                offline_ok,
                "recall",
                |memory| async move {
                    match memory.search(query, limit).await {
                        Ok(docs) => {
                            println!("Found {} results offline for \"{}\":", docs.len(), query);
                            for (i, doc) in docs.iter().enumerate() {
                                let content = &doc.content;
                                let kind = doc
                                    .metadata
                                    .get("kind")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                let score = doc
                                    .metadata
                                    .get("score")
                                    .and_then(|v| v.as_f64())
                                    .unwrap_or(1.0);
                                let preview = if content.len() > 120 {
                                    format!(
                                        "{}...",
                                        crate::memory::snippet::clip_chars(content, 120)
                                    )
                                } else {
                                    content.to_string()
                                };
                                println!(
                                    "{:>3}. [{:>12}] σ={:.3}  {}",
                                    i + 1,
                                    kind,
                                    score,
                                    preview
                                );
                            }
                            Ok(())
                        }
                        Err(e) => {
                            println!("❌ Local search failed: {}", e);
                            Err(anyhow::anyhow!("local offline search failed: {e}"))
                        }
                    }
                },
            )
            .await
        }
    }
}

/// Fetch and display server statistics.
pub async fn show_stats(offline_ok: bool) -> Result<()> {
    println!("Xavier CLI version: {}", env!("CARGO_PKG_VERSION"));

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
            println!("\nXavier Server Statistics:");
            println!("{}", serde_json::to_string_pretty(&body)?);
            Ok(())
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            handle_offline_or_fail(
                classify_error_response(status, body),
                offline_ok,
                "stats",
                |memory| async move {
                    let usage = memory.usage().await;
                    println!("\nXavier Offline Statistics:");
                    println!("  Workspace: {}", memory.workspace_id());
                    println!("  Document Count: {}", usage.document_count);
                    println!("  Storage (Estimated Bytes): {}", usage.storage_bytes);
                    Ok(())
                },
            )
            .await
        }
        Err(e) => {
            handle_offline_or_fail(
                classify_transport_error(&e),
                offline_ok,
                "stats",
                |memory| async move {
                    let usage = memory.usage().await;
                    println!("\nXavier Offline Statistics:");
                    println!("  Workspace: {}", memory.workspace_id());
                    println!("  Document Count: {}", usage.document_count);
                    println!("  Storage (Estimated Bytes): {}", usage.storage_bytes);
                    Ok(())
                },
            )
            .await
        }
    }
}

async fn handle_offline_or_fail<F, Fut>(
    outcome: CliHttpOutcome,
    offline_ok: bool,
    op: &str,
    offline_fn: F,
) -> Result<()>
where
    F: FnOnce(std::sync::Arc<xavier::memory::qmd_memory::QmdMemory>) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    match outcome {
        CliHttpOutcome::AuthFailed { status } => {
            if offline_ok {
                eprintln!("{}", auth_failed_message(status));
                println!(
                    "⚠️ AUTH_FAILED but --offline-ok set. Falling back to local offline {}...",
                    op
                );
                match load_spawn_memory().await {
                    Ok(memory) => offline_fn(memory).await,
                    Err(e) => {
                        println!(
                            "❌ Failed to initialize local offline database store: {}",
                            e
                        );
                        Err(anyhow::anyhow!(
                            "offline fallback failed after AUTH_FAILED: {e}"
                        ))
                    }
                }
            } else {
                eprintln!("{}", auth_failed_message(status));
                Err(auth_failed_error(status))
            }
        }
        CliHttpOutcome::ConnectionRefused { detail } => {
            println!(
                "⚠️ CONNECTION_REFUSED ({detail}). Falling back to local offline {}...",
                op
            );
            match load_spawn_memory().await {
                Ok(memory) => offline_fn(memory).await,
                Err(e) => {
                    println!(
                        "❌ Failed to initialize local offline database store: {}",
                        e
                    );
                    Err(anyhow::anyhow!(
                        "offline fallback failed after CONNECTION_REFUSED: {e}"
                    ))
                }
            }
        }
        CliHttpOutcome::HttpError { status, body } => {
            println!(
                "⚠️ Server HTTP {status} ({body}). Falling back to local offline {}...",
                op
            );
            match load_spawn_memory().await {
                Ok(memory) => offline_fn(memory).await,
                Err(e) => {
                    println!(
                        "❌ Failed to initialize local offline database store: {}",
                        e
                    );
                    Err(anyhow::anyhow!(
                        "offline fallback failed after HTTP {status}: {e}"
                    ))
                }
            }
        }
    }
}

/// Re-index memories missing embeddings via the HTTP server.
pub async fn reindex_memories() -> Result<()> {
    let token = xavier_token();
    let base_url = resolve_base_url();
    let url = format!("{}/memory/reindex", base_url);

    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .post(&url)
        .header("X-Xavier-Token", &token)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({}))
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            println!("\nRe-index Complete:");
            println!("  Total: {}", body["total"].as_u64().unwrap_or(0));
            println!("  Re-indexed: {}", body["reindexed"].as_u64().unwrap_or(0));
            println!(
                "  Skipped (already embedded): {}",
                body["skipped"].as_u64().unwrap_or(0)
            );
            let errors = body["errors"].as_array().map(|a| a.len()).unwrap_or(0);
            if errors > 0 {
                println!("  Errors: {}", errors);
                for e in body["errors"].as_array().unwrap_or(&vec![]) {
                    println!("    - {}", e.as_str().unwrap_or("unknown"));
                }
            }
            println!("  Status: {}", body["status"].as_str().unwrap_or("unknown"));
            Ok(())
        }
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if crate::cli::config::is_auth_failure(status) {
                eprintln!("{}", auth_failed_message(status.as_u16()));
                Err(auth_failed_error(status.as_u16()))
            } else {
                println!("❌ Server error: {} {}", status, text);
                Err(anyhow::anyhow!("reindex failed with HTTP {status}"))
            }
        }
        Err(e) => {
            println!("❌ Failed to reach Xavier server: {}", e);
            Err(anyhow::anyhow!("reindex connection failed: {e}"))
        }
    }
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
                Ok(())
            } else {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                if crate::cli::config::is_auth_failure(status) {
                    eprintln!("{}", auth_failed_message(status.as_u16()));
                    Err(auth_failed_error(status.as_u16()))
                } else {
                    println!("❌ Export failed ({}): {}", status, text);
                    Err(anyhow::anyhow!("export-pack failed with HTTP {status}"))
                }
            }
        }
        Err(e) => {
            println!("❌ Error connecting to Xavier server: {}", e);
            Err(anyhow::anyhow!("export-pack connection failed: {e}"))
        }
    }
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
                Ok(())
            } else {
                let status = resp.status();
                if crate::cli::config::is_auth_failure(status) {
                    eprintln!("{}", auth_failed_message(status.as_u16()));
                    Err(auth_failed_error(status.as_u16()))
                } else {
                    println!("Failed to save session context: {}", status);
                    Err(anyhow::anyhow!("session-save failed with HTTP {status}"))
                }
            }
        }
        Err(e) => {
            println!("Error connecting to Xavier server: {}", e);
            println!("Configured endpoint: {}", base_url);
            println!("Is the server running? (xavier http)");
            Err(anyhow::anyhow!("session-save connection failed: {e}"))
        }
    }
}
