//! Doctor: CLI subcommand to diagnose local-first health.

use crate::cli::handlers::system_scan::scan_system;
use crate::settings::XavierSettings;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DoctorCheck {
    pub name: String,
    pub status: CheckStatus, // Ok / Warn / Fail
    pub detail: String,
    pub hint: Option<String>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Copy, Debug)]
pub enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
    pub overall: CheckStatus,
}

pub async fn handle_doctor(format: String, verbose: bool) -> Result<()> {
    let settings = XavierSettings::current();
    let scan = scan_system(false).await;

    let mut checks = Vec::new();

    // 1. Ollama Reachability
    let ollama_reachable = scan.ollama.running;
    checks.push(DoctorCheck {
        name: "Ollama Reachability".to_string(),
        status: if ollama_reachable {
            CheckStatus::Ok
        } else {
            CheckStatus::Fail
        },
        detail: if ollama_reachable {
            format!(
                "Ollama is running at {} (version: {})",
                scan.ollama.url,
                scan.ollama.version.as_deref().unwrap_or("unknown")
            )
        } else {
            format!(
                "Ollama is not running or not reachable at {}",
                scan.ollama.url
            )
        },
        hint: if ollama_reachable {
            None
        } else {
            Some(
                "Please start Ollama with 'ollama serve' or install it from https://ollama.com"
                    .to_string(),
            )
        },
    });

    // 2. LLM Model Installed
    let expected_llm = std::env::var("XAVIER_LOCAL_LLM_MODEL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            if !settings.models.local_llm_model.trim().is_empty() {
                settings.models.local_llm_model.clone()
            } else {
                "qwen3-coder".to_string()
            }
        });

    let llm_installed = scan.ollama.models.iter().any(|m| {
        m.to_lowercase().contains(&expected_llm.to_lowercase())
            || expected_llm.to_lowercase().contains(&m.to_lowercase())
    });

    checks.push(DoctorCheck {
        name: "LLM Model Installed".to_string(),
        status: if llm_installed {
            CheckStatus::Ok
        } else {
            CheckStatus::Fail
        },
        detail: if llm_installed {
            format!("Model '{}' is installed in Ollama", expected_llm)
        } else {
            format!("Model '{}' is not installed in Ollama", expected_llm)
        },
        hint: if llm_installed {
            None
        } else {
            Some(format!(
                "Run 'ollama pull {}' to install the model",
                expected_llm
            ))
        },
    });

    // 3. Embedding Model Installed
    let expected_embed = std::env::var("XAVIER_EMBEDDING_MODEL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            if !settings.models.embedding_model.trim().is_empty() {
                settings.models.embedding_model.clone()
            } else {
                "embeddinggemma".to_string()
            }
        });

    let embed_installed = scan.ollama.models.iter().any(|m| {
        m.to_lowercase().contains(&expected_embed.to_lowercase())
            || expected_embed.to_lowercase().contains(&m.to_lowercase())
    });

    checks.push(DoctorCheck {
        name: "Embedding Model Installed".to_string(),
        status: if embed_installed {
            CheckStatus::Ok
        } else {
            CheckStatus::Fail
        },
        detail: if embed_installed {
            format!(
                "Embedding model '{}' is installed in Ollama",
                expected_embed
            )
        } else {
            format!(
                "Embedding model '{}' is not installed in Ollama",
                expected_embed
            )
        },
        hint: if embed_installed {
            None
        } else {
            Some(format!(
                "Run 'ollama pull {}' to install the embedding model",
                expected_embed
            ))
        },
    });

    // 4. Config Válida para Local
    let provider = std::env::var("XAVIER_MODEL_PROVIDER")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| settings.models.provider.clone());

    let local_llm_url = std::env::var("XAVIER_LOCAL_LLM_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| settings.models.local_llm_url.clone());

    let local_llm_model = std::env::var("XAVIER_LOCAL_LLM_MODEL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| settings.models.local_llm_model.clone());

    let provider_is_local = provider == "local";
    let url_is_valid = !local_llm_url.trim().is_empty()
        && (local_llm_url.starts_with("http://") || local_llm_url.starts_with("https://"));
    let model_is_valid = !local_llm_model.trim().is_empty();
    let config_valid = provider_is_local && url_is_valid && model_is_valid;

    checks.push(DoctorCheck {
        name: "Local Configuration".to_string(),
        status: if config_valid { CheckStatus::Ok } else { CheckStatus::Fail },
        detail: if config_valid {
            format!("Local provider is configured with model '{}' at '{}'", local_llm_model, local_llm_url)
        } else {
            format!("Local configuration is invalid: provider='{}' (expected 'local'), url='{}', model='{}'", provider, local_llm_url, local_llm_model)
        },
        hint: if config_valid {
            None
        } else {
            Some("Run 'xavier setup --local' to configure local-first settings automatically".to_string())
        },
    });

    // 5. URL Reachable
    let client = reqwest::Client::new();
    let mut url_reachable = false;
    let mut url_error = String::new();

    if url_is_valid {
        let url1 = if local_llm_url.ends_with("/v1") {
            format!("{}/api/version", &local_llm_url[..local_llm_url.len() - 3])
        } else if local_llm_url.ends_with("/v1/") {
            format!("{}/api/version", &local_llm_url[..local_llm_url.len() - 4])
        } else {
            format!("{}/api/version", local_llm_url)
        };

        match client
            .get(&url1)
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                url_reachable = true;
            }
            Err(e) => {
                url_error = e.to_string();
            }
            Ok(resp) => {
                url_error = format!("HTTP {}", resp.status());
            }
        }

        if !url_reachable {
            let url2 = format!("{}/api/version", local_llm_url);
            if let Ok(resp) = client
                .get(&url2)
                .timeout(std::time::Duration::from_secs(2))
                .send()
                .await
            {
                if resp.status().is_success() {
                    url_reachable = true;
                }
            }
        }
    } else {
        url_error = "LLM URL is invalid or empty".to_string();
    }

    checks.push(DoctorCheck {
        name: "Local LLM URL Reachability".to_string(),
        status: if url_reachable {
            CheckStatus::Ok
        } else {
            CheckStatus::Fail
        },
        detail: if url_reachable {
            format!("Local LLM URL is reachable and responded successfully")
        } else {
            format!("Failed to reach LLM URL '{}': {}", local_llm_url, url_error)
        },
        hint: if url_reachable {
            None
        } else {
            Some(
                "Check if Ollama is running and the URL is correct in your configuration"
                    .to_string(),
            )
        },
    });

    // 6. DB Access
    let db_path = std::env::var("XAVIER_MEMORY_VEC_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            if settings.memory.vec_path.trim().is_empty() {
                std::path::PathBuf::from(&settings.memory.data_dir).join("xavier_memory_vec.db")
            } else {
                std::path::PathBuf::from(&settings.memory.vec_path)
            }
        });

    let (db_access_ok, db_detail, db_hint) = match rusqlite::Connection::open(&db_path) {
        Ok(conn) => {
            match conn.query_row("SELECT count(*) FROM memory_records", [], |row| {
                row.get::<_, i64>(0)
            }) {
                Ok(count) => (
                    true,
                    format!(
                        "Database accessible at '{}' (contains {} memory records)",
                        db_path.display(),
                        count
                    ),
                    None,
                ),
                Err(e) => (
                    false,
                    format!("Failed to query database at '{}': {}", db_path.display(), e),
                    Some(
                        "Verify that the database schema is initialized and up to date."
                            .to_string(),
                    ),
                ),
            }
        }
        Err(e) => (
            false,
            format!("Failed to open database at '{}': {}", db_path.display(), e),
            Some("Check file permissions or if the path is correct.".to_string()),
        ),
    };

    checks.push(DoctorCheck {
        name: "Database Access".to_string(),
        status: if db_access_ok {
            CheckStatus::Ok
        } else {
            CheckStatus::Fail
        },
        detail: db_detail,
        hint: db_hint,
    });

    // Keep a copy of critical checks (1-6) to decide the final exit status
    let critical_checks = checks.clone();

    // 7. Embedding Model Consistency (Soft/Warn)
    let mut check_7_status = CheckStatus::Ok;
    let mut check_7_detail =
        "Table 'embedding_model_meta' does not exist, skipping consistency check (not applicable)"
            .to_string();
    let mut check_7_hint = None;

    if db_access_ok {
        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
            let table_exists: bool = conn.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='embedding_model_meta'",
                [],
                |row| row.get(0),
            ).unwrap_or(0) > 0;

            if table_exists {
                let db_model: Option<String> = conn
                    .query_row(
                        "SELECT model_name FROM embedding_model_meta LIMIT 1",
                        [],
                        |row| row.get(0),
                    )
                    .or_else(|_| {
                        conn.query_row(
                            "SELECT model FROM embedding_model_meta LIMIT 1",
                            [],
                            |row| row.get(0),
                        )
                    })
                    .ok();

                if let Some(model) = db_model {
                    if model
                        .to_lowercase()
                        .contains(&expected_embed.to_lowercase())
                        || expected_embed
                            .to_lowercase()
                            .contains(&model.to_lowercase())
                    {
                        check_7_status = CheckStatus::Ok;
                        check_7_detail = format!(
                            "Embedding model in database ('{}') is consistent with config ('{}')",
                            model, expected_embed
                        );
                    } else {
                        check_7_status = CheckStatus::Warn;
                        check_7_detail = format!("Embedding model mismatch: Database has '{}' but configuration expects '{}'", model, expected_embed);
                        check_7_hint = Some("Re-indexing memories or updating model configuration may be required to avoid vector mismatches.".to_string());
                    }
                } else {
                    check_7_status = CheckStatus::Warn;
                    check_7_detail =
                        "Table 'embedding_model_meta' exists but is empty or could not be read"
                            .to_string();
                    check_7_hint = Some(
                        "Ensure the embedding model metadata is correctly populated.".to_string(),
                    );
                }
            }
        }
    } else {
        check_7_status = CheckStatus::Warn;
        check_7_detail = "Skipped consistency check because database is not accessible".to_string();
    }

    let soft_check = DoctorCheck {
        name: "Embedding Model Consistency".to_string(),
        status: check_7_status,
        detail: check_7_detail,
        hint: check_7_hint,
    };

    if verbose {
        checks.push(soft_check);
    }

    let overall_status = if checks.iter().any(|c| matches!(c.status, CheckStatus::Fail)) {
        CheckStatus::Fail
    } else if checks.iter().any(|c| matches!(c.status, CheckStatus::Warn)) {
        CheckStatus::Warn
    } else {
        CheckStatus::Ok
    };

    let report = DoctorReport {
        checks: checks.clone(),
        overall: overall_status,
    };

    // Output formatting
    match format.as_str() {
        "json" => {
            let pretty_json = serde_json::to_string_pretty(&report)?;
            println!("{}", pretty_json);
        }
        "markdown" => {
            let markdown_table = format_as_markdown(&checks);
            println!("{}", markdown_table);
        }
        _ => {
            print_table_output(&checks);
        }
    }

    // Código salida 0 si todos los críticos (1-6) son Ok, 1 si alguno falla.
    let any_critical_failed = critical_checks
        .iter()
        .any(|c| matches!(c.status, CheckStatus::Fail));
    if any_critical_failed {
        std::process::exit(1);
    } else {
        std::process::exit(0);
    }
}

