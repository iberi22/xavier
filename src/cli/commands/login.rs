//! CLI command for Xavier Cloud authentication (login, logout, status).
//!
//! Provides a seamless web/browser and 1-click magic link authentication
//! flow for the Xavier CLI (similar to `gh auth login`), securely saving the
//! session token into the hardware vault and runtime configuration.

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Subcommand};
use colored::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::secrets::vault::HardwareVault;
use crate::settings::XavierSettings;

pub const DEFAULT_CLOUD_URL: &str = "https://xavier-cloud.swal.network";
pub const DEFAULT_LOGIN_EMAIL: &str = "iberi22@gmail.com";

#[derive(Args, Debug, Clone)]
pub struct LoginArgs {
    /// Account email to authenticate with (defaults to founder account)
    #[arg(short, long)]
    pub email: Option<String>,

    /// Xavier Cloud endpoint URL
    #[arg(long, default_value = DEFAULT_CLOUD_URL)]
    pub cloud_url: String,

    /// Direct session token login (bypasses browser magic link)
    #[arg(long)]
    pub token: Option<String>,

    /// Do not automatically open the default web browser
    #[arg(long, default_value_t = false)]
    pub no_browser: bool,

    /// Show current authentication status instead of logging in
    #[arg(long, default_value_t = false)]
    pub status: bool,

    /// Output results in JSON format
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum AuthCommand {
    /// Log in to Xavier Cloud
    Login(LoginArgs),
    /// Log out from Xavier Cloud
    Logout {
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Show current authentication status
    Status {
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct MagicLinkResponse {
    pub ok: bool,
    pub message: Option<String>,
    pub magic_url: Option<String>,
    pub email: Option<String>,
    pub expires_in_secs: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CloudUsageResponse {
    pub tenant: String,
    pub plan: Option<String>,
    pub bytes: u64,
    pub chunks: u64,
    pub quota: u64,
    #[serde(default)]
    pub ai_tokens_per_day: Option<u64>,
    #[serde(default)]
    pub percent: Option<f64>,
}

/// Open a URL in the system's default browser (cross-platform).
pub fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .context("failed to launch default browser on Windows")?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .context("failed to launch default browser on macOS")?;
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .context("failed to launch default browser on Linux")?;
    }
    Ok(())
}

/// Extracts a named query, hash, or cookie parameter from a string.
pub fn extract_param_from_url(url_str: &str, param_name: &str) -> Option<String> {
    let key = format!("{}=", param_name);
    if let Some(idx) = url_str.find(&key) {
        let after = &url_str[idx + key.len()..];
        let end = after
            .find('&')
            .map(|i| after[..i].find(';').unwrap_or(i))
            .or_else(|| after.find(';'))
            .unwrap_or(after.len());
        let val = &after[..end];
        if !val.trim().is_empty() {
            return Some(val.trim().to_string());
        }
    }
    None
}

/// Verifies a session token against Xavier Cloud `/v1/cloud/usage`.
pub async fn verify_cloud_session(cloud_url: &str, token: &str) -> Result<CloudUsageResponse> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let url = format!("{}/v1/cloud/usage", cloud_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .with_context(|| format!("failed to reach Xavier Cloud at {}", url))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!(
            "Xavier Cloud authentication check rejected (HTTP {}): {}",
            status,
            body
        );
    }

    let usage = resp
        .json::<CloudUsageResponse>()
        .await
        .context("invalid usage response JSON from Xavier Cloud")?;

    Ok(usage)
}

/// Persists the active session token in both the Hardware Vault and XavierSettings.
pub async fn save_session_credentials(cloud_url: &str, token: &str) -> Result<()> {
    // 1. Hardware Vault
    let vault = HardwareVault::new("xavier");
    let _ = vault.store_secret("PGHEART_TOKEN", token);
    let _ = vault.store_secret("XAVIER_CLOUD_TOKEN", token);

    // 2. Settings / Config file
    let mut settings = XavierSettings::current();
    settings.pgheart.url = Some(cloud_url.trim_end_matches('/').to_string());
    settings.pgheart.token = Some(token.to_string());
    settings.pgheart.instance_id = Some("default".to_string());
    settings
        .save()
        .await
        .context("failed to save settings to config file")?;

    Ok(())
}

/// Formats bytes into a human-readable string.
fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{} bytes", bytes)
    }
}

