//! System & security scan CLI command handlers
//!
//! Implements local-first system scan and comprehensive security audits.

use crate::cli::commands::enums::ScanCommand;
use crate::cli::handlers::system_scan::{
    format_as_json, format_as_markdown, format_as_table, scan_system, SystemScanResult,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Run system scan (system or security)
pub async fn handle_scan_command(cmd: ScanCommand) -> Result<()> {
    match cmd {
        ScanCommand::System { format, detailed } => {
            println!("🔍 Running system scan...\n");

            // Try HTTP first
            let base_url = crate::cli::config::resolve_base_url();
            let mut scan_result: Option<SystemScanResult> = None;

            if let Ok(token) = crate::cli::config::require_xavier_token() {
                let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();
                let url = format!("{}/v1/system/scan", base_url);
                if let Ok(resp) = client.get(&url).header("X-Xavier-Token", &token).send().await {
                    if resp.status().is_success() {
                        if let Ok(res) = resp.json::<SystemScanResult>().await {
                            scan_result = Some(res);
                        }
                    }
                }
            }

            let result = match scan_result {
                Some(res) => res,
                None => {
                    println!("⚠️ Server offline or unreachable. Falling back to local offline scan...\n");
                    scan_system(detailed).await
                }
            };

            match format.as_str() {
                "json" => println!("{}", format_as_json(&result)),
                "markdown" | "md" => println!("{}", format_as_markdown(&result)),
                "table" => println!("{}", format_as_table(&result)),
                other => {
                    anyhow::bail!("unsupported scan output format: {other}");
                }
            }
        }
        ScanCommand::Security { format } => {
            println!("🔒 Running security audit...\n");
            let result = run_security_audit().await;

            match format.as_str() {
                "json" => println!("{}", serde_json::to_string_pretty(&result)?),
                _ => print_security_table(&result),
            }
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PermissionCheck {
    pub name: String,
    pub path: String,
    pub exists: bool,
    pub status: String, // "SECURE", "WARNING", "INSECURE", "N/A"
    pub details: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TokenAuditItem {
    pub name: String,
    pub configured: bool,
    pub source: String, // "env", "config", "none"
    pub masked_value: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SecurityAuditResult {
    pub permissions: Vec<PermissionCheck>,
    pub tokens: Vec<TokenAuditItem>,
}

async fn run_security_audit() -> SecurityAuditResult {
    // 1. Critical file permissions
    let critical_paths = vec![
        (".env", "Environment configuration file"),
        ("config/xavier.config.json", "Core configuration file"),
        (".xavier", "Application metadata and reports directory"),
        ("data", "Application databases directory"),
    ];

    let permissions = critical_paths
        .into_iter()
        .map(|(path, desc)| check_path_security(path, desc))
        .collect();

    // 2. Load settings file direct to extract credentials for token audit
    let mut config_json: Option<serde_json::Value> = None;
    if let Ok(content) = std::fs::read_to_string("config/xavier.config.json") {
        config_json = serde_json::from_str(&content).ok();
    }

    let token_keys = vec![
        ("XAVIER_TOKEN", config_json.as_ref().and_then(|v| v.get("auth_token").or(v.get("token")).and_then(|t| t.as_str()))),
        ("XAVIER_API_KEY", config_json.as_ref().and_then(|v| v.get("api_key").and_then(|t| t.as_str()))),
        ("OPENAI_API_KEY", config_json.as_ref().and_then(|v| v.get("models").and_then(|m| m.get("llm_api_key").or(m.get("local_llm_api_key")).and_then(|t| t.as_str())))),
        ("ANTHROPIC_API_KEY", None),
        ("GROQ_API_KEY", None),
        ("DEEPSEEK_API_KEY", None),
        ("GEMINI_API_KEY", None),
        ("MINIMAX_API_KEY", None),
    ];

    let tokens = token_keys
        .into_iter()
        .map(|(name, config_val)| audit_token(name, config_val))
        .collect();

    SecurityAuditResult {
        permissions,
        tokens,
    }
}

fn check_path_security(path_str: &str, description: &str) -> PermissionCheck {
    let path = std::path::Path::new(path_str);
    if !path.exists() {
        return PermissionCheck {
            name: description.to_string(),
            path: path_str.to_string(),
            exists: false,
            status: "N/A".to_string(),
            details: "File/directory does not exist".to_string(),
        };
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mode = meta.permissions().mode();
            let is_dir = meta.is_dir();

            let world_readable = (mode & 0o004) != 0;
            let world_writable = (mode & 0o002) != 0;
            let group_readable = (mode & 0o040) != 0;
            let group_writable = (mode & 0o020) != 0;

            if world_writable {
                PermissionCheck {
                    name: description.to_string(),
                    path: path_str.to_string(),
                    exists: true,
                    status: "INSECURE".to_string(),
                    details: format!("World-writable: mode {:o} (Severe Risk)", mode & 0o777),
                }
            } else if is_dir {
                if world_writable || group_writable {
                    PermissionCheck {
                        name: description.to_string(),
                        path: path_str.to_string(),
                        exists: true,
                        status: "WARNING".to_string(),
                        details: format!("Group-writable directory: mode {:o}", mode & 0o777),
                    }
                } else {
                    PermissionCheck {
                        name: description.to_string(),
                        path: path_str.to_string(),
                        exists: true,
                        status: "SECURE".to_string(),
                        details: format!("Safe directory: mode {:o}", mode & 0o777),
                    }
                }
            } else {
                if world_readable {
                    PermissionCheck {
                        name: description.to_string(),
                        path: path_str.to_string(),
                        exists: true,
                        status: "INSECURE".to_string(),
                        details: format!("World-readable credential file: mode {:o} (Risk)", mode & 0o777),
                    }
                } else if group_readable || group_writable {
                    PermissionCheck {
                        name: description.to_string(),
                        path: path_str.to_string(),
                        exists: true,
                        status: "WARNING".to_string(),
                        details: format!("Group-accessible file: mode {:o}", mode & 0o777),
                    }
                } else {
                    PermissionCheck {
                        name: description.to_string(),
                        path: path_str.to_string(),
                        exists: true,
                        status: "SECURE".to_string(),
                        details: format!("Safe credential file: mode {:o}", mode & 0o777),
                    }
                }
            }
        } else {
            PermissionCheck {
                name: description.to_string(),
                path: path_str.to_string(),
                exists: true,
                status: "WARNING".to_string(),
                details: "Could not read file metadata".to_string(),
            }
        }
    }

    #[cfg(not(unix))]
    {
        PermissionCheck {
            name: description.to_string(),
            path: path_str.to_string(),
            exists: true,
            status: "SECURE".to_string(),
            details: "File exists (NTFS ACL security handled by OS)".to_string(),
        }
    }
}

fn audit_token(name: &str, config_val: Option<&str>) -> TokenAuditItem {
    let env_val = std::env::var(name).ok();

    let (configured, source, raw_value) = if let Some(v) = env_val {
        if !v.is_empty() {
            (true, "env".to_string(), Some(v))
        } else {
            (false, "none".to_string(), None)
        }
    } else if let Some(v) = config_val {
        if !v.is_empty() {
            (true, "config".to_string(), Some(v.to_string()))
        } else {
            (false, "none".to_string(), None)
        }
    } else {
        (false, "none".to_string(), None)
    };

    let masked_value = if let Some(v) = raw_value {
        if v.len() > 8 {
            format!("{}...{}", &v[..4], &v[v.len() - 4..])
        } else {
            "****".to_string()
        }
    } else {
        "not configured".to_string()
    };

    TokenAuditItem {
        name: name.to_string(),
        configured,
        source,
        masked_value,
    }
}

fn print_security_table(result: &SecurityAuditResult) {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║                    SECURITY SCAN RESULTS                      ║");
    println!("╠═══════════════════════════════════════════════════════════════╣");
    println!("║  Critical File Permissions:                                   ║");

    for item in &result.permissions {
        let status_color = match item.status.as_str() {
            "SECURE" => "🟢 SECURE ",
            "WARNING" => "🟡 WARNING",
            "INSECURE" => "🔴 INSECURE",
            _ => "⚪ N/A     ",
        };
        println!(
            "║  - {:30} [{}] {:15}  ║",
            item.path, status_color, item.details
        );
    }

    println!("╠═══════════════════════════════════════════════════════════════╣");
    println!("║  Configured Tokens & Keys:                                    ║");

    for token in &result.tokens {
        let status = if token.configured {
            format!("🟢 [{}]", token.source)
        } else {
            "🔴 [NOT SET]".to_string()
        };
        println!(
            "║  - {:25} {:15} {:20} ║",
            token.name, status, token.masked_value
        );
    }

    println!("╚═══════════════════════════════════════════════════════════════╝");
}
