//! CLI command implementation sub-modules
//!
//! This module splits the former monolithic `commands.rs` into focused
//! sub-modules:
//!
//! - [`enums`] — Command, UsageCommand, CodeCommand, TokenCommand, SecretsCommand
//! - [`http`] — HTTP-based API calls (recall, stats, export, session_save)
//! - [`code`] — Code graph queries
//! - [`spawn`] — Agent spawning, multi-spawn, swarm
//! - [`token`] — Token generation
//! - [`usage`] — Provider usage tracking
//! - [`secrets`] — Ephemeral secret management (Clavis)
//!
//! The top-level [`Command`] enum and [`Cli::run()`] dispatch remain visible
//! through re-exports so that external consumers are unaffected.

pub mod airgap;
pub mod benchmark_runner;
pub mod billing;
pub mod cleanup;
pub mod code;
pub mod data_commons;
pub mod encrypt_records;
pub mod enums;
pub mod governance;
pub mod http;
pub mod improve;
pub mod installer;
pub mod license;
pub mod memory;
pub mod mesh;
pub mod mirror;
pub mod navigation;
pub mod node;
pub mod nodes;
pub mod provider;
pub mod regen;
pub mod repo;
pub mod secrets;
pub mod session;
pub mod spawn;
pub mod tasks;
pub mod telecom;
pub mod token;
pub mod usage;
pub mod users;
pub mod verify;
pub mod wallet;

// Re-export for backward compatibility
pub use enums::*;
#[allow(unused_imports)]
pub use spawn::load_spawn_memory;

use crate::cli::config::{require_xavier_token, resolve_base_url, resolve_http_port};
use crate::cli::mcp::start_mcp_stdio;
use crate::cli::server::{add_memory_hierarchical, search_memories_filtered, start_http_server};
use crate::cli::state::Cli;

use anyhow::Result;

use xavier::memory::qmd_memory::MemoryDocument;