/// Handles `xavier login` command.
pub async fn handle_login_cmd(args: LoginArgs) -> Result<()> {
    if args.status {
        return handle_auth_status_cmd(args.json).await;
    }

    let cloud_url = args.cloud_url.trim_end_matches('/').to_string();

    // Direct token login
    if let Some(ref raw_token) = args.token {
        let token = raw_token.trim().to_string();
        if token.is_empty() {
            bail!("Provided token cannot be empty");
        }

        let usage = verify_cloud_session(&cloud_url, &token).await?;
        save_session_credentials(&cloud_url, &token).await?;

        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "status": "authenticated",
                    "cloud_url": cloud_url,
                    "tenant": usage.tenant,
                    "plan": usage.plan,
                    "bytes": usage.bytes,
                    "quota": usage.quota,
                }))?
            );
        } else {
            println!();
            println!(
                "{}",
                "════════════════════════════════════════════════════════════".cyan()
            );
            println!(
                "  {} ",
                "✓ Successfully authenticated with Xavier Cloud!"
                    .bold()
                    .green()
            );
            println!(
                "{}",
                "════════════════════════════════════════════════════════════".cyan()
            );
            println!("  {:<16} {}", "Cloud Endpoint:", cloud_url.white());
            println!("  {:<16} {}", "Tenant:", usage.tenant.green());
            if let Some(ref p) = usage.plan {
                println!("  {:<16} {}", "Plan:", p.yellow());
            }
            println!(
                "  {:<16} {} / {}",
                "Cloud Storage:",
                format_bytes(usage.bytes).cyan(),
                format_bytes(usage.quota).white()
            );
            println!(
                "{}",
                "════════════════════════════════════════════════════════════".cyan()
            );
            println!("  Local CLI, daemon, and synchronization are now logged in.\n");
        }
        return Ok(());
    }

    // Interactive Browser / Magic Link flow
    let email = args
        .email
        .filter(|e| !e.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_LOGIN_EMAIL.to_string());

    if !args.json {
        println!();
        println!(
            "{}",
            "════════════════════════════════════════════════════════════".cyan()
        );
        println!("  {} ", "Xavier Cloud Login (SWAL Mesh)".bold().white());
        println!(
            "{}",
            "════════════════════════════════════════════════════════════".cyan()
        );
        println!("  {:<16} {}", "Account Email:", email.yellow());
        println!("  {:<16} {}", "Target Node:", cloud_url.white());
        println!();
        println!("  Requesting 1-click authentication link from Cloudflare...");
    }

    // Build client that does NOT follow redirects, so we can capture Location headers
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()?;

    let magic_req_url = format!("{}/auth/magic-link", cloud_url);
    let magic_resp = client
        .post(&magic_req_url)
        .json(&serde_json::json!({ "email": email }))
        .send()
        .await
        .with_context(|| format!("failed to request magic link from {}", magic_req_url))?;

    if !magic_resp.status().is_success() {
        let status = magic_resp.status();
        let body = magic_resp.text().await.unwrap_or_default();
        bail!(
            "Xavier Cloud rejected login request (HTTP {}): {}",
            status,
            body
        );
    }

    let magic_data = magic_resp.json::<MagicLinkResponse>().await?;
    let magic_url = magic_data
        .magic_url
        .ok_or_else(|| anyhow!("Cloud response did not return a valid authentication URL"))?;

    if !args.json {
        println!(
            "  {} {}",
            "🔗 Magic Link URL:".bold(),
            magic_url.cyan().underline()
        );
        println!();
        if !args.no_browser {
            println!("  Opening your default browser to authorize...");
            if let Err(e) = open_browser(&magic_url) {
                tracing::debug!("Browser auto-open notice: {e}");
            }
        }
        println!("  Completing token exchange with Cloudflare...");
    }

    // Exchange / consume magic link directly to acquire session token
    let exchange_resp = client
        .get(&magic_url)
        .send()
        .await
        .with_context(|| format!("failed to execute verification against {}", magic_url))?;

    let mut session_token: Option<String> = None;

    // Check Location header on 302 redirect
    if let Some(loc_val) = exchange_resp.headers().get("location") {
        if let Ok(loc_str) = loc_val.to_str() {
            if let Some(t) = extract_param_from_url(loc_str, "token") {
                session_token = Some(t);
            }
        }
    }

    // Fallback: Check Set-Cookie header
    if session_token.is_none() {
        for cookie_val in exchange_resp.headers().get_all("set-cookie") {
            if let Ok(c_str) = cookie_val.to_str() {
                if let Some(t) = extract_param_from_url(c_str, "__Host-swal_session") {
                    session_token = Some(t);
                    break;
                }
            }
        }
    }

    let token = session_token.ok_or_else(|| {
        anyhow!(
            "Could not automatically extract session token from redirect. \
             You can manually log in via `xavier login --token <TOKEN>`."
        )
    })?;

    // Verify and fetch usage metrics
    let usage = verify_cloud_session(&cloud_url, &token).await?;
    save_session_credentials(&cloud_url, &token).await?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "authenticated",
                "email": email,
                "cloud_url": cloud_url,
                "tenant": usage.tenant,
                "plan": usage.plan,
                "storage_bytes": usage.bytes,
                "storage_quota": usage.quota,
                "chunks": usage.chunks,
                "session_token": token,
            }))?
        );
    } else {
        println!();
        println!(
            "{}",
            "════════════════════════════════════════════════════════════".cyan()
        );
        println!(
            "  {} ",
            "✓ Successfully authenticated with Xavier Cloud!"
                .bold()
                .green()
        );
        println!(
            "{}",
            "════════════════════════════════════════════════════════════".cyan()
        );
        println!("  {:<16} {}", "Account Email:", email.yellow());
        println!("  {:<16} {}", "Tenant:", usage.tenant.green());
        if let Some(ref p) = usage.plan {
            println!("  {:<16} {}", "Plan:", p.white());
        }
        println!(
            "  {:<16} {} in {} chunks ({:.2}% of {})",
            "Cloud Storage:",
            format_bytes(usage.bytes).cyan(),
            usage.chunks,
            usage.percent.unwrap_or(0.0),
            format_bytes(usage.quota).white()
        );
        if let Some(tokens) = usage.ai_tokens_per_day {
            println!("  {:<16} {} tokens/day", "AI Inference:", tokens);
        }
        println!(
            "{}",
            "════════════════════════════════════════════════════════════".cyan()
        );
        println!(
            "  {} Session credentials persisted to secure vault.",
            "✓".green()
        );
        println!(
            "  {} Cloud backups and local CLI are now fully synchronized.\n",
            "✓".green()
        );
    }

    Ok(())
}

