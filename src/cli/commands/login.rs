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

    /// 6-digit TOTP two-factor authentication code (for headless/automated environments)
    #[arg(long = "totp-code", env = "XAVIER_TOTP_CODE")]
    pub totp_code: Option<String>,

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
    #[serde(default)]
    pub ok: bool,
    pub message: Option<String>,
    pub magic_url: Option<String>,
    pub email: Option<String>,
    pub expires_in_secs: Option<u64>,
    #[serde(default)]
    pub requires_2fa: bool,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TwoFactorChallenge {
    pub requires_2fa: bool,
    pub session_id: String,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Verify2faRequest {
    pub session_id: String,
    pub code: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Verify2faResponse {
    pub token: Option<String>,
    pub session_token: Option<String>,
    pub access_token: Option<String>,
    pub jwt: Option<String>,
    pub ok: Option<bool>,
    pub message: Option<String>,
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

/// Validates and normalizes an RFC 6238 TOTP 6-digit code.
/// Strips leading/trailing and inner whitespace, and ensures it consists of exactly 6 ASCII digits.
pub fn validate_totp_code(input: &str) -> Result<String> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.len() != 6 || !cleaned.chars().all(|c| c.is_ascii_digit()) {
        bail!(
            "Invalid 2FA code format: expected exactly 6 digits, received '{}'",
            cleaned
        );
    }
    Ok(cleaned)
}

/// Resolves the TOTP code from arguments, environment variable, or interactive terminal prompt.
pub fn prompt_or_resolve_totp_code(args_code: Option<&str>) -> Result<String> {
    // 1. Check argument if provided
    if let Some(code) = args_code {
        let trimmed = code.trim();
        if !trimmed.is_empty() {
            return validate_totp_code(trimmed);
        }
    }

    // 2. Check environment variable XAVIER_TOTP_CODE
    if let Ok(env_code) = std::env::var("XAVIER_TOTP_CODE") {
        let trimmed = env_code.trim();
        if !trimmed.is_empty() {
            return validate_totp_code(trimmed);
        }
    }

    // 3. Interactive prompt
    // Prompt the user in terminal: "Enter 6-digit 2FA code (Authenticator app): "
    let prompt_text = "Enter 6-digit 2FA code (Authenticator app): ";
    let raw_input: String = match dialoguer::Input::new()
        .with_prompt(prompt_text.trim_end())
        .interact_text()
    {
        Ok(val) => val,
        Err(_) => {
            // Fallback to standard stdin reading if dialoguer fails or non-standard TTY
            use std::io::Write;
            eprint!("{}", prompt_text);
            std::io::stderr().flush().ok();
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .context("failed to read 2FA code from standard input")?;
            line
        }
    };

    validate_totp_code(&raw_input)
}

/// Sends POST /auth/verify-2fa with { session_id, code } to receive final JWT session token.
pub async fn send_verify_2fa(cloud_url: &str, session_id: &str, code: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;

    let verify_url = format!("{}/auth/verify-2fa", cloud_url.trim_end_matches('/'));
    let payload = Verify2faRequest {
        session_id: session_id.to_string(),
        code: code.to_string(),
    };

    let resp = client
        .post(&verify_url)
        .json(&payload)
        .send()
        .await
        .with_context(|| format!("failed to send 2FA verification to {}", verify_url))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        bail!("2FA verification failed (HTTP {}): {}", status, body);
    }

    let parsed: Verify2faResponse = serde_json::from_str(&body)
        .with_context(|| format!("failed to parse 2FA verification response JSON: {}", body))?;

    let token = parsed
        .token
        .or(parsed.session_token)
        .or(parsed.access_token)
        .or(parsed.jwt)
        .ok_or_else(|| {
            anyhow!("2FA verification response did not include a valid session token")
        })?;

    Ok(token)
}

/// Prints the successful login message or JSON payload.
fn print_login_success(
    args: &LoginArgs,
    email: &str,
    cloud_url: &str,
    token: &str,
    usage: &CloudUsageResponse,
) -> Result<()> {
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

    let status = magic_resp.status();
    let resp_text = magic_resp.text().await.unwrap_or_default();

    if !status.is_success() {
        // Also inspect if error body indicates 2FA challenge
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&resp_text) {
            if val.get("requires_2fa").and_then(|v| v.as_bool()) == Some(true) {
                if let Some(session_id) = val.get("session_id").and_then(|v| v.as_str()) {
                    let code = prompt_or_resolve_totp_code(args.totp_code.as_deref())?;
                    let token = send_verify_2fa(&cloud_url, session_id, &code).await?;
                    let usage = verify_cloud_session(&cloud_url, &token).await?;
                    save_session_credentials(&cloud_url, &token).await?;

                    print_login_success(&args, &email, &cloud_url, &token, &usage)?;
                    return Ok(());
                }
            }
        }

        bail!(
            "Xavier Cloud rejected login request (HTTP {}): {}",
            status,
            resp_text
        );
    }

    let magic_data: MagicLinkResponse = serde_json::from_str(&resp_text)
        .with_context(|| format!("invalid response JSON from Xavier Cloud: {}", resp_text))?;

    // Check if initial magic-link request requires 2FA challenge
    let token = if magic_data.requires_2fa && magic_data.session_id.is_some() {
        let session_id = magic_data.session_id.as_deref().unwrap();
        let code = prompt_or_resolve_totp_code(args.totp_code.as_deref())?;
        send_verify_2fa(&cloud_url, session_id, &code).await?
    } else {
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

        let exchange_status = exchange_resp.status();
        let mut session_token: Option<String> = None;
        let mut exchange_2fa_session_id: Option<String> = None;

        // Check Location header on 302 redirect
        if let Some(loc_val) = exchange_resp.headers().get("location") {
            if let Ok(loc_str) = loc_val.to_str() {
                if let Some(t) = extract_param_from_url(loc_str, "token") {
                    session_token = Some(t);
                }
                if let Some(sid) = extract_param_from_url(loc_str, "session_id") {
                    if extract_param_from_url(loc_str, "requires_2fa").as_deref() == Some("true") {
                        exchange_2fa_session_id = Some(sid);
                    }
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

        if let Some(token) = session_token {
            token
        } else if let Some(sid) = exchange_2fa_session_id {
            let code = prompt_or_resolve_totp_code(args.totp_code.as_deref())?;
            send_verify_2fa(&cloud_url, &sid, &code).await?
        } else {
            // Check exchange response body if JSON returned requires_2fa
            let exchange_body = exchange_resp.text().await.unwrap_or_default();
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&exchange_body) {
                if val.get("requires_2fa").and_then(|v| v.as_bool()) == Some(true) {
                    if let Some(sid) = val.get("session_id").and_then(|v| v.as_str()) {
                        let code = prompt_or_resolve_totp_code(args.totp_code.as_deref())?;
                        send_verify_2fa(&cloud_url, sid, &code).await?
                    } else {
                        bail!("Backend requires 2FA but did not provide a session_id");
                    }
                } else if let Some(t) = val
                    .get("token")
                    .or_else(|| val.get("session_token"))
                    .and_then(|v| v.as_str())
                {
                    t.to_string()
                } else {
                    bail!(
                        "Could not automatically extract session token from redirect (HTTP {}): {}. \
                         You can manually log in via `xavier login --token <TOKEN>`.",
                        exchange_status,
                        exchange_body
                    );
                }
            } else {
                bail!(
                    "Could not automatically extract session token from redirect. \
                     You can manually log in via `xavier login --token <TOKEN>`."
                );
            }
        }
    };

    // Verify and fetch usage metrics
    let usage = verify_cloud_session(&cloud_url, &token).await?;
    save_session_credentials(&cloud_url, &token).await?;

    print_login_success(&args, &email, &cloud_url, &token, &usage)?;

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
    use clap::Parser;

    #[derive(Parser, Debug)]
    struct LoginCliTest {
        #[command(flatten)]
        args: LoginArgs,
    }

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

    #[test]
    fn test_validate_totp_code() {
        assert_eq!(validate_totp_code("123456").unwrap(), "123456");
        assert_eq!(validate_totp_code(" 123 456 ").unwrap(), "123456");
        assert_eq!(validate_totp_code("\t654321\n").unwrap(), "654321");

        // Invalid codes
        assert!(validate_totp_code("12345").is_err());
        assert!(validate_totp_code("1234567").is_err());
        assert!(validate_totp_code("12a456").is_err());
        assert!(validate_totp_code("abcdef").is_err());
        assert!(validate_totp_code("").is_err());
    }

    #[test]
    fn test_parse_2fa_challenge_response() {
        let json_data = r#"{"requires_2fa": true, "session_id": "sess_challenge_9988"}"#;
        let challenge: TwoFactorChallenge =
            serde_json::from_str(json_data).expect("valid challenge json");
        assert!(challenge.requires_2fa);
        assert_eq!(challenge.session_id, "sess_challenge_9988");

        let magic_data: MagicLinkResponse =
            serde_json::from_str(json_data).expect("valid magic link 2fa json");
        assert!(magic_data.requires_2fa);
        assert_eq!(
            magic_data.session_id,
            Some("sess_challenge_9988".to_string())
        );
    }

    #[test]
    fn test_login_args_totp_code_flag() {
        let parsed = LoginCliTest::try_parse_from(["login_test", "--totp-code", "123456"]).unwrap();
        assert_eq!(parsed.args.totp_code, Some("123456".to_string()));
    }

    #[test]
    fn test_login_args_totp_code_env() {
        std::env::set_var("XAVIER_TOTP_CODE", "654321");
        let parsed = LoginCliTest::try_parse_from(["login_test"]).unwrap();
        assert_eq!(parsed.args.totp_code, Some("654321".to_string()));
        std::env::remove_var("XAVIER_TOTP_CODE");
    }
}
