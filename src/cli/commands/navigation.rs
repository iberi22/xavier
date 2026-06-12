//! CLI navigation command handlers (ls, cd, pwd)

use crate::cli::commands::enums::CLI_HTTP_CLIENT;
use crate::cli::config::{require_xavier_token, resolve_base_url, resolve_cwd, save_cwd};
use crate::memory::qmd::types::NavEntry;
use anyhow::{anyhow, Result};

pub async fn handle_ls(path: Option<String>) -> Result<()> {
    let token = require_xavier_token()?;
    let base_url = resolve_base_url();
    let client = CLI_HTTP_CLIENT.clone();

    let cwd = resolve_cwd();
    let effective_path = match path {
        Some(p) if p.starts_with('/') => p,
        Some(p) => if cwd == "/" { format!("/{}", p) } else { format!("{}/{}", cwd, p) },
        None => cwd.clone(),
    };

    let response = client
        .get(format!("{}/v1/nav/ls?path={}", base_url, effective_path))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if response.status().is_success() {
        let body: serde_json::Value = response.json().await?;
        let entries: Vec<NavEntry> = serde_json::from_value(body["entries"].clone())?;

        println!("Contents of {}:", body["path"]);
        if entries.is_empty() {
            println!("  (empty)");
        } else {
            for entry in entries {
                let prefix = if entry.is_dir { "DIR " } else { "DOC " };
                println!("  {} {}", prefix, entry.name);
            }
        }
    } else {
        println!("❌ ls failed: {}", response.text().await?);
    }
    Ok(())
}

pub async fn handle_cd(path: String) -> Result<()> {
    let token = require_xavier_token()?;
    let base_url = resolve_base_url();
    let client = CLI_HTTP_CLIENT.clone();

    let cwd = resolve_cwd();
    let target_path = if path == ".." {
        if cwd == "/" {
            "/".to_string()
        } else {
            let mut parts: Vec<&str> = cwd.split('/').filter(|s| !s.is_empty()).collect();
            if !parts.is_empty() {
                parts.pop();
            }
            if parts.is_empty() {
                "/".to_string()
            } else {
                format!("/{}", parts.join("/"))
            }
        }
    } else if path.starts_with('/') {
        path
    } else if cwd == "/" {
        format!("/{}", path)
    } else {
        format!("{}/{}", cwd, path)
    };

    // Normalize target_path (remove trailing slash except for /)
    let mut normalized_target = target_path;
    if normalized_target.len() > 1 && normalized_target.ends_with('/') {
        normalized_target.pop();
    }

    let response = client
        .post(format!("{}/v1/nav/cd", base_url))
        .header("X-Xavier-Token", &token)
        .json(&serde_json::json!({ "path": normalized_target }))
        .send()
        .await?;

    if response.status().is_success() {
        save_cwd(&normalized_target)?;
        println!("✅ Current directory changed to: {}", normalized_target);
    } else if response.status() == 404 {
        println!("❌ cd failed: Path not found: {}", normalized_target);
    } else {
        println!("❌ cd failed: {}", response.text().await?);
    }
    Ok(())
}

pub async fn handle_pwd() -> Result<()> {
    let cwd = resolve_cwd();
    println!("{}", cwd);
    Ok(())
}