impl Cli {
    /// Run the selected subcommand.
    pub async fn run(&self) -> Result<()> {
        use enums::Command;

        match self.cmd.as_ref().unwrap_or(&Command::Http {
            port: None,
            mcp_port: None,
            no_ui: false,
            http: false,
            host: None,
        }) {
            Command::Http {
                port,
                mcp_port,
                no_ui,
                http: _,
                host,
            } => {
                // The bind address is read from XAVIER_HOST (cli::config); the
                // previous XAVIER_HTTP_HOST had no reader, so --host was a no-op.
                if let Some(ref h) = host {
                    std::env::set_var("XAVIER_HOST", h);
                }
                let port = port.unwrap_or_else(resolve_http_port);
                start_http_server(port, *mcp_port, *no_ui).await
            }
            Command::Init(ref args) => installer::run_installer(args.clone()),
            Command::Mcp => start_mcp_stdio().await,
            Command::IndexSelf => {
                println!("IndexSelf is not yet implemented.");
                Ok(())
            }
            Command::Search {
                query,
                limit,
                max_results,
                cluster,
                level,
                offline_ok,
            } => {
                let base_url = resolve_base_url();
                println!("Searching memories via HTTP API on {}", base_url);
                // Prefer --max-results / -n flag over positional limit
                let lim = (*max_results).or(*limit).unwrap_or(10);
                search_memories_filtered(query, lim, cluster.clone(), level.clone(), *offline_ok)
                    .await
            }
            Command::Chat {
                prompt,
                agent,
                interactive,
                json,
                limit,
                model,
            } => {
                crate::cli::handlers::chat::handle_chat_command(
                    prompt.clone(),
                    agent.clone(),
                    *interactive,
                    *json,
                    *limit,
                    model.clone(),
                )
                .await
            }
            Command::Ask {
                prompt,
                agent,
                json,
                limit,
                model,
            } => {
                crate::cli::handlers::chat::handle_chat_command(
                    Some(prompt.clone()),
                    agent.clone(),
                    false,
                    *json,
                    *limit,
                    model.clone(),
                )
                .await
            }
            Command::Usage { cmd } => usage::handle_usage_command(cmd.clone()).await,
            Command::Add {
                content,
                title,
                kind,
                cluster,
                level,
                relation,
            } => {
                println!("Adding memory...");
                add_memory_hierarchical(
                    content,
                    title.as_ref().map(|s| s.as_str()),
                    kind.as_deref(),
                    cluster.as_deref(),
                    level.as_deref(),
                    relation.as_deref(),
                )
                .await
            }
            Command::Recall {
                query,
                limit,
                offline_ok,
            } => http::recall_memories(query, *limit, *offline_ok).await,
            Command::ExportPack {
                topic,
                max_level,
                out,
            } => http::export_context_pack(topic, *max_level, out).await,
            Command::Stats { offline_ok } => {
                println!("Fetching Xavier statistics...");
                http::show_stats(*offline_ok).await
            }
            Command::Reindex => {
                println!("Re-indexing memories missing embeddings...");
                http::reindex_memories().await
            }
            Command::Code { cmd } => code::handle_code_command(cmd.clone()).await,
            Command::Repo { cmd } => match cmd {
                RepoCommand::Config { cmd } => repo::run_repo_config_command(cmd.clone()),
            },
            Command::Telecom(args) => telecom::execute_telecom_command(args.clone()).await,
            Command::Exec {
                command,
                session,
                cwd,
            } => {
                if command.is_empty() {
                    anyhow::bail!("No command specified. Usage: xavier exec <command> [args...]");
                }
                let full_cmd = command.join(" ");
                let res = crate::kernel::runner::execute_rtk_command(
                    &full_cmd,
                    cwd.as_deref(),
                    session.as_deref(),
                )
                .await?;
                println!("{}", res.output);
                eprintln!(
                    "\n[xavier-proxy] tokens: raw ≈ {}, filtered ≈ {} | saved: {} ({:.1}%) in {}ms",
                    res.estimated_raw_tokens,
                    res.estimated_filtered_tokens,
                    res.tokens_saved,
                    res.savings_percentage,
                    res.duration_ms
                );
                if res.exit_code != 0 {
                    std::process::exit(res.exit_code);
                }
                Ok(())
            }
            Command::Issue { cmd } => match cmd {
                IssueCommand::Pack { id, repo } => {
                    crate::cli::handlers::issue::handle_issue_pack(id, repo.clone()).await
                }
            },
            Command::Ls { path } => navigation::handle_ls(path.clone()).await,
            Command::Cd { path } => navigation::handle_cd(path.clone()).await,
            Command::Pwd => navigation::handle_pwd().await,
            Command::Nav { cmd } => match cmd {
                NavCommand::Ls { path } => navigation::handle_ls(path.clone()).await,
                NavCommand::Cd { path } => navigation::handle_cd(path.clone()).await,
                NavCommand::Pwd => navigation::handle_pwd().await,
                NavCommand::Affected {
                    path,
                    depth,
                    format,
                    exclude_file_type,
                } => {
                    navigation::handle_affected(
                        path.clone(),
                        *depth,
                        format.clone(),
                        exclude_file_type.clone(),
                    )
                    .await
                }
                NavCommand::Visualize {
                    format,
                    hotspots,
                    tree,
                    output,
                } => {
                    navigation::handle_visualize(format.clone(), *hotspots, *tree, output.clone())
                        .await
                }
                NavCommand::Telemetry { kind } => navigation::handle_telemetry(kind.clone()).await,
            },
            Command::SessionSave {
                session_id,
                content,
            } => http::session_save(session_id, content).await,
            Command::Spawn {
                count,
                provider,
                model,
                skills,
                context,
                task,
            } => {
                spawn::spawn_agents(
                    *count,
                    provider.clone(),
                    model.clone(),
                    skills,
                    context,
                    task.as_deref(),
                )
                .await
            }
            Command::MultiSpawn {
                agents,
                batch,
                provider,
                model,
                skills,
                task,
            } => {
                spawn::multi_spawn_agents(
                    *agents,
                    *batch,
                    provider.clone(),
                    model.clone(),
                    skills.clone(),
                    task.as_deref(),
                )
                .await
            }
            Command::Swarm { config, parallel } => {
                spawn::run_swarm(config.clone(), *parallel).await
            }
            Command::Chronicle { cmd } => {
                xavier::chronicle::cli::handle_chronicle_command(cmd.clone()).await
            }
            Command::Token { cmd } => token::handle_token_command(cmd.clone()).await,
            Command::Users { cmd } => users::handle_users_command(cmd.clone()).await,
            Command::Provider { cmd } => provider::handle_provider_command(cmd.clone()).await,
            Command::Setup { local } => crate::cli::handlers::setup::handle_setup(*local).await,
            Command::Doctor { format, verbose } => {
                crate::cli::handlers::doctor::handle_doctor(format.clone(), *verbose).await
            }
            Command::DataCommons { cmd } => {
                data_commons::handle_data_commons_command(cmd.clone()).await
            }
            Command::Governance { command } => governance::handle_governance(command.clone()).await,
            Command::Wallet { cmd } => wallet::handle_wallet_command(cmd.clone()).await,
            Command::Session { cmd } => session::handle_session_command(cmd.clone()).await,
            Command::Mesh { cmd } => mesh::handle_mesh_command(cmd.clone()).await,
            Command::Node { cmd } => node::handle_node_command(cmd.clone()).await,
            Command::Nodes { cmd } => nodes::handle_nodes_command(cmd.clone()).await,
            Command::Secrets { cmd } => secrets::handle_secrets_command(cmd.clone()).await,
            Command::Vault { cmd } => secrets::handle_vault_command(cmd.clone()).await,
            Command::Keys { cmd } => secrets::handle_keys_command(cmd.clone()).await,
            Command::Quota => crate::cli::handlers::quota::handle_quota_command().await,
            Command::Tasks { cmd } => tasks::handle_tasks_command(cmd.clone()).await,
            Command::Billing => crate::cli::handlers::billing::handle_billing_command().await,
            Command::Task { cmd } => {
                crate::cli::handlers::tasks::handle_task_command(cmd.clone()).await
            }
            Command::Sync { cmd: _ } => crate::cli::handlers::sync::handle_sync_command().await,
            Command::Verify { cmd } => verify::handle_verify_command(cmd.clone()).await,
            Command::Cloud { cmd } => {
                crate::cli::handlers::cloud::handle_cloud_command(cmd.clone()).await
            }
            Command::Agent { cmd } => {
                crate::cli::handlers::agent_cli::handle_agent_command(cmd.clone()).await
            }
            Command::Plugin { cmd } => match cmd {
                PluginCommand::Install { name } => {
                    crate::cli::handlers::plugins::install_plugin(name.clone()).await
                }
                PluginCommand::List => crate::cli::handlers::plugins::list_plugins().await,
            },
            Command::MiniExpert { cmd } => {
                crate::cli::handlers::mini_experts::handle_mini_expert_command(cmd.clone()).await
            }
            Command::Scan { cmd } => {
                crate::cli::handlers::system_scan_cli::handle_scan_command(cmd.clone()).await
            }
            Command::License { cmd } => {
                crate::cli::commands::license::handle_license_command(cmd.clone()).await
            }
            Command::Memory { cmd } => memory::handle_memory_command(cmd.clone()).await,
            Command::Export {
                public,
                output,
                limit,
            } => {
                let base_url = resolve_base_url();
                let token = require_xavier_token()?;
                let client = enums::CLI_HTTP_CLIENT.clone();

                let limit = limit.unwrap_or(1000).clamp(1, 10000);
                println!(
                    "Exporting memories (public_only={}, limit={})...",
                    public, limit
                );
                let resp = client
                    .get(format!(
                        "{}/memory/export?public={}&limit={}",
                        base_url, public, limit
                    ))
                    .header("X-Xavier-Token", &token)
                    .send()
                    .await?;

                if resp.status().is_success() {
                    let docs: Vec<MemoryDocument> = resp.json().await?;
                    let json = serde_json::to_string_pretty(&docs)?;

                    if let Some(path) = output {
                        std::fs::write(path, json)?;
                        println!("✅ Exported {} memories to {}", docs.len(), path.display());
                    } else {
                        println!("{}", json);
                        println!("\n✅ Exported {} memories to stdout", docs.len());
                    }
                } else {
                    println!("❌ Export failed: {}", resp.text().await?);
                }
                Ok(())
            }
            Command::Maturity { cmd } => {
                xavier::maturity::cli::handle_maturity_command(cmd.clone()).await
            }
            Command::Health { cloud } => {
                crate::cli::handlers::system::handle_health_command(*cloud).await
            }
            Command::Improve { ci, cmd } => improve::handle_improve_command(*ci, cmd.clone()).await,
            Command::Regen { cmd } => regen::handle_regen_command(cmd.clone()).await,
            Command::Cleanup {
                dry_run,
                apply,
                days,
            } => cleanup::handle_cleanup(*dry_run, *apply, *days).await,
            Command::EncryptRecords { dry_run, apply } => {
                encrypt_records::handle_encrypt_records(*dry_run, *apply).await
            }
            Command::MirrorExport { out, limit, since } => {
                mirror::handle_mirror_export(out.clone(), *limit, since.clone()).await
            }
            Command::MirrorImport { input } => mirror::handle_mirror_import(input.clone()).await,
            Command::Airgap(args) => airgap::handle_airgap_command(args.clone()).await,
            Command::Skills { cmd } => handle_skills_command(cmd.clone()).await,
        }
    }
}

