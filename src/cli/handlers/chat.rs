//! Interactive and agent-friendly chat CLI handler for Xavier
//!
//! Provides conversational interaction with contextual memory retrieval
//! and LLM response synthesis for users and autonomous agents.

use anyhow::Result;
use colored::*;
use serde_json::json;
use std::io::{self, Write};

use xavier::agents::provider::ModelProviderClient;
use crate::cli::commands::enums::CLI_HTTP_CLIENT;
use crate::cli::commands::spawn::load_spawn_memory;
use crate::cli::config::{resolve_base_url, xavier_token};
use crate::cli::security::secure_cli_input;

/// Structure representing a retrieved memory item for chat context.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMemoryContext {
    pub id: String,
    pub title: Option<String>,
    pub path: String,
    pub content: String,
    pub score: Option<f64>,
}

/// Handle top-level chat command
pub async fn handle_chat_command(
    prompt: Option<String>,
    agent: Option<String>,
    interactive: bool,
    as_json: bool,
    limit: usize,
    model: Option<String>,
) -> Result<()> {
    let agent_name = agent.unwrap_or_else(|| "agent".to_string());
    let memory_limit = limit.clamp(1, 25);

    if interactive || prompt.is_none() {
        run_interactive_repl(&agent_name, memory_limit, model).await
    } else {
        let question = prompt.unwrap();
        let question = secure_cli_input("chat query", &question, 8_192)?;
        execute_single_turn(&question, &agent_name, as_json, memory_limit, model.as_deref()).await
    }
}

/// Retrieve memories relevant to the query via HTTP if server is running, or offline directly.
pub async fn retrieve_chat_context(query: &str, limit: usize) -> Vec<ChatMemoryContext> {
    let base_url = resolve_base_url();
    let token = xavier_token();
    let url = format!("{}/memory/search", base_url);

    // Attempt HTTP first
    if let Ok(resp) = CLI_HTTP_CLIENT
        .post(&url)
        .header("X-Xavier-Token", &token)
        .json(&serde_json::json!({
            "query": query,
            "limit": limit
        }))
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(val) = resp.json::<serde_json::Value>().await {
                if let Some(results) = val.get("results").and_then(|r| r.as_array()) {
                    let items: Vec<ChatMemoryContext> = results
                        .iter()
                        .map(|item| ChatMemoryContext {
                            id: item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            title: item.get("title").and_then(|v| v.as_str()).map(|s| s.to_string()),
                            path: item.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            content: item.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            score: item.get("score").and_then(|v| v.as_f64()),
                        })
                        .collect();
                    if !items.is_empty() {
                        return items;
                    }
                }
            }
        }
    }

    // Offline local fallback
    if let Ok(memory) = load_spawn_memory().await {
        if let Ok(docs) = memory.search(query, limit).await {
            return docs
                .into_iter()
                .map(|doc| ChatMemoryContext {
                    id: doc.id.clone().unwrap_or_default(),
                    title: doc.metadata.get("title").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    path: doc.path.clone(),
                    content: doc.content.clone(),
                    score: doc.metadata.get("score").and_then(|v| v.as_f64()),
                })
                .collect();
        }
    }

    Vec::new()
}

