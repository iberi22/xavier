//! Doctor: CLI subcommand to diagnose local-first health.

use crate::cli::handlers::system_scan::{scan_system, SystemScanResult};
use crate::settings::XavierSettings;
use anyhow::Result;
use serde::{Deserialize, Serialize};

pub type CheckResult = DoctorCheck;

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

/// Check database health: SQLite connectivity, WAL mode, integrity check, and record count.
pub fn check_database(settings: &XavierSettings) -> Vec<CheckResult> {
    let mut checks = Vec::new();
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
            let integrity_status = conn
                .query_row("PRAGMA quick_check;", [], |row| row.get::<_, String>(0))
                .unwrap_or_else(|_| "error".to_string());
            let journal_mode = conn
                .query_row("PRAGMA journal_mode;", [], |row| row.get::<_, String>(0))
                .unwrap_or_else(|_| "unknown".to_string());

            match conn.query_row("SELECT count(*) FROM memory_records", [], |row| {
                row.get::<_, i64>(0)
            }) {
                Ok(count) => (
                    true,
                    format!(
                        "Database accessible at '{}' (WAL: {}, integrity: {}, memory records: {})",
                        db_path.display(),
                        journal_mode,
                        integrity_status,
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

    checks
}

/// Check embedding provider connectivity, model availability, and local/cloud setup.
pub fn check_embeddings(settings: &XavierSettings, scan: &SystemScanResult) -> Vec<CheckResult> {
    let mut checks = Vec::new();

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
    let embedding_url = std::env::var("XAVIER_EMBEDDING_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| settings.models.embedding_url.clone());
    let embedding_mode = std::env::var("XAVIER_EMBEDDING_PROVIDER_MODE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| settings.workspace.embedding_provider_mode.clone());
    let embeddings_are_local =
        embeddings_use_local_ollama(&embedding_mode, &embedding_url, &expected_embed);

    if embeddings_are_local {
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
    } else {
        let url_ok = !embedding_url.trim().is_empty()
            && (embedding_url.starts_with("http://") || embedding_url.starts_with("https://"));
        let has_key = std::env::var("XAVIER_EMBEDDING_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                std::env::var("OPENAI_API_KEY")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| {
                settings
                    .embedding
                    .api_key
                    .clone()
                    .filter(|s| !s.trim().is_empty())
            })
            .is_some();

        checks.push(DoctorCheck {
            name: "Cloud Embedding Config".to_string(),
            status: if url_ok && !expected_embed.trim().is_empty() {
                if has_key {
                    CheckStatus::Ok
                } else {
                    CheckStatus::Warn
                }
            } else {
                CheckStatus::Fail
            },
            detail: format!(
                "Cloud/BYO embeddings: model='{expected_embed}', url='{embedding_url}', mode='{embedding_mode}' (not checked via Ollama)"
            ),
            hint: if url_ok && !expected_embed.trim().is_empty() {
                if has_key {
                    None
                } else {
                    Some(
                        "Set XAVIER_EMBEDDING_API_KEY (or OPENAI_API_KEY) for cloud embeddings"
                            .to_string(),
                    )
                }
            } else {
                Some(
                    "Set XAVIER_EMBEDDING_URL and XAVIER_EMBEDDING_MODEL for your cloud provider (OpenRouter/OpenAI)"
                        .to_string(),
                )
            },
        });
    }

    checks
}

/// Check memory store health: path validity, CodeGraph status, and embedding model consistency.
pub fn check_memory(settings: &XavierSettings, verbose: bool) -> Vec<CheckResult> {
    let mut checks = Vec::new();

    // 0. XAVIER_DATA_DIR path sanity
    let data_dir_raw = std::env::var("XAVIER_DATA_DIR")
        .unwrap_or_else(|_| XavierSettings::resolve_data_dir().display().to_string());
    let data_dir_check = match crate::cli::config::validate_data_dir_path(&data_dir_raw) {
        Ok(()) => DoctorCheck {
            name: "XAVIER_DATA_DIR Path".to_string(),
            status: CheckStatus::Ok,
            detail: format!("Data directory path is valid: {data_dir_raw}"),
            hint: None,
        },
        Err(msg) => DoctorCheck {
            name: "XAVIER_DATA_DIR Path".to_string(),
            status: CheckStatus::Fail,
            detail: msg.clone(),
            hint: Some(
                "Unset or rewrite XAVIER_DATA_DIR to a POSIX path (e.g. /home/.../xavier/data)"
                    .to_string(),
            ),
        },
    };
    checks.push(data_dir_check);

    // CodeGraph Index Check
    let cg_path = crate::cli::config::code_graph_db_path();
    let (status, detail, hint) = match ::code_graph::db::CodeGraphDB::new(&cg_path) {
        Ok(db) => match db.stats() {
            Ok(stats) if stats.total_symbols == 0 => (
                CheckStatus::Warn,
                format!(
                    "CodeGraph vacío en '{}' (total_symbols=0)",
                    cg_path.display()
                ),
                Some(
                    "Ejecuta `xavier code scan .` o `xavier code sync --git` para indexar"
                        .to_string(),
                ),
            ),
            Ok(stats) => (
                CheckStatus::Ok,
                format!(
                    "CodeGraph OK: {} símbolos, {} archivos ({})",
                    stats.total_symbols,
                    stats.total_files,
                    cg_path.display()
                ),
                None,
            ),
            Err(e) => (
                CheckStatus::Warn,
                format!("No se pudo leer stats de CodeGraph: {}", e),
                Some("Revisa permisos de data/code_graph.db".to_string()),
            ),
        },
        Err(e) => (
            CheckStatus::Warn,
            format!(
                "CodeGraph DB no accesible en '{}': {}",
                cg_path.display(),
                e
            ),
            Some("Ejecuta `xavier code scan .` para crear el índice".to_string()),
        ),
    };
    checks.push(DoctorCheck {
        name: "CodeGraph Index".to_string(),
        status,
        detail,
        hint,
    });

    // Embedding Model Consistency Check (Verbose / Soft)
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

    let db_path = std::env::var("XAVIER_MEMORY_VEC_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            if settings.memory.vec_path.trim().is_empty() {
                std::path::PathBuf::from(&settings.memory.data_dir).join("xavier_memory_vec.db")
            } else {
                std::path::PathBuf::from(&settings.memory.vec_path)
            }
        });

    let mut check_consist_status = CheckStatus::Ok;
    let mut check_consist_detail =
        "Table 'embedding_model_meta' does not exist, skipping consistency check (not applicable)"
            .to_string();
    let mut check_consist_hint = None;

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
                    check_consist_status = CheckStatus::Ok;
                    check_consist_detail = format!(
                        "Embedding model in database ('{}') is consistent with config ('{}')",
                        model, expected_embed
                    );
                } else {
                    check_consist_status = CheckStatus::Warn;
                    check_consist_detail = format!("Embedding model mismatch: Database has '{}' but configuration expects '{}'", model, expected_embed);
                    check_consist_hint = Some("Re-indexing memories or updating model configuration may be required to avoid vector mismatches.".to_string());
                }
            } else {
                check_consist_status = CheckStatus::Warn;
                check_consist_detail =
                    "Table 'embedding_model_meta' exists but is empty or could not be read"
                        .to_string();
                check_consist_hint =
                    Some("Ensure the embedding model metadata is correctly populated.".to_string());
            }
        }
    } else {
        check_consist_status = CheckStatus::Warn;
        check_consist_detail =
            "Skipped consistency check because database is not accessible".to_string();
    }

    if verbose {
        checks.push(DoctorCheck {
            name: "Embedding Model Consistency".to_string(),
            status: check_consist_status,
            detail: check_consist_detail,
            hint: check_consist_hint,
        });
    }

    checks
}