/// Read and parse the skill manifest at `path`; blocking IO never runs on the async worker.
async fn read_skill_manifest(path: String) -> Result<xavier::context::SkillManifest> {
    tokio::task::spawn_blocking(move || {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("cannot read manifest '{path}': {e}"))?;
        xavier::context::SkillManifest::from_toml(&content)
            .map_err(|e| anyhow::anyhow!("invalid manifest '{path}': {e}"))
    })
    .await
    .map_err(|e| anyhow::anyhow!("manifest read task failed: {e}"))?
}

/// The policy allowlist and resolved roots come straight from the manifest's own declared
/// source/target directories (D5), matching the HTTP skill handlers in
/// `crate::cli::handlers::skills`.
fn skill_roots_and_policy(
    manifest: &xavier::context::SkillManifest,
) -> Result<(
    xavier::context::SkillResolvedRoots,
    xavier::context::SkillFsPolicy,
)> {
    let sources: Vec<std::path::PathBuf> = manifest
        .sources
        .values()
        .map(std::path::PathBuf::from)
        .collect();
    let targets: Vec<std::path::PathBuf> = manifest
        .targets
        .iter()
        .map(|t| std::path::PathBuf::from(&t.root))
        .collect();
    let policy = xavier::context::SkillFsPolicy::new(&sources, &targets)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let roots = xavier::context::SkillResolvedRoots {
        sources: manifest
            .sources
            .iter()
            .map(|(id, dir)| (id.clone(), std::path::PathBuf::from(dir)))
            .collect(),
        targets: manifest
            .targets
            .iter()
            .map(|t| (t.id.clone(), std::path::PathBuf::from(&t.root)))
            .collect(),
    };
    Ok((roots, policy))
}

