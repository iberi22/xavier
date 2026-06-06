//! CLI agent spawning and swarm commands
//!
//! Handles `xavier spawn`, `xavier multi-spawn`, and `xavier swarm` subcommands.
//! These commands create and run one or more agent instances with optional
//! provider/model routing, skill loading, and context injection.

use crate::cli::server::SwarmConfig;

use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use xavier::agents::{Agent, AgentConfig};
use xavier::memory::qmd_memory::{MemoryDocument, QmdMemory};
use xavier::memory::sqlite_vec_store::VecSqliteMemoryStore;
use xavier::memory::store::{MemoryRecord, MemoryStore};

/// Spawn a fixed number of agents with optional provider routing and skill loading.
///
/// Each agent gets a unique index and may inherit provider/model from
/// the user-supplied vectors (cycling if fewer providers than agents).
pub async fn spawn_agents(
    count: usize,
    providers: Vec<String>,
    models: Vec<String>,
    skills: &[String],
    custom_context: &[String],
    task: Option<&str>,
) -> Result<()> {
    println!("Spawning {} agents...", count);

    let available_providers = if providers.is_empty() {
        vec!["local".to_string()]
    } else {
        providers
    };

    let mut agents = Vec::with_capacity(count);
    for i in 0..count {
        let name = format!("agent-{}", i + 1);
        let provider_name = available_providers
            .get(i % available_providers.len())
            .cloned();
        let model_name = models.get(i % models.len().max(1)).cloned();

        let mut context = HashMap::new();
        context.insert("agent_index".to_string(), i.to_string());
        context.insert("total_agents".to_string(), count.to_string());
        if let Some(ref provider_name) = provider_name {
            context.insert("spawn_provider".to_string(), provider_name.clone());
        }

        for kv in custom_context {
            if let Some((key, value)) = kv.split_once('=') {
                context.insert(key.to_string(), value.to_string());
            }
        }

        let mut effective_skills = skills.to_vec();
        if let Some(ref provider_name) = provider_name {
            let provider_key = provider_name.to_lowercase();
            if provider_key.contains("minimax")
                && !effective_skills.iter().any(|skill| skill == "coding-agent")
            {
                effective_skills.push("coding-agent".to_string());
            }
            if provider_key.contains("deepseek")
                && !effective_skills.iter().any(|skill| skill == "research")
            {
                effective_skills.push("research".to_string());
            }
        }

        let mut loaded_skills = Vec::new();
        for skill_name in &effective_skills {
            if let Some(content) = load_skill(skill_name) {
                context.insert(format!("skill_{}", skill_name), content);
                loaded_skills.push(skill_name.clone());
            } else {
                println!("Warning: skill '{}' not found", skill_name);
            }
        }

        let mut config = AgentConfig::new(name.clone())
            .with_skills(loaded_skills)
            .with_context(context);
        if let Some(ref provider_name) = provider_name {
            config = config.with_provider(provider_name.clone());
        }
        if let Some(ref model_name) = model_name {
            config = config.with_model(model_name.clone());
        }
        if let Some(task) = task {
            config = config.with_task(task.to_string());
        }

        println!(
            "  spawned {} [provider: {}, model: {}]",
            name,
            provider_name.as_deref().unwrap_or("auto"),
            model_name.as_deref().unwrap_or("default"),
        );
        agents.push(Agent::new(config));
    }

    if let Some(task) = task {
        println!("Executing task across spawned agents: {}", task);
        let memory = load_spawn_memory().await?;
        let mut futures = Vec::with_capacity(agents.len());
        for mut agent in agents {
            let memory = Arc::clone(&memory);
            futures.push(tokio::spawn(async move {
                let name = agent.name.clone();
                match agent.run(memory).await {
                    Ok(resp) => println!("  {} completed: {}", name, resp.response),
                    Err(error) => println!("  {} failed: {}", name, error),
                }
            }));
        }

        for future in futures {
            let _ = future.await;
        }
    }

    Ok(())
}

/// Batch-spawn many agents with provider/model cycling.
pub async fn multi_spawn_agents(
    agents_count: usize,
    batch_size: usize,
    providers: Vec<String>,
    models: Vec<String>,
    skills: Vec<String>,
    task: Option<&str>,
) -> Result<()> {
    println!(
        "Batch spawning {} agents in groups of {}...",
        agents_count, batch_size
    );

    let providers = if providers.is_empty() {
        vec!["local".to_string()]
    } else {
        providers
    };

    for offset in (0..agents_count).step_by(batch_size.max(1)) {
        let current_batch = std::cmp::min(batch_size.max(1), agents_count - offset);
        let batch_providers = (0..current_batch)
            .map(|i| providers[(offset + i) % providers.len()].clone())
            .collect::<Vec<_>>();
        let batch_models = if models.is_empty() {
            Vec::new()
        } else {
            (0..current_batch)
                .map(|i| models[(offset + i) % models.len()].clone())
                .collect::<Vec<_>>()
        };

        spawn_agents(
            current_batch,
            batch_providers,
            batch_models,
            &skills,
            &[],
            task,
        )
        .await?;
    }

    Ok(())
}

/// Launch a swarm of agents from a JSON configuration file.
pub async fn run_swarm(config_path: PathBuf, parallel: usize) -> Result<()> {
    println!(
        "Loading swarm configuration from {}...",
        config_path.display()
    );
    let content = std::fs::read_to_string(&config_path)?;
    let swarm: SwarmConfig = serde_json::from_str(&content)?;

    println!(
        "Launching swarm with {} agents (parallelism: {})...",
        swarm.agents.len(),
        parallel
    );
    let memory = load_spawn_memory().await?;

    let semaphore = Arc::new(tokio::sync::Semaphore::new(parallel));
    let mut futures = Vec::new();

    for agent_cfg in swarm.agents {
        let memory = Arc::clone(&memory);
        let semaphore = Arc::clone(&semaphore);

        futures.push(tokio::spawn(async move {
            let _permit = match semaphore.acquire().await {
                Ok(permit) => permit,
                Err(e) => {
                    tracing::error!("Failed to acquire semaphore: {}", e);
                    return;
                }
            };

            let mut config = AgentConfig::new(agent_cfg.name.clone())
                .with_provider(agent_cfg.provider.clone())
                .with_task(agent_cfg.task.clone());

            if let Some(model) = agent_cfg.model {
                config = config.with_model(model);
            }

            if let Some(skills) = agent_cfg.skills {
                config = config.with_skills(skills);
            }

            if let Some(context) = agent_cfg.context {
                config = config.with_context(context);
            }

            let mut agent = Agent::new(config);
            println!("  starting {}", agent.name);
            match agent.run(memory).await {
                Ok(resp) => println!("  {} finished: {}", agent.name, resp.response),
                Err(error) => println!("  {} failed: {}", agent.name, error),
            }
        }));
    }

    for f in futures {
        let _ = f.await;
    }

    println!("Swarm execution completed.");
    Ok(())
}

/// Load a memory store for spawned agents.
pub async fn load_spawn_memory() -> Result<Arc<QmdMemory>> {
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

/// Load a skill file from the filesystem.
///
/// Searches known paths for a skill with the given name.
pub fn load_skill(skill_name: &str) -> Option<String> {
    let paths = [
        format!("skills/{}/SKILL.md", skill_name),
        format!("skills/{}.md", skill_name),
        format!(".agents/skills/{}/SKILL.md", skill_name),
        format!(".agents/skills/{}.md", skill_name),
    ];

    for path in paths {
        if let Ok(content) = std::fs::read_to_string(path) {
            return Some(content);
        }
    }
    None
}