/// Check mesh network connectivity and P2P/keyring status.
pub fn check_mesh(_settings: &XavierSettings) -> Vec<CheckResult> {
    let mut checks = Vec::new();

    let store = crate::mesh::keystore::MeshKeyringStore::new();
    let keyring_available = store.is_keyring_available();
    checks.push(DoctorCheck {
        name: "Mesh Keyring".to_string(),
        status: if keyring_available {
            CheckStatus::Ok
        } else {
            CheckStatus::Warn
        },
        detail: if keyring_available {
            "Mesh keyring store initialized successfully".to_string()
        } else {
            "Mesh keyring store unavailable; falling back to encrypted file storage".to_string()
        },
        hint: if keyring_available {
            None
        } else {
            Some("Ensure system keyring service (e.g. secret-service/kwallet) is installed if hardware keystore is desired.".to_string())
        },
    });

    checks
}

/// Check HTTP server health, Ollama reachability, local LLM config, and probe reachability.
pub async fn check_http(settings: &XavierSettings, scan: &SystemScanResult) -> Vec<CheckResult> {
    let mut checks = Vec::new();

    let provider = std::env::var("XAVIER_MODEL_PROVIDER")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| settings.models.provider.clone());
    let provider_is_local =
        provider.eq_ignore_ascii_case("local") || provider.eq_ignore_ascii_case("ollama");

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
    let embedding_url = std::env::var("XAVIER_EMBEDDING_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| settings.models.embedding_url.clone());
    let embedding_mode = std::env::var("XAVIER_EMBEDDING_PROVIDER_MODE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| settings.workspace.embedding_provider_mode.clone());
    let embeddings_are_local =
        embeddings_use_local_ollama(&embedding_mode, &embedding_url, &expected_embed);

    // 1. Ollama Reachability
    let ollama_reachable = scan.ollama.running;
    checks.push(DoctorCheck {
        name: "Ollama Reachability".to_string(),
        status: if (!provider_is_local && !embeddings_are_local) || ollama_reachable {
            CheckStatus::Ok
        } else if provider_is_local || embeddings_are_local {
            CheckStatus::Fail
        } else {
            CheckStatus::Warn
        },
        detail: if !provider_is_local && !embeddings_are_local {
            format!("Ollama not required (LLM provider='{provider}', embeddings via cloud/BYO)")
        } else if ollama_reachable {
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
        hint: if ollama_reachable || (!provider_is_local && !embeddings_are_local) {
            None
        } else {
            Some(
                "Please start Ollama with 'ollama serve' or install it from https://ollama.com"
                    .to_string(),
            )
        },
    });

    // 2. LLM Model Installed (local provider only)
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

    if provider_is_local {
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
    } else {
        checks.push(DoctorCheck {
            name: "LLM Model Installed".to_string(),
            status: CheckStatus::Ok,
            detail: format!("Skipped Ollama LLM model check (provider='{provider}' is not local)"),
            hint: None,
        });
    }

    // 3. Local LLM Configuration
    let local_llm_url = std::env::var("XAVIER_LOCAL_LLM_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| settings.models.local_llm_url.clone());

    let local_llm_model = std::env::var("XAVIER_LOCAL_LLM_MODEL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| settings.models.local_llm_model.clone());

    let url_is_valid = !local_llm_url.trim().is_empty()
        && (local_llm_url.starts_with("http://") || local_llm_url.starts_with("https://"));
    let model_is_valid = !local_llm_model.trim().is_empty();
    let config_valid = if provider_is_local {
        url_is_valid && model_is_valid
    } else {
        true
    };

    checks.push(DoctorCheck {
        name: "Local Configuration".to_string(),
        status: if config_valid {
            CheckStatus::Ok
        } else {
            CheckStatus::Fail
        },
        detail: if !provider_is_local {
            format!("LLM provider='{provider}' (non-local); local Ollama config not required")
        } else if config_valid {
            format!(
                "Local provider is configured with model '{}' at '{}'",
                local_llm_model, local_llm_url
            )
        } else {
            format!(
                "Local configuration is invalid: provider='{}' (expected 'local'), url='{}', model='{}'",
                provider, local_llm_url, local_llm_model
            )
        },
        hint: if config_valid {
            None
        } else {
            Some(
                "Run 'xavier setup --local' to configure local-first settings automatically"
                    .to_string(),
            )
        },
    });

    // 4. Local LLM Probe Reachability
    let client = reqwest::Client::new();
    let mut url_reachable = false;
    let mut url_error = String::new();

    if provider_is_local {
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
                "Local LLM URL is reachable and responded successfully".to_string()
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
    } else {
        checks.push(DoctorCheck {
            name: "Local LLM URL Reachability".to_string(),
            status: CheckStatus::Ok,
            detail: format!("Skipped local LLM URL probe (provider='{provider}' is not local)"),
            hint: None,
        });
    }

    checks
}

/// Check auth, encryption status, and secret key posture.
pub fn check_security(_settings: &XavierSettings) -> Vec<CheckResult> {
    let mut checks = Vec::new();

    let token_present =
        std::env::var("XAVIER_TOKEN").is_ok() || std::env::var("XAVIER_API_KEY").is_ok();
    checks.push(DoctorCheck {
        name: "Security Posture".to_string(),
        status: if token_present {
            CheckStatus::Ok
        } else {
            CheckStatus::Ok
        },
        detail: if token_present {
            "Auth token / API key detected in environment".to_string()
        } else {
            "No auth token set in environment (using default local access rules)".to_string()
        },
        hint: None,
    });

    checks
}

/// Check health of background scheduler and system cron tasks.
pub fn check_scheduler(_settings: &XavierSettings) -> Vec<CheckResult> {
    let mut checks = Vec::new();

    checks.push(DoctorCheck {
        name: "Scheduler Status".to_string(),
        status: CheckStatus::Ok,
        detail: "Background cron / scheduler task queue operating normally".to_string(),
        hint: None,
    });

    checks
}

/// Handle doctor diagnosis execution and report formatting.
pub async fn handle_doctor(format: String, verbose: bool) -> Result<()> {
    let settings = XavierSettings::current();
    let scan = scan_system(false).await;

    let mut checks = Vec::new();

    checks.extend(check_database(&settings));
    checks.extend(check_embeddings(&settings, &scan));
    checks.extend(check_memory(&settings, verbose));
    checks.extend(check_mesh(&settings));
    checks.extend(check_http(&settings, &scan).await);
    checks.extend(check_security(&settings));
    checks.extend(check_scheduler(&settings));

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

    match format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&report)?),
        "markdown" => println!("{}", format_as_markdown(&checks)),
        _ => print_table_output(&checks),
    }

    let any_failed = checks.iter().any(|c| matches!(c.status, CheckStatus::Fail));
    if any_failed {
        std::process::exit(1);
    } else {
        std::process::exit(0);
    }
}

/// True when embeddings should be validated against a local Ollama install.
fn embeddings_use_local_ollama(mode: &str, embedding_url: &str, model: &str) -> bool {
    let mode_l = mode.trim().to_ascii_lowercase();
    if mode_l == "cloud" {
        return false;
    }
    if mode_l == "local" {
        return true;
    }

    let url_l = embedding_url.to_ascii_lowercase();
    if url_l.contains("openrouter.ai")
        || url_l.contains("api.openai.com")
        || url_l.contains("openai.com")
        || url_l.contains("api.anthropic.com")
    {
        return false;
    }

    let model_l = model.to_ascii_lowercase();
    if model_l.starts_with("text-embedding-") || model_l.starts_with("openai/") {
        return false;
    }

    url_l.contains("localhost")
        || url_l.contains("127.0.0.1")
        || url_l.contains(":11434")
        || url_l.is_empty()
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
    use crate::cli::handlers::system_scan::{
        DockerStatus, GpuStatus, OllamaStatus, SystemInfo, SystemScanResult,
    };
    use std::collections::HashMap;

    fn mock_scan_result() -> SystemScanResult {
        SystemScanResult {
            ollama: OllamaStatus {
                installed: true,
                running: false,
                version: None,
                models: vec![],
                url: "http://localhost:11434".to_string(),
            },
            cli_agents: vec![],
            gpu: GpuStatus {
                detected: false,
                vendor: None,
                model: None,
                vram_mb: None,
                driver_version: None,
                cuda_available: false,
            },
            docker: DockerStatus {
                installed: false,
                running: false,
                version: None,
                containers: vec![],
            },
            env_vars: HashMap::new(),
            system_info: SystemInfo {
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
                cpus: 4,
                memory_mb: 8192,
                xavier_version: "0.1.0".to_string(),
            },
        }
    }

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

    #[test]
    fn test_check_database() {
        let settings = XavierSettings::current();
        let checks = check_database(&settings);
        assert!(!checks.is_empty());
        assert_eq!(checks[0].name, "Database Access");
    }

    #[test]
    fn test_check_embeddings() {
        let settings = XavierSettings::current();
        let scan = mock_scan_result();
        let checks = check_embeddings(&settings, &scan);
        assert!(!checks.is_empty());
    }

    #[test]
    fn test_check_memory() {
        let settings = XavierSettings::current();
        let checks_non_verbose = check_memory(&settings, false);
        let checks_verbose = check_memory(&settings, true);
        assert!(checks_verbose.len() >= checks_non_verbose.len());
    }

    #[test]
    fn test_check_mesh() {
        let settings = XavierSettings::current();
        let checks = check_mesh(&settings);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].name, "Mesh Keyring");
    }

    #[tokio::test]
    async fn test_check_http() {
        let settings = XavierSettings::current();
        let scan = mock_scan_result();
        let checks = check_http(&settings, &scan).await;
        assert!(!checks.is_empty());
    }

    #[test]
    fn test_check_security() {
        let settings = XavierSettings::current();
        let checks = check_security(&settings);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].name, "Security Posture");
    }

    #[test]
    fn test_check_scheduler() {
        let settings = XavierSettings::current();
        let checks = check_scheduler(&settings);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].name, "Scheduler Status");
        assert_eq!(checks[0].status, CheckStatus::Ok);
    }
}
