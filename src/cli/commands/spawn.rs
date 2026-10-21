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
use xavier::context::skill_controller::composer::{
    compose, CompositionInput, CompositionScope, SkillEntry,
};
use xavier::context::skill_controller::{lease, state::Journal};
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
    let task_id = uuid::Uuid::new_v4().simple().to_string();
    let mut skill_bodies: Vec<(String, String)> = Vec::new();
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
            let skill = skill_name.clone();
            if let Some(content) = tokio::task::spawn_blocking(move || load_skill(&skill)).await? {
                if !skill_bodies.iter().any(|(n, _)| n == skill_name) {
                    skill_bodies.push((skill_name.clone(), content.clone()));
                }
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
        let worktree = std::env::current_dir()?;
        let task_content = compose_task_content(&task_id, task, &skill_bodies)?;
        let journal = tokio::task::spawn_blocking(Journal::from_settings).await??;
        with_task_skill(journal, worktree, task_id, task_content, || async move {
            let memory = load_spawn_memory().await?;
            let mut futures = Vec::with_capacity(agents.len());
            for mut agent in agents {
                let memory = Arc::clone(&memory);
                futures.push(tokio::spawn(async move {
                    let name = agent.name.clone();
                    let response = agent.run(memory, None).await?;
                    println!("  {} completed: {}", name, response.response);
                    Ok::<_, anyhow::Error>(())
                }));
            }
            let mut result = Ok(());
            for future in futures {
                match future.await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => result = Err(error),
                    Err(error) => result = Err(error.into()),
                }
            }
            result
        })
        .await?;
    }

    Ok(())
}

/// Compose the bounded task pack, then append the selected skill bodies.
fn compose_task_content(
    task_id: &str,
    task: &str,
    skill_bodies: &[(String, String)],
) -> Result<String> {
    let workspace_id =
        std::env::var("XAVIER_DEFAULT_WORKSPACE_ID").unwrap_or_else(|_| "default".to_string());
    let scope = CompositionScope::new(&workspace_id, None)?;
    let input = CompositionInput {
        task_id: task_id.to_string(),
        task: task.to_string(),
        skills: skill_bodies
            .iter()
            .map(|(name, body)| SkillEntry {
                name: name.clone(),
                content_hash: xavier::crypto::hex_encode(<sha2::Sha256 as sha2::Digest>::digest(
                    body.as_bytes(),
                )),
            })
            .collect(),
        evidence: Vec::new(),
        memory_available: false,
        graph_revision: None,
    };
    let max = xavier::settings::ephemeral_max_tokens();
    let mut content = compose(&scope, &input, max)?.markdown;
    for (_, body) in skill_bodies {
        content.push('\n');
        content.push_str(body);
        content.push('\n');
    }
    Ok(content)
}

