//! Cloud management handlers for Xavier.

use anyhow::Result;
use colored::*;
use serde_json::json;

use crate::cli::commands::enums::{CloudCommand, CLI_HTTP_CLIENT};
use crate::cli::config::{require_xavier_token, resolve_base_url};
use crate::settings::XavierSettings;
use crate::sync::xavier_cloud::{self as cloud_backup, CloudBackupConfig};

/// Handle cloud commands.
pub async fn handle_cloud_command(cmd: CloudCommand) -> Result<()> {
    match cmd {
        CloudCommand::Status { json } => handle_cloud_status(json).await,
        CloudCommand::SetBackend { backend, json } => handle_cloud_set_backend(backend, json).await,
        CloudCommand::Sync {
            passphrase_file,
            allow_shrink,
            json,
        }
        | CloudCommand::Backup {
            passphrase_file,
            allow_shrink,
            json,
        } => handle_cloud_backup(passphrase_file, allow_shrink, json).await,
        CloudCommand::Restore {
            instance,
            passphrase_file,
            json,
        } => handle_cloud_restore(instance, passphrase_file, json).await,
        CloudCommand::Verify {
            deep,
            passphrase_file,
            json,
        } => handle_cloud_verify(deep, passphrase_file, json).await,
    }
}

/// Show cloud backend status, connection, and sync stats.
pub async fn handle_cloud_status(as_json: bool) -> Result<()> {
    let settings = XavierSettings::current();
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    // 1. Local backend info
    let local_backend = settings.memory.backend.clone();

    // 2. Cloud info from server
    let cloud_resp = client
        .get(format!("{}/v1/mesh/cloud", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await;

    let cloud_info = if let Ok(resp) = cloud_resp {
        if resp.status().is_success() {
            resp.json::<serde_json::Value>().await.ok()
        } else {
            None
        }
    } else {
        None
    };

    // 3. Stats from server
    let stats_resp = client
        .get(format!("{}/memory/stats", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await;

    let stats = if let Ok(resp) = stats_resp {
        if resp.status().is_success() {
            resp.json::<serde_json::Value>().await.ok()
        } else {
            None
        }
    } else {
        None
    };

    if as_json {
        let output = json!({
            "status": "ok",
            "local_backend": local_backend,
            "cloud_info": cloud_info,
            "stats": stats,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!();
        println!("═══════════════════════════════════════════");
        println!("  {} ", "Cloud Backend Status".bold());
        println!("═══════════════════════════════════════════");
        println!("  {:<15} {}", "Active Backend:", local_backend.cyan());

        if let Some(ci) = &cloud_info {
            let url = ci["url"].as_str().unwrap_or("None");
            let instance = ci["instance_id"].as_str().unwrap_or("None");
            println!("  {:<15} {}", "Cloud URL:", url);
            println!("  {:<15} {}", "Instance ID:", instance);
            println!("  {:<15} {}", "Connection:", "✅ Connected".green());
        } else {
            println!(
                "  {:<15} {}",
                "Connection:",
                "⚠️ Disconnected / Not Configured".yellow()
            );
        }

        if let Some(s) = &stats {
            let docs = s["document_count"]
                .as_u64()
                .or(s["total_documents"].as_u64())
                .unwrap_or(0);
            println!("  {:<15} {}", "Documents:", docs);
        }

        println!("═══════════════════════════════════════════");
    }

    Ok(())
}

async fn handle_cloud_set_backend(backend: String, as_json: bool) -> Result<()> {
    let mut settings = XavierSettings::current();
    let normalized = backend.trim().to_lowercase();

    match normalized.as_str() {
        "sqlite" | "vec" | "supabase" | "postgres" | "auto" => {
            settings.memory.backend = normalized.clone();
            settings.save().await?;

            if as_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "status": "ok",
                        "message": format!("Backend set to {}", normalized),
                        "backend": normalized
                    }))?
                );
            } else {
                println!(
                    "{} Backend successfully set to: {}",
                    "✅".green(),
                    normalized.cyan().bold()
                );
                println!(
                    "Note: You may need to restart the Xavier server for changes to take effect."
                );
            }
        }
        _ => {
            let error_msg = format!(
                "Invalid backend: {}. Supported: sqlite, vec, supabase, postgres, auto",
                backend
            );
            if as_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "status": "error",
                        "message": error_msg
                    }))?
                );
            } else {
                println!("{} {}", "❌".red(), error_msg.red());
            }
        }
    }

    Ok(())
}

fn backup_config() -> Result<CloudBackupConfig> {
    CloudBackupConfig::from_env().ok_or_else(|| {
        anyhow::anyhow!(
            "Xavier Cloud is not configured: set PGHEART_URL and PGHEART_TOKEN \
             (aliases XAVIER_PGHEART_URL / XAVIER_PGHEART_TOKEN) and optionally PGHEART_INSTANCE_ID"
        )
    })
}

