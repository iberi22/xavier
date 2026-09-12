//! CLI code graph query command
//!
//! Handles the `xavier code` subcommand which queries Xavier's code graph
//! for symbol discovery, dependency analysis, and complexity metrics.

use crate::cli::codegraph_sync::{sync_codegraph_from_git, GitSyncOptions};
use crate::cli::commands::enums::{CodeCommand, CODE_HTTP_CLIENT};
use crate::cli::config::{require_xavier_token, resolve_base_url};
use xavier::codebase::codegraph_sidecar::{
    ensure_codegraph_sidecar, install_codegraph_sidecar_sync, EnsureOptions, InstallMode,
    SidecarInstallOutcome,
};

use anyhow::{bail, Result};
use std::path::PathBuf;

/// Dispatch a [`CodeCommand`] by making the appropriate HTTP request to the Xavier server.
pub async fn handle_code_command(cmd: CodeCommand) -> Result<()> {
    // Install sidecar runs locally (no HTTP server required).
    if let CodeCommand::Install {
        from_source,
        force: _,
        json,
    } = &cmd
    {
        if !*json {
            eprintln!(
                "[codegraph] Installing CodeGraph sidecar binary (from_source={})...",
                from_source
            );
        }
        let outcome: SidecarInstallOutcome = install_codegraph_sidecar_sync(*from_source)?;
        if *json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "status": if outcome.success { "ok" } else { "error" },
                    "installed_path": outcome.bin_path.as_ref().map(|p| p.display().to_string()),
                    "version": outcome.version,
                    "verified": outcome.verified,
                    "message": outcome.message,
                }))?
            );
        } else {
            println!("CodeGraph sidecar installation complete:");
            if let Some(path) = &outcome.bin_path {
                println!("  Path:     {}", path.display());
            }
            println!("  Version:  {}", outcome.version);
            println!("  Verified: {}", if outcome.verified { "yes" } else { "no" });
            println!("  Status:   {}", outcome.message);
        }
        return Ok(());
    }

    // Git sync runs locally against the CodeGraph DB (no HTTP server required).
    if let CodeCommand::Sync {
        git,
        base,
        staged,
        memory,
    } = &cmd
    {
        if !*git {
            bail!("Usa --git: xavier code sync --git [--base <commit>] [--staged] [--memory]");
        }
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = sync_codegraph_from_git(GitSyncOptions {
            workspace,
            base: base.clone(),
            staged: *staged,
            with_memory: *memory,
        })
        .await?;
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    let token = require_xavier_token()?;
    let base_url = resolve_base_url();
    let client = CODE_HTTP_CLIENT.clone();

    // XAV-01: explicit repo identity derived from the caller's cwd so the
    // server resolves the graph for THIS checkout, never the daemon's
    // startup workspace.
    let repo = xavier::codebase::repo_identity::derive_repo_identity(
        &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    );

    let mut scanned_path = None;
    if let CodeCommand::Scan { path, .. } = &cmd {
        scanned_path = Some(path.clone());
    }

    let response = match cmd {
        CodeCommand::Scan {
            path,
            reprompt_codegraph,
        } => {
            // Consent-first Colby sidecar (TTY on CLI). Soft-fails to native.
            let workspace = PathBuf::from(&path);
            let workspace = std::path::absolute(&workspace).unwrap_or(workspace);
            let mut opts = EnsureOptions::default();
            opts.reprompt = reprompt_codegraph || opts.reprompt;
            if reprompt_codegraph {
                // Force ask path when flag set and mode was "no"
                if opts.install_mode == InstallMode::No {
                    opts.install_mode = InstallMode::Ask;
                }
            }
            let outcome = ensure_codegraph_sidecar(&workspace, opts);
            eprintln!("[codegraph] {}", outcome.message);

            client
                .post(format!("{}/code/scan", base_url))
                .header("X-Xavier-Token", &token)
                .json(&serde_json::json!({
                    "path": path,
                    "codegraph_available": outcome.available,
                    "codegraph_bin": outcome.bin_path.as_ref().map(|p| p.display().to_string()),
                }))
                .send()
                .await?
        }
        CodeCommand::BlastRadius { query, depth } => {
            client
                .post(format!("{}/code/blast-radius", base_url))
                .header("X-Xavier-Token", &token)
                .json(&serde_json::json!({
                    "query": query,
                    "depth": depth,
                }))
                .send()
                .await?
        }
        CodeCommand::Find { query, limit, kind } => client
            .post(format!("{}/code/find", base_url))
            .header("X-Xavier-Token", &token)
            .json(
                &serde_json::json!({ "query": query, "limit": limit, "kind": kind, "repo": repo }),
            )
            .send()
            .await?,
        CodeCommand::Dependencies {
            query,
            depth,
            limit,
            edge_type,
        } => {
            client
                .post(format!("{}/code/dependencies", base_url))
                .header("X-Xavier-Token", &token)
                .json(&serde_json::json!({
                    "query": query,
                    "depth": depth,
                    "limit": limit,
                    "edge_type": edge_type
                }))
                .send()
                .await?
        }
        CodeCommand::ReverseDependencies {
            query,
            depth,
            limit,
            edge_type,
        } => {
            client
                .post(format!("{}/code/reverse-dependencies", base_url))
                .header("X-Xavier-Token", &token)
                .json(&serde_json::json!({
                    "query": query,
                    "depth": depth,
                    "limit": limit,
                    "edge_type": edge_type
                }))
                .send()
                .await?
        }
        CodeCommand::CallChain {
            query,
            depth,
            limit,
        } => {
            client
                .post(format!("{}/code/call-chain", base_url))
                .header("X-Xavier-Token", &token)
                .json(&serde_json::json!({
                    "query": query,
                    "depth": depth,
                    "limit": limit
                }))
                .send()
                .await?
        }
        CodeCommand::Hubs => {
            client
                .get(format!("{}/code/hubs", base_url))
                .header("X-Xavier-Token", &token)
                .send()
                .await?
        }
        CodeCommand::Hotspots => {
            client
                .get(format!("{}/code/hotspots", base_url))
                .header("X-Xavier-Token", &token)
                .send()
                .await?
        }
        CodeCommand::Stats => {
            // XAV-01: explicit repo identity as query params (reqwest here
            // has no `.query()` helper — minimal features — so encode manually).
            let url = format!(
                "{}/code/stats?project_id={}&root={}&indexed_commit={}",
                base_url,
                pct_encode(&repo.project_id),
                pct_encode(&repo.root),
                pct_encode(&repo.indexed_commit),
            );
            client
                .get(url)
                .header("X-Xavier-Token", &token)
                .send()
                .await?
        }
        CodeCommand::Dump { path } => {
            client
                .post(format!("{}/code/dump", base_url))
                .header("X-Xavier-Token", &token)
                .json(&serde_json::json!({ "path": path }))
                .send()
                .await?
        }
        CodeCommand::Load { path } => {
            client
                .post(format!("{}/code/load", base_url))
                .header("X-Xavier-Token", &token)
                .json(&serde_json::json!({ "path": path }))
                .send()
                .await?
        }
        CodeCommand::Install { .. } => unreachable!("Install handled above"),
        CodeCommand::Sync { .. } => unreachable!("Sync handled above"),
    };

    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or_default();
    if status.is_success() {
        let remote_status = body.get("status").and_then(|v| v.as_str()).unwrap_or("ok");
        if remote_status == "error" {
            eprintln!("{}", serde_json::to_string_pretty(&body)?);
            bail!(
                "Code graph request returned status=error: {}",
                body.get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
            );
        }
        println!("{}", serde_json::to_string_pretty(&body)?);
        if let Some(path) = scanned_path {
            if let Err(err) = soft_dump(&client, &base_url, &token, &path).await {
                eprintln!("[warn] Soft-dump failed: {}", err);
            }
        }
        Ok(())
    } else {
        eprintln!("Code graph request failed ({}):", status);
        eprintln!("{}", serde_json::to_string_pretty(&body)?);
        bail!("Code graph HTTP {} — see response body above", status);
    }
}

/// Minimal RFC3986 percent-encoding for query values (unreserved +
/// `/` left intact so repo roots stay readable; `/` is legal in values).
fn pct_encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

async fn soft_dump(
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
    path: &str,
) -> Result<()> {
    let response = client
        .post(format!("{}/code/dump", base_url))
        .header("X-Xavier-Token", token)
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await?;

    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or_default();

    if !status.is_success() {
        let err_msg = body
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error");
        anyhow::bail!("Dump request failed ({}): {}", status, err_msg);
    }

    let resolved_path =
        xavier::codebase::codegraph_paths::codegraph_dump_path_for(std::path::Path::new(path));
    println!("Portable code graph dumped to {}", resolved_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_install_codegraph_sidecar_human_output() {
        let cmd = CodeCommand::Install {
            from_source: false,
            force: false,
            json: false,
        };
        let res = handle_code_command(cmd).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_install_codegraph_sidecar_json_output() {
        let cmd = CodeCommand::Install {
            from_source: true,
            force: true,
            json: true,
        };
        let res = handle_code_command(cmd).await;
        assert!(res.is_ok());
    }
}