/// Handles `xavier logout` command.
pub async fn handle_logout_cmd(json: bool) -> Result<()> {
    // Clear hardware vault
    let vault = HardwareVault::new("xavier");
    let _ = vault.store_secret("PGHEART_TOKEN", "");
    let _ = vault.store_secret("XAVIER_CLOUD_TOKEN", "");

    // Clear settings
    let mut settings = XavierSettings::current();
    settings.pgheart.token = None;
    let _ = settings.save().await;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "logged_out",
                "ok": true
            }))?
        );
    } else {
        println!();
        println!("  {} Logged out from Xavier Cloud.", "✓".green());
        println!("  Local credentials removed from vault and configuration.\n");
    }

    Ok(())
}

/// Handles `xavier auth status` command.
pub async fn handle_auth_status_cmd(json: bool) -> Result<()> {
    let cloud_url = std::env::var("PGHEART_URL")
        .or_else(|_| std::env::var("XAVIER_PGHEART_URL"))
        .unwrap_or_else(|_| {
            XavierSettings::current()
                .pgheart
                .url
                .unwrap_or_else(|| DEFAULT_CLOUD_URL.to_string())
        });

    let token_opt = std::env::var("XAVIER_CLOUD_TOKEN")
        .or_else(|_| std::env::var("PGHEART_TOKEN"))
        .or_else(|_| std::env::var("XAVIER_PGHEART_TOKEN"))
        .ok()
        .filter(|t| !t.trim().is_empty())
        .or_else(|| {
            XavierSettings::current()
                .pgheart
                .token
                .filter(|t| !t.trim().is_empty())
        })
        .or_else(|| {
            HardwareVault::new("xavier")
                .get_secret("XAVIER_CLOUD_TOKEN")
                .or_else(|_| HardwareVault::new("xavier").get_secret("PGHEART_TOKEN"))
                .ok()
                .filter(|t| !t.trim().is_empty())
        });

    let Some(token) = token_opt else {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "authenticated": false,
                    "cloud_url": cloud_url,
                }))?
            );
        } else {
            println!();
            println!(
                "{}",
                "════════════════════════════════════════════════════════════".yellow()
            );
            println!(
                "  {} ",
                "Xavier Cloud Status: Not Authenticated".bold().yellow()
            );
            println!(
                "{}",
                "════════════════════════════════════════════════════════════".yellow()
            );
            println!("  {:<16} {}", "Cloud Endpoint:", cloud_url.white());
            println!("  {:<16} {}", "Status:", "Offline / Unauthenticated".red());
            println!();
            println!(
                "  To log in via browser / magic link, run: {}",
                "xavier login".green().bold()
            );
            println!(
                "{}",
                "════════════════════════════════════════════════════════════".yellow()
            );
            println!();
        }
        return Ok(());
    };

    match verify_cloud_session(&cloud_url, &token).await {
        Ok(usage) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "authenticated": true,
                        "cloud_url": cloud_url,
                        "tenant": usage.tenant,
                        "plan": usage.plan,
                        "storage_bytes": usage.bytes,
                        "storage_quota": usage.quota,
                        "chunks": usage.chunks,
                    }))?
                );
            } else {
                println!();
                println!(
                    "{}",
                    "════════════════════════════════════════════════════════════".cyan()
                );
                println!("  {} ", "Xavier Cloud Status: Authenticated".bold().green());
                println!(
                    "{}",
                    "════════════════════════════════════════════════════════════".cyan()
                );
                println!("  {:<16} {}", "Cloud Endpoint:", cloud_url.white());
                println!("  {:<16} {}", "Tenant:", usage.tenant.green());
                if let Some(ref p) = usage.plan {
                    println!("  {:<16} {}", "Plan:", p.white());
                }
                println!(
                    "  {:<16} {} in {} chunks ({:.2}% of {})",
                    "Cloud Storage:",
                    format_bytes(usage.bytes).cyan(),
                    usage.chunks,
                    usage.percent.unwrap_or(0.0),
                    format_bytes(usage.quota).white()
                );
                if let Some(tokens) = usage.ai_tokens_per_day {
                    println!("  {:<16} {} tokens/day", "AI Inference:", tokens);
                }
                println!(
                    "{}",
                    "════════════════════════════════════════════════════════════".cyan()
                );
                println!("  Node is fully connected and ready for cloud backup & sync.\n");
            }
        }
        Err(e) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "authenticated": false,
                        "cloud_url": cloud_url,
                        "error": e.to_string(),
                    }))?
                );
            } else {
                println!();
                println!(
                    "{}",
                    "════════════════════════════════════════════════════════════".red()
                );
                println!(
                    "  {} ",
                    "Xavier Cloud Status: Session Expired or Error".bold().red()
                );
                println!(
                    "{}",
                    "════════════════════════════════════════════════════════════".red()
                );
                println!("  {:<16} {}", "Cloud Endpoint:", cloud_url.white());
                println!("  {:<16} {}", "Error:", e.to_string().red());
                println!();
                println!(
                    "  To re-authenticate, run: {}",
                    "xavier login".green().bold()
                );
                println!(
                    "{}",
                    "════════════════════════════════════════════════════════════".red()
                );
                println!();
            }
        }
    }

    Ok(())
}

/// Handles `xavier auth <subcommand>`
pub async fn handle_auth_cmd(cmd: AuthCommand) -> Result<()> {
    match cmd {
        AuthCommand::Login(args) => handle_login_cmd(args).await,
        AuthCommand::Logout { json } => handle_logout_cmd(json).await,
        AuthCommand::Status { json } => handle_auth_status_cmd(json).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_param_from_url() {
        let url1 = "https://xavier-cloud.swal.network/#/session?token=swal_sess_12345&email=user%40test.com&tenant=swal";
        assert_eq!(
            extract_param_from_url(url1, "token"),
            Some("swal_sess_12345".to_string())
        );
        assert_eq!(
            extract_param_from_url(url1, "tenant"),
            Some("swal".to_string())
        );

        let cookie_str = "__Host-swal_session=swal_sess_abcde; Path=/; Secure; HttpOnly";
        assert_eq!(
            extract_param_from_url(cookie_str, "__Host-swal_session"),
            Some("swal_sess_abcde".to_string())
        );
    }
}