/// `ApplyReport`/`RollbackReport` and their per-entry outcomes intentionally do not derive
/// `Serialize` in the controller crate, so render them to JSON here (mirrors
/// `crate::cli::handlers::skills::render_apply_report`/`render_rollback_report`).
fn entry_outcome_json(outcome: &xavier::context::EntryOutcome) -> serde_json::Value {
    use xavier::context::EntryOutcome;
    match outcome {
        EntryOutcome::Created => serde_json::json!({"kind": "created"}),
        EntryOutcome::Refreshed => serde_json::json!({"kind": "refreshed"}),
        EntryOutcome::Unchanged => serde_json::json!({"kind": "unchanged"}),
        EntryOutcome::SkippedConflict => serde_json::json!({
            "kind": "conflict",
            "detail": "destination is not controller-owned; remove or rename it, then plan again",
        }),
        EntryOutcome::BlockedSecret(summary) => serde_json::json!({
            "kind": "blocked_secret",
            "detail": format!(
                "publication blocked: {summary}; remove the secret from the source package, then plan again"
            ),
        }),
    }
}

fn render_apply_report(report: &xavier::context::ApplyReport) -> serde_json::Value {
    let applied: Vec<serde_json::Value> = report
        .applied
        .iter()
        .map(|(name, target, outcome)| {
            serde_json::json!({
                "name": name,
                "target": target,
                "outcome": entry_outcome_json(outcome),
            })
        })
        .collect();
    let removed: Vec<serde_json::Value> = report
        .removed
        .iter()
        .map(|(name, target)| serde_json::json!({"name": name, "target": target}))
        .collect();
    serde_json::json!({
        "tx_id": report.tx_id,
        "dry_run": report.dry_run,
        "applied": applied,
        "removed": removed,
    })
}

fn rollback_outcome_json(outcome: &xavier::context::RollbackOutcome) -> serde_json::Value {
    use xavier::context::RollbackOutcome;
    match outcome {
        RollbackOutcome::RestoredLink { to } => {
            serde_json::json!({"kind": "restored_link", "to": to.to_string_lossy()})
        }
        RollbackOutcome::RestoredCopy => serde_json::json!({"kind": "restored_copy"}),
        RollbackOutcome::Removed => serde_json::json!({"kind": "removed"}),
        RollbackOutcome::AlreadyRolledBack => serde_json::json!({"kind": "already_rolled_back"}),
        RollbackOutcome::Conflict(detail) => serde_json::json!({
            "kind": "conflict",
            "detail": format!("{detail}; left untouched, resolve manually then roll back again"),
        }),
    }
}