/// Publication is a prerequisite for running; cleanup preserves the runner's result.
async fn with_task_skill<F, Fut>(
    journal: Journal,
    worktree: PathBuf,
    task_id: String,
    content: String,
    run: F,
) -> Result<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let (j, w, id) = (journal.clone(), worktree.clone(), task_id.clone());
    tokio::task::spawn_blocking(move || {
        lease::publish_task_skill_bounded(
            &j,
            &w,
            &id,
            &content,
            xavier::settings::ephemeral_max_tokens(),
        )
    })
    .await??;
    let (j, w, id) = (journal.clone(), worktree.clone(), task_id.clone());
    let (stop, mut stopped) = tokio::sync::oneshot::channel();
    let mut renew = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut stopped => break,
                _ = tokio::time::sleep(std::time::Duration::from_secs(lease::LEASE_SECONDS / 3)) => {},
            }
            let (j, w, id) = (j.clone(), w.clone(), id.clone());
            match tokio::task::spawn_blocking(move || lease::renew_lease(&j, &w, &id)).await {
                Ok(Ok(())) => {}
                error => tracing::warn!(?error, "Task skill lease renewal failed"),
            }
        }
    });
    let result = run().await;
    let _ = stop.send(());
    // Drain any blocking heartbeat before aborting, so release cannot race its root lock.
    let _ = (&mut renew).await;
    renew.abort();
    match tokio::task::spawn_blocking(move || lease::release_task(&journal, &worktree, &task_id))
        .await
    {
        Ok(Ok(report)) => {
            for (_, outcome) in report.entries {
                if let xavier::context::skill_controller::rollback::RollbackOutcome::Conflict(
                    reason,
                ) = outcome
                {
                    tracing::warn!(%reason, "Task skill release conflict");
                }
            }
        }
        error => tracing::warn!(?error, "Task skill release failed"),
    }
    result
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
            match agent.run(memory, None).await {
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
    if !crate::context::skill_controller::manifest::is_valid_slug(skill_name) {
        return None;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn spawn_waits_for_publication_and_releases_on_agent_error() {
        let worktree = tempdir().unwrap();
        let state = tempdir().unwrap();
        let journal = Journal::new(state.path().join("journal")).unwrap();
        let skill = worktree.path().join(".agents/skills/eph-task/SKILL.md");
        let link = worktree.path().join(".claude/skills/eph-task");
        let result = with_task_skill(
            journal.clone(),
            worktree.path().into(),
            "task".into(),
            "body".into(),
            || async {
                assert_eq!(std::fs::read_to_string(&skill).unwrap(), "body");
                assert_eq!(std::fs::read_link(&link).unwrap(), skill.parent().unwrap());
                Err(anyhow::anyhow!("agent fixture failed"))
            },
        )
        .await;
        assert_eq!(result.unwrap_err().to_string(), "agent fixture failed");
        assert!(!skill.parent().unwrap().exists());
        assert!(std::fs::symlink_metadata(link).is_err());
        assert!(journal.committed_transactions().unwrap().is_empty());
    }

    #[tokio::test]
    async fn spawn_never_runs_after_failed_publication() {
        let worktree = tempdir().unwrap();
        let state = tempdir().unwrap();
        let journal = Journal::new(state.path().join("journal")).unwrap();
        let occupied = worktree.path().join(".claude/skills/eph-task");
        std::fs::create_dir_all(occupied.parent().unwrap()).unwrap();
        std::fs::write(&occupied, "foreign").unwrap();
        let ran = std::sync::atomic::AtomicBool::new(false);
        let result = with_task_skill(
            journal,
            worktree.path().into(),
            "task".into(),
            "body".into(),
            || async {
                ran.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            },
        )
        .await;
        assert!(result.is_err());
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(std::fs::read_to_string(occupied).unwrap(), "foreign");
    }

    #[tokio::test]
    async fn spawn_releases_after_success() {
        let worktree = tempdir().unwrap();
        let state = tempdir().unwrap();
        let journal = Journal::new(state.path().join("journal")).unwrap();
        with_task_skill(
            journal,
            worktree.path().into(),
            "task".into(),
            "body".into(),
            || async { Ok(()) },
        )
        .await
        .unwrap();
        assert!(!worktree.path().join(".agents/skills/eph-task").exists());
    }
    #[tokio::test]
    async fn spawn_release_error_preserves_the_agent_error() {
        let worktree = tempdir().unwrap();
        let state = tempdir().unwrap();
        let journal = Journal::new(state.path().join("journal")).unwrap();
        let marker = worktree.path().join(".agents/skills/eph-task/.lease");
        let result = with_task_skill(
            journal,
            worktree.path().into(),
            "task".into(),
            "body".into(),
            || async {
                std::fs::write(marker, "foreign marker").unwrap();
                Err(anyhow::anyhow!("original agent error"))
            },
        )
        .await;
        assert_eq!(result.unwrap_err().to_string(), "original agent error");
        assert!(worktree
            .path()
            .join(".agents/skills/eph-task/SKILL.md")
            .exists());
    }

    #[test]
    fn compose_task_content_uses_composer_and_appends_skills() {
        let bodies = vec![("alpha".to_string(), "ALPHA BODY".to_string())];
        let out = compose_task_content("t1", "do the thing", &bodies).unwrap();
        assert!(out.starts_with("---\nname: eph-t1\n"));
        assert!(out.contains("## Task (untrusted, quoted)"));
        assert!(out.contains("- alpha ("));
        assert!(out.contains("ALPHA BODY"));
    }

    #[tokio::test]
    async fn spawn_publication_enforces_token_budget() {
        let worktree = tempdir().unwrap();
        let state = tempdir().unwrap();
        let journal = Journal::new(state.path().join("journal")).unwrap();
        let big = "word ".repeat(xavier::settings::ephemeral_max_tokens() * 2);
        let ran = std::sync::atomic::AtomicBool::new(false);
        let result = with_task_skill(
            journal,
            worktree.path().into(),
            "task".into(),
            big,
            || async {
                ran.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            },
        )
        .await;
        assert!(result.is_err());
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!worktree.path().join(".agents").exists());
    }

    #[test]
    fn test_load_skill_rejects_invalid_slugs() {
        assert_eq!(
            load_skill("jules-multi-account/../jules-multi-account"),
            None
        );
        for invalid in ["../../etc", "a/b", "", "..", "x\\y", "BadSlug", "a b"] {
            assert_eq!(load_skill(invalid), None);
        }
    }
}