/// Passphrase from `--passphrase-file`, else the environment. Never prompts
/// and never falls back to uploading plaintext.
fn backup_passphrase(file: Option<&std::path::Path>) -> Result<zeroize::Zeroizing<String>> {
    if let Some(path) = file {
        return cloud_backup::passphrase_from_file(path);
    }
    cloud_backup::passphrase_from_env()?.ok_or_else(|| {
        anyhow::anyhow!(
            "no cloud backup passphrase: set {} or {} (or pass --passphrase-file). \
             Backups are encrypted on this device with it; keep it safe, it cannot be recovered.",
            cloud_backup::PASSPHRASE_ENV,
            cloud_backup::PASSPHRASE_FILE_ENV
        )
    })
}

fn fmt_bytes(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.2} MiB ({} bytes)", n as f64 / 1048576.0, n)
    } else if n >= 1024 {
        format!("{:.1} KiB ({} bytes)", n as f64 / 1024.0, n)
    } else {
        format!("{} bytes", n)
    }
}

fn print_usage(u: &cloud_backup::CloudUsage) {
    println!(
        "  {:<22} {} in {} chunks (tenant {}, {:.3}% of quota)",
        "Cloud usage:",
        fmt_bytes(u.bytes),
        u.chunks,
        u.tenant,
        u.percent
    );
}

/// Run one encrypted backup of the local store. Shared by `cloud backup`,
/// `cloud sync` and `sync now`.
pub async fn run_cli_backup(
    passphrase_file: Option<&std::path::Path>,
    allow_shrink: bool,
) -> Result<cloud_backup::BackupReport> {
    let cfg = backup_config()?;
    let pass = backup_passphrase(passphrase_file)?;
    let store = crate::memory::sqlite_vec_store::VecSqliteMemoryStore::from_env().await?;
    let remote = cloud_backup::HttpBlobStore::new(&cfg)?;
    cloud_backup::run_backup(
        &store,
        &remote,
        &cfg,
        &pass,
        cloud_backup::KdfParams::new_default(),
        allow_shrink,
    )
    .await
}

/// Human-readable backup report. Returns an error when the backup is incomplete.
pub fn print_backup_report(r: &cloud_backup::BackupReport, as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(r)?);
    } else {
        println!();
        println!("═══════════════════════════════════════════");
        println!("  {} ", "Xavier Cloud backup (encrypted)".bold());
        println!("═══════════════════════════════════════════");
        println!("  {:<22} {}", "Instance:", r.instance_id);
        println!(
            "  {:<22} {} of {} ({} workspaces)",
            "Records backed up:", r.records_backed_up, r.records_total, r.workspaces
        );
        println!(
            "  {:<22} {} uploaded, {} unchanged (of {})",
            "Packs:", r.packs_uploaded, r.packs_unchanged, r.packs_total
        );
        println!(
            "  {:<22} {}",
            "Uploaded this run:",
            fmt_bytes(r.bytes_uploaded)
        );
        println!(
            "  {:<22} {}",
            "Backup size:",
            fmt_bytes(r.backup_bytes_total)
        );
        match &r.usage_after {
            Some(u) => print_usage(u),
            None => println!("  {:<22} {}", "Cloud usage:", "unavailable".yellow()),
        }
        if !r.manifest_published {
            println!(
                "  {} incomplete run: the previous complete backup was left in place",
                "⚠️".yellow()
            );
        }
        if r.packs_repaired > 0 {
            println!("  {:<22} {}", "Packs repaired:", r.packs_repaired);
        }
        if r.records_locked > 0 {
            println!(
                "  {} {} private record(s) are locked on this node and were NOT backed up (unlock the node and run again)",
                "⚠️".yellow(),
                r.records_locked
            );
        }
        if r.records_total == 0 {
            println!(
                "  {} the local store has no memories: nothing to back up",
                "ℹ️".cyan()
            );
        }
        println!("═══════════════════════════════════════════");
    }
    if !r.complete {
        anyhow::bail!(
            "cloud backup incomplete: {} locked record(s) not uploaded",
            r.records_locked
        );
    }
    Ok(())
}

async fn handle_cloud_backup(
    passphrase_file: Option<std::path::PathBuf>,
    allow_shrink: bool,
    as_json: bool,
) -> Result<()> {
    if !as_json {
        println!(
            "{} Encrypting and uploading memories to Xavier Cloud...",
            "🔐".cyan()
        );
    }
    let report = run_cli_backup(passphrase_file.as_deref(), allow_shrink).await?;
    print_backup_report(&report, as_json)
}