fn render_rollback_report(report: &xavier::context::RollbackReport) -> serde_json::Value {
    let entries: Vec<serde_json::Value> = report
        .entries
        .iter()
        .map(|(path, outcome)| {
            serde_json::json!({
                "path": path.to_string_lossy(),
                "outcome": rollback_outcome_json(outcome),
            })
        })
        .collect();
    serde_json::json!({"tx_id": report.tx_id, "dry_run": report.dry_run, "entries": entries})
}

/// `xavier skills plan|apply|rollback`: the skill controller's only mutation surface (D4).
/// This calls the controller directly rather than the HTTP handlers in
/// `crate::cli::handlers::skills`, so no new network mutation route is added (D6).
async fn handle_skills_command(cmd: enums::SkillsCommand) -> Result<()> {
    match cmd {
        enums::SkillsCommand::Plan { manifest } => {
            let manifest = read_skill_manifest(manifest).await?;
            let (roots, policy) = skill_roots_and_policy(&manifest)?;
            let plan = xavier::context::build_skill_plan(&manifest, &policy, &roots)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            println!("{}", serde_json::to_string_pretty(&plan)?);
            Ok(())
        }
        enums::SkillsCommand::Apply {
            manifest,
            plan,
            apply,
        } => {
            let plan_path = plan;
            let plan: xavier::context::ProjectionPlan = tokio::task::spawn_blocking({
                let plan_path = plan_path.clone();
                move || -> Result<xavier::context::ProjectionPlan> {
                    let content = std::fs::read_to_string(&plan_path)
                        .map_err(|e| anyhow::anyhow!("no saved plan at '{plan_path}': {e}"))?;
                    serde_json::from_str(&content).map_err(|e| {
                        anyhow::anyhow!("saved plan at '{plan_path}' is not valid: {e}")
                    })
                }
            })
            .await
            .map_err(|e| anyhow::anyhow!("plan read task failed: {e}"))??;
            let manifest = read_skill_manifest(manifest).await?;
            let (roots, policy) = skill_roots_and_policy(&manifest)?;
            let journal = xavier::context::SkillJournal::from_settings()
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            let tx_id = uuid::Uuid::new_v4().to_string();
            let dry_run = !apply;
            let report = tokio::task::spawn_blocking(move || {
                xavier::context::apply_skill_plan(
                    &plan, &manifest, &policy, &roots, &journal, &tx_id, dry_run,
                )
            })
            .await
            .map_err(|e| anyhow::anyhow!("apply task failed: {e}"))?
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&render_apply_report(&report))?
            );
            Ok(())
        }
        enums::SkillsCommand::Rollback {
            manifest,
            tx_id,
            apply,
        } => {
            let manifest = read_skill_manifest(manifest).await?;
            let (_roots, policy) = skill_roots_and_policy(&manifest)?;
            let journal = xavier::context::SkillJournal::from_settings()
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            let dry_run = !apply;
            let report = tokio::task::spawn_blocking(move || {
                xavier::context::rollback_transaction(&journal, &policy, &tx_id, dry_run)
            })
            .await
            .map_err(|e| anyhow::anyhow!("rollback task failed: {e}"))?
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&render_rollback_report(&report))?
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod skills_command_tests {
    use super::*;
    use clap::CommandFactory;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn skills_help_lists_plan_apply_rollback() {
        let cli = crate::cli::state::Cli::command();
        let skills = cli
            .get_subcommands()
            .find(|c| c.get_name() == "skills")
            .expect("skills subcommand is registered");
        let names: Vec<&str> = skills.get_subcommands().map(|c| c.get_name()).collect();
        assert!(names.contains(&"plan"), "missing plan: {names:?}");
        assert!(names.contains(&"apply"), "missing apply: {names:?}");
        assert!(names.contains(&"rollback"), "missing rollback: {names:?}");
    }

    fn write_package(source: &Path, name: &str) {
        let dir = source.join(name);
        fs::create_dir_all(&dir).expect("mkdir package");
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\n---\n\nBody.\n"),
        )
        .expect("write SKILL.md");
    }

    fn manifest_toml(source: &Path, target: &Path) -> String {
        format!(
            "version = 1\n[sources]\ncanonical = \"{}\"\n\
             [[targets]]\nid = \"t1\"\ntools = [\"codex\"]\nroot = \"{}\"\nmode = \"symlink\"\n\
             [[placements]]\nsource = \"canonical\"\npath = \"demo\"\nname = \"demo\"\ntargets = [\"t1\"]\n",
            source.display(),
            target.display(),
        )
    }

    #[tokio::test]
    async fn skills_plan_writes_nothing() {
        let tmp = TempDir::new().expect("tmp");
        let source = tmp.path().join("store");
        let target = tmp.path().join("tools/skills");
        fs::create_dir_all(&source).expect("mkdir source");
        fs::create_dir_all(&target).expect("mkdir target");
        write_package(&source, "demo");
        let manifest_path = tmp.path().join("manifest.toml");
        fs::write(&manifest_path, manifest_toml(&source, &target)).expect("write manifest");

        handle_skills_command(enums::SkillsCommand::Plan {
            manifest: manifest_path.to_string_lossy().into_owned(),
        })
        .await
        .expect("plan succeeds");

        assert!(
            fs::read_dir(&target).expect("read target").next().is_none(),
            "plan must not write to the target root"
        );
    }

    #[tokio::test]
    async fn skills_apply_defaults_to_dry_run_then_rollback_round_trips() {
        let _guard = crate::context::skill_registry::env_lock().lock().await;
        let previous_data_dir = std::env::var("XAVIER_DATA_DIR").ok();

        let tmp = TempDir::new().expect("tmp");
        std::env::set_var("XAVIER_DATA_DIR", tmp.path().join("journal"));

        let source = tmp.path().join("store");
        let target = tmp.path().join("tools/skills");
        fs::create_dir_all(&source).expect("mkdir source");
        fs::create_dir_all(&target).expect("mkdir target");
        write_package(&source, "demo");
        let manifest_path = tmp.path().join("manifest.toml");
        fs::write(&manifest_path, manifest_toml(&source, &target)).expect("write manifest");

        let manifest_obj = read_skill_manifest(manifest_path.to_string_lossy().into_owned())
            .await
            .expect("read manifest");
        let (roots, policy) = skill_roots_and_policy(&manifest_obj).expect("roots and policy");
        let plan =
            xavier::context::build_skill_plan(&manifest_obj, &policy, &roots).expect("build plan");
        let plan_path = tmp.path().join("plan.json");
        fs::write(
            &plan_path,
            serde_json::to_string(&plan).expect("serialize plan"),
        )
        .expect("write plan");

        let dest = target.join("demo");

        handle_skills_command(enums::SkillsCommand::Apply {
            manifest: manifest_path.to_string_lossy().into_owned(),
            plan: plan_path.to_string_lossy().into_owned(),
            apply: false,
        })
        .await
        .expect("dry-run apply succeeds");
        assert!(
            fs::symlink_metadata(&dest).is_err(),
            "apply without --apply must not mutate the target (D4)"
        );

        handle_skills_command(enums::SkillsCommand::Apply {
            manifest: manifest_path.to_string_lossy().into_owned(),
            plan: plan_path.to_string_lossy().into_owned(),
            apply: true,
        })
        .await
        .expect("apply succeeds");
        assert!(
            fs::symlink_metadata(&dest)
                .expect("dest metadata")
                .file_type()
                .is_symlink(),
            "apply --apply must publish an owned symlink"
        );

        let journal = xavier::context::SkillJournal::from_settings().expect("journal");
        let tx_id = journal
            .list_transactions()
            .expect("list transactions")
            .into_iter()
            .next()
            .expect("one transaction recorded")
            .id;

        handle_skills_command(enums::SkillsCommand::Rollback {
            manifest: manifest_path.to_string_lossy().into_owned(),
            tx_id: tx_id.clone(),
            apply: false,
        })
        .await
        .expect("dry-run rollback succeeds");
        assert!(
            fs::symlink_metadata(&dest).is_ok(),
            "rollback without --apply must not mutate the target (D4)"
        );

        handle_skills_command(enums::SkillsCommand::Rollback {
            manifest: manifest_path.to_string_lossy().into_owned(),
            tx_id,
            apply: true,
        })
        .await
        .expect("apply rollback succeeds");
        assert!(
            fs::symlink_metadata(&dest).is_err(),
            "apply rollback must remove the entry created by apply (D14)"
        );

        match previous_data_dir {
            Some(v) => std::env::set_var("XAVIER_DATA_DIR", v),
            None => std::env::remove_var("XAVIER_DATA_DIR"),
        }
    }
}
