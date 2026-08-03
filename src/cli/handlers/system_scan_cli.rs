//! System and security scan CLI command handlers
//!
//! Implements system scanning and security audits locally.

use crate::cli::commands::enums::ScanCommand;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SecurityScanResult {
    pub files: Vec<FilePermissionAudit>,
    pub tokens: Vec<TokenAudit>,
    pub timestamp: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FilePermissionAudit {
    pub name: String,
    pub path: String,
    pub exists: bool,
    pub readable: bool,
    pub writeable: bool,
    pub permissions_octal: Option<String>,
    pub status: String, // "SECURE", "WARNING", "CRITICAL", "N/A"
    pub recommendation: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TokenAudit {
    pub name: String,
    pub present: bool,
    pub source: String, // "env" or "config" or "none"
    pub masked_value: Option<String>,
    pub status: String, // "CONFIGURED", "MISSING"
}

/// Run system or security scan
pub async fn handle_scan_command(cmd: ScanCommand) -> Result<()> {
    match cmd {
        ScanCommand::System { format, detailed } => {
            println!("🔍 Running system scan...\n");
            let result = crate::cli::handlers::system_scan::scan_system(detailed).await;
            match format.as_str() {
                "json" => {
                    println!("{}", crate::cli::handlers::system_scan::format_as_json(&result));
                }
                "markdown" | "md" => {
                    println!("{}", crate::cli::handlers::system_scan::format_as_markdown(&result));
                }
                _ => {
                    println!("{}", crate::cli::handlers::system_scan::format_as_table(&result));
                }
            }
        }
        ScanCommand::Security { format } => {
            println!("🛡️  Running security audit scan...\n");
            let result = run_security_scan().await?;
            match format.as_str() {
                "json" => {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                _ => {
                    print_security_scan_table(&result);
                }
            }
        }
    }
    Ok(())
}

async fn run_security_scan() -> Result<SecurityScanResult> {
    let mut files = Vec::new();

    // List of critical files to audit
    let critical_files = vec![
        ("Environment Configuration", ".env", "Contains sensitive database credentials and API keys."),
        ("Xavier Configuration", "config/xavier.config.json", "Xavier settings and global workspace variables."),
        ("Security DB", ".xavier/security.db", "Stores audit logs, API key registry, and security policies."),
        ("Keyring Store", ".xavier/keyring", "Contains local encryption keys and credential tokens."),
    ];

    for (name, rel_path, _desc) in critical_files {
        let path = std::path::Path::new(rel_path);
        let exists = path.exists();
        let mut readable = false;
        let mut writeable = false;
        let mut permissions_octal = None;
        let mut status = "N/A".to_string();
        let mut recommendation = None;

        if exists {
            readable = std::fs::read(path).is_ok();
            writeable = std::fs::OpenOptions::new().write(true).open(path).is_ok();

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = std::fs::metadata(path) {
                    let mode = meta.permissions().mode() & 0o777;
                    permissions_octal = Some(format!("{:04o}", mode));

                    // Check for loose permissions: writeable by group/others is critical; readable by group/others is warning.
                    if (mode & 0o022) != 0 {
                        status = "CRITICAL".to_string();
                        recommendation = Some(format!("Restrict write access: chmod 0600 {}", rel_path));
                    } else if (mode & 0o077) != 0 {
                        status = "WARNING".to_string();
                        recommendation = Some(format!("Restrict read access: chmod 0600 {}", rel_path));
                    } else {
                        status = "SECURE".to_string();
                    }
                }
            }

            #[cfg(not(unix))]
            {
                status = "SECURE".to_string();
            }
        } else {
            recommendation = Some(format!("File '{}' not found. Ensure it is initialized.", rel_path));
        }

        files.push(FilePermissionAudit {
            name: name.to_string(),
            path: rel_path.to_string(),
            exists,
            readable,
            writeable,
            permissions_octal,
            status,
            recommendation,
        });
    }

    // List of tokens to audit
    let token_keys = vec![
        "XAVIER_TOKEN",
        "XAVIER_TOKEN_SECRET",
        "XAVIER_ENCRYPTION_KEY",
        "CLAVIS_MASTER_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "GROQ_API_KEY",
        "DEEPSEEK_API_KEY",
        "GEMINI_API_KEY",
    ];

    let mut tokens = Vec::new();
    for key in token_keys {
        let value = std::env::var(key).ok();
        let present = value.is_some();

        let masked_value = if let Some(v) = value {
            if v.len() > 8 {
                Some(format!("{}...{}", &v[..4], &v[v.len() - 4..]))
            } else {
                Some("****".to_string())
            }
        } else {
            None
        };

        tokens.push(TokenAudit {
            name: key.to_string(),
            present,
            source: if present { "env".to_string() } else { "none".to_string() },
            masked_value,
            status: if present { "CONFIGURED".to_string() } else { "MISSING".to_string() },
        });
    }

    Ok(SecurityScanResult {
        files,
        tokens,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

fn print_security_scan_table(result: &SecurityScanResult) {
    println!("═══════════════════════════════════════════════════════════════");
    println!("  XAVIER SECURITY AUDIT REPORT");
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Timestamp: {}", result.timestamp);
    println!("═══════════════════════════════════════════════════════════════");

    println!("\n[1] CRITICAL FILES & DIRECTORIES PERMISSIONS:");
    println!("---------------------------------------------------------------");
    println!("{:<28} {:<10} {:<10} {:<10}", "File / Directory", "Exists", "Perms", "Status");
    println!("---------------------------------------------------------------");
    for f in &result.files {
        let exists_str = if f.exists { "Yes" } else { "No" };
        let perms_str = f.permissions_octal.as_deref().unwrap_or("N/A");
        let status_icon = match f.status.as_str() {
            "SECURE" => "✅ SECURE",
            "WARNING" => "⚠️  WARNING",
            "CRITICAL" => "🚨 CRITICAL",
            _ => "ℹ️  N/A",
        };
        println!("{:<28} {:<10} {:<10} {:<10}", f.name, exists_str, perms_str, status_icon);
    }

    let recommendations: Vec<_> = result.files.iter()
        .filter_map(|f| f.recommendation.as_ref().map(|r| (f.name.as_str(), r)))
        .collect();

    if !recommendations.is_empty() {
        println!("\n💡 File Security Recommendations:");
        for (name, rec) in recommendations {
            println!("  * {}: {}", name, rec);
        }
    }

    println!("\n[2] SYSTEM CREDENTIALS & API TOKENS AUDIT:");
    println!("---------------------------------------------------------------");
    println!("{:<28} {:<12} {:<20}", "Secret Name", "Status", "Masked Value");
    println!("---------------------------------------------------------------");
    for t in &result.tokens {
        let status_str = if t.present { "✅ CONFIGURED" } else { "❌ MISSING" };
        let masked = t.masked_value.as_deref().unwrap_or("N/A");
        println!("{:<28} {:<12} {:<20}", t.name, status_str, masked);
    }
    println!("═══════════════════════════════════════════════════════════════\n");
}