fn format_as_markdown(checks: &[DoctorCheck]) -> String {
    let mut lines = vec![
        "# Xavier Doctor Diagnostic Report".to_string(),
        "".to_string(),
        "| Status | Check | Detail | Hint |".to_string(),
        "| :---: | :--- | :--- | :--- |".to_string(),
    ];
    for check in checks {
        let icon = match check.status {
            CheckStatus::Ok => "✓",
            CheckStatus::Warn => "⚠",
            CheckStatus::Fail => "✗",
        };
        let hint_str = check.hint.as_deref().unwrap_or("");
        lines.push(format!(
            "| {} | {} | {} | {} |",
            icon, check.name, check.detail, hint_str
        ));
    }
    lines.join("\n")
}

fn print_table_output(checks: &[DoctorCheck]) {
    println!();
    println!("  Xavier Doctor Diagnostic Report");
    println!("{}", "=".repeat(120));
    println!("  {:<8} | {:<30} | {:<75}", "Status", "Check", "Detail");
    println!("{}", "=".repeat(120));
    for check in checks {
        let status_icon = match check.status {
            CheckStatus::Ok => "  [✓] OK",
            CheckStatus::Warn => "  [⚠] WARN",
            CheckStatus::Fail => "  [✗] FAIL",
        };
        println!("{:<8} | {:<30} | {}", status_icon, check.name, check.detail);
        if let Some(ref hint) = check.hint {
            println!("{:<8} | {:<30} | Hint: {}", "", "", hint);
        }
        println!("{}", "-".repeat(120));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doctor_report_serialization() {
        let report = DoctorReport {
            checks: vec![
                DoctorCheck {
                    name: "Test Check".to_string(),
                    status: CheckStatus::Ok,
                    detail: "All good".to_string(),
                    hint: None,
                },
                DoctorCheck {
                    name: "Warning Check".to_string(),
                    status: CheckStatus::Warn,
                    detail: "Attention required".to_string(),
                    hint: Some("Check logs".to_string()),
                },
            ],
            overall: CheckStatus::Warn,
        };

        let serialized = serde_json::to_string(&report);
        assert!(serialized.is_ok(), "Should serialize without panic");
        let json_str = serialized.unwrap();
        assert!(json_str.contains("Test Check"));
        assert!(json_str.contains("Warning Check"));
    }
}