async fn handle_cloud_restore(
    instance: Option<String>,
    passphrase_file: Option<std::path::PathBuf>,
    as_json: bool,
) -> Result<()> {
    let cfg = backup_config()?;
    let pass = backup_passphrase(passphrase_file.as_deref())?;
    let instance = instance.unwrap_or_else(|| cfg.instance_id.clone());
    let store = crate::memory::sqlite_vec_store::VecSqliteMemoryStore::from_env().await?;
    let remote = cloud_backup::HttpBlobStore::new(&cfg)?;
    let r = cloud_backup::run_restore(&store, &remote, &instance, &pass).await?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&r)?);
    } else {
        println!();
        println!("═══════════════════════════════════════════");
        println!("  {} ", "Xavier Cloud restore".bold());
        println!("═══════════════════════════════════════════");
        println!("  {:<22} {}", "Instance:", r.instance_id);
        if let Some(t) = r.backup_created_at {
            println!("  {:<22} {}", "Backup taken at:", t.to_rfc3339());
        }
        println!("  {:<22} {}", "Records in backup:", r.records_in_backup);
        if !r.backup_complete {
            println!(
                "  {:<22} {}",
                "Backup complete:",
                "no (locked records were left out)".yellow()
            );
        }
        println!("  {:<22} {}", "Restored:", r.restored.to_string().green());
        println!("  {:<22} {}", "Kept (local newer):", r.skipped_newer_local);
        println!(
            "  {:<22} {} packs, {}",
            "Downloaded:",
            r.packs,
            fmt_bytes(r.bytes_downloaded)
        );
        println!("  Restored records have no embeddings yet (status \"pending\"); they are re-embedded locally.");
        println!("═══════════════════════════════════════════");
    }
    Ok(())
}

async fn handle_cloud_verify(
    deep: bool,
    passphrase_file: Option<std::path::PathBuf>,
    as_json: bool,
) -> Result<()> {
    let cfg = backup_config()?;
    let remote = cloud_backup::HttpBlobStore::new(&cfg)?;
    let health = remote.health().await;
    let pass = if deep {
        Some(backup_passphrase(passphrase_file.as_deref())?)
    } else {
        None
    };
    let v = cloud_backup::run_verify(
        &remote,
        &cfg.instance_id,
        pass.as_deref().map(|s| s.as_str()),
    )
    .await?;
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "worker_health": health.as_ref().ok(),
                "url": cfg.url,
                "verify": v,
            }))?
        );
    } else {
        println!();
        println!("═══════════════════════════════════════════");
        println!("  {} ", "Xavier Cloud backup check".bold());
        println!("═══════════════════════════════════════════");
        println!("  {:<22} {}", "Worker:", cfg.url);
        println!(
            "  {:<22} {}",
            "Health:",
            match &health {
                Ok(_) => "ok".green(),
                Err(e) => e.to_string().red(),
            }
        );
        println!("  {:<22} {}", "Instance:", v.instance_id);
        if let Some(u) = &v.usage {
            print_usage(u);
        }
        if !v.manifest_found {
            println!(
                "  {:<22} {}",
                "Backup:",
                "none yet (run `xavier cloud backup`)".yellow()
            );
        } else {
            if let Some(t) = v.backup_created_at {
                println!("  {:<22} {}", "Latest backup:", t.to_rfc3339());
            }
            println!("  {:<22} {} in {} packs", "Records:", v.records, v.packs);
            if v.packs_missing.is_empty() {
                println!("  {:<22} {}", "Packs present:", "all".green());
            } else {
                println!(
                    "  {:<22} {}",
                    "Packs missing:",
                    v.packs_missing.len().to_string().red()
                );
            }
            if v.manifest_authenticated != Some(true) {
                println!(
                    "  {:<22} {}",
                    "Manifest:",
                    "not authenticated (use --deep)".yellow()
                );
            }
            match v.decrypted_ok {
                Some(true) => println!("  {:<22} {}", "Decrypt check:", "ok".green()),
                Some(false) => println!("  {:<22} {}", "Decrypt check:", "FAILED".red()),
                None => println!("  {:<22} not run (use --deep)", "Decrypt check:"),
            }
        }
        println!("═══════════════════════════════════════════");
    }
    if v.manifest_found && !v.ok {
        anyhow::bail!("the cloud backup failed verification");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::cli::commands::enums::{CloudCommand, Command};
    use clap::Parser;

    fn parse(args: &[&str]) -> CloudCommand {
        match crate::cli::state::Cli::try_parse_from(args)
            .expect("cloud args parse")
            .cmd
        {
            Some(Command::Cloud { cmd }) => cmd,
            _ => panic!("not a cloud command"),
        }
    }

    #[test]
    fn allow_shrink_is_opt_in_for_backup_and_sync() {
        for sub in ["backup", "sync"] {
            let flag = |args: &[&str]| match parse(args) {
                CloudCommand::Backup { allow_shrink, .. }
                | CloudCommand::Sync { allow_shrink, .. } => allow_shrink,
                _ => panic!("unexpected command"),
            };
            assert!(!flag(&["xavier", "cloud", sub]));
            assert!(flag(&["xavier", "cloud", sub, "--allow-shrink"]));
        }
    }
}