/// Execute a single conversational turn: retrieve memories -> call LLM -> output answer.
pub async fn execute_single_turn(
    query: &str,
    agent_name: &str,
    as_json: bool,
    memory_limit: usize,
    model_override: Option<&str>,
) -> Result<()> {
    let memories = retrieve_chat_context(query, memory_limit).await;

    // Build context snippet
    let mut context_text = String::new();
    if !memories.is_empty() {
        for (idx, mem) in memories.iter().enumerate() {
            let excerpt: String = mem.content.trim().chars().take(1200).collect();
            context_text.push_str(&format!(
                "[{}] (Path: {}, Title: {})\n{}\n\n",
                idx + 1,
                mem.path,
                mem.title.as_deref().unwrap_or("untitled"),
                excerpt
            ));
        }
    }

    let system_prompt = if context_text.is_empty() {
        format!(
            "You are Xavier, the cognitive memory intelligence of the SWAL ecosystem.              You are conversing with agent '{}'. Answer concisely, accurately, and helpfully.",
            agent_name
        )
    } else {
        format!(
            "You are Xavier, the cognitive memory intelligence of the SWAL ecosystem.              You are conversing with agent '{}'.              Synthesize an accurate, direct response utilizing the retrieved contextual memories below.

             === RETRIEVED MEMORIES ===
             {}
             ==========================",
            agent_name, context_text
        )
    };

    let client = ModelProviderClient::from_model_override(model_override.map(|s| s.to_string()));
    let provider_status = client.status();

    let llm_result = client
        .generate_text_with_cache(&system_prompt, query, false)
        .await;

    match llm_result {
        Ok(resp) => {
            let response_text = resp.text.trim().to_string();
            if as_json {
                let output = json!({
                    "status": "ok",
                    "query": query,
                    "response": response_text,
                    "agent": agent_name,
                    "model": provider_status.model,
                    "provider": provider_status.provider,
                    "retrieved_memories_count": memories.len(),
                    "memories": memories
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("
{}", "─".repeat(60).dimmed());
                println!("{} {}", "🧠 Xavier:".bold().cyan(), response_text);
                if !memories.is_empty() {
                    println!("
{}", format!("(Recuperados {} fragmentos de memoria)", memories.len()).dimmed());
                }
                println!("{}
", "─".repeat(60).dimmed());
            }
        }
        Err(err) => {
            // Graceful fallback to memory synthesis if LLM is offline/unreachable
            if as_json {
                let output = json!({
                    "status": "memory_fallback",
                    "query": query,
                    "error": err.to_string(),
                    "agent": agent_name,
                    "retrieved_memories_count": memories.len(),
                    "memories": memories
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("
{}", "─".repeat(60).dimmed());
                println!("{} {}", "⚠️ Xavier [Modo Memoria Offline]:".bold().yellow(), err.to_string().dimmed());
                if !memories.is_empty() {
                    println!("
{}", "Información relevante encontrada en memoria:".bold());
                    for (i, m) in memories.iter().enumerate() {
                        println!("  {}. [{}] {}", i + 1, m.path.cyan(), m.content.trim());
                    }
                } else {
                    println!("
No se encontró memoria previa relacionada y el modelo LLM no está disponible.");
                }
                println!("{}
", "─".repeat(60).dimmed());
            }
        }
    }

    Ok(())
}

/// Run interactive REPL loop
async fn run_interactive_repl(
    agent_name: &str,
    memory_limit: usize,
    model_override: Option<String>,
) -> Result<()> {
    println!("{}", "╔══════════════════════════════════════════════════════════════════════╗".cyan());
    println!("{}", "║  🧠 Xavier Cognitive Memory — CLI Conversacional e Interactivo       ║".cyan().bold());
    println!("{}", "║  Escribe tu pregunta o consulta. Escribe 'exit' o 'quit' para salir. ║".cyan());
    println!("{}", "╚══════════════════════════════════════════════════════════════════════╝".cyan());
    println!(
        "{} Agente: {} | Memoria: Activa (Top {}) | Modelo: {}
",
        "⚙️".dimmed(),
        agent_name.bold().green(),
        memory_limit,
        model_override.as_deref().unwrap_or("auto (OpenRouter / Local)").magenta()
    );

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("{} > ", format!("xavier[{}]", agent_name).bold().blue());
        stdout.flush()?;

        let mut input = String::new();
        if stdin.read_line(&mut input)? == 0 {
            break; // EOF
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.eq_ignore_ascii_case("exit")
            || trimmed.eq_ignore_ascii_case("quit")
            || trimmed.eq_ignore_ascii_case("q")
            || trimmed.eq_ignore_ascii_case(":q")
        {
            println!("{}", "Sesión de Xavier finalizada. ¡Hasta luego!".green());
            break;
        }

        if let Err(e) = execute_single_turn(
            trimmed,
            agent_name,
            false,
            memory_limit,
            model_override.as_deref(),
        )
        .await
        {
            eprintln!("{} Error: {}", "❌".red(), e);
        }
    }

    Ok(())
}
