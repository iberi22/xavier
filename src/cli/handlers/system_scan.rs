// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! SystemScanner: Detect CLI agents, local models, GPU, env vars, and login status

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

/// Result of a system scan
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SystemScanResult {
    pub ollama: OllamaStatus,
    pub cli_agents: Vec<CliAgentStatus>,
    pub gpu: GpuStatus,
    pub docker: DockerStatus,
    pub env_vars: HashMap<String, EnvVarStatus>,
    pub system_info: SystemInfo,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OllamaStatus {
    pub installed: bool,
    pub running: bool,
    pub version: Option<String>,
    pub models: Vec<String>,
    pub url: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CliAgentStatus {
    pub name: String,
    pub installed: bool,
    pub version: Option<String>,
    pub logged_in: bool,
    pub config_path: Option<String>,
    pub usage_tier: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GpuStatus {
    pub detected: bool,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub vram_mb: Option<u64>,
    pub driver_version: Option<String>,
    pub cuda_available: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DockerStatus {
    pub installed: bool,
    pub running: bool,
    pub version: Option<String>,
    pub containers: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EnvVarStatus {
    pub present: bool,
    pub masked_value: Option<String>,
    pub source: String, // "env", "vault", "config"
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub cpus: usize,
    pub memory_mb: u64,
    pub xavier_version: String,
}

/// Run full system scan
pub async fn scan_system(detailed: bool) -> SystemScanResult {
    let ollama = detect_ollama().await;
    let cli_agents = detect_cli_agents(detailed).await;
    let gpu = detect_gpu();
    let docker = detect_docker().await;
    let env_vars = detect_env_vars(detailed).await;
    let system_info = gather_system_info();

    SystemScanResult {
        ollama,
        cli_agents,
        gpu,
        docker,
        env_vars,
        system_info,
    }
}

async fn detect_ollama() -> OllamaStatus {
    let base_url =
        std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let client = reqwest::Client::new();

    // Try to get version
    let version = match client.get(format!("{}/api/version", base_url)).send().await {
        Ok(resp) => resp.json::<serde_json::Value>().await.ok().and_then(|v| {
            v.get("version")
                .and_then(|v| v.as_str().map(|s| s.to_string()))
        }),
        Err(_) => None,
    };

    // Try to list models using /api/tags (Ollama native)
    let mut models = match client.get(format!("{}/api/tags", base_url)).send().await {
        Ok(resp) => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                json.get("models")
                    .and_then(|m| m.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|m| {
                                m.get("name")
                                    .and_then(|n| n.as_str().map(|s| s.to_string()))
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                vec![]
            }
        }
        Err(_) => vec![],
    };

    // Fallback or alternative: try /v1/models (OpenAI compatible)
    if models.is_empty() {
        if let Ok(resp) = client.get(format!("{}/v1/models", base_url)).send().await {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
                    models = data
                        .iter()
                        .filter_map(|m| m.get("id").and_then(|n| n.as_str().map(|s| s.to_string())))
                        .collect();
                }
            }
        }
    }

    let running = version.is_some() || !models.is_empty();
    let installed = running
        || Command::new("ollama")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

    OllamaStatus {
        installed,
        running,
        version,
        models,
        url: base_url,
    }
}

async fn detect_cli_agents(detailed: bool) -> Vec<CliAgentStatus> {
    let mut agents = vec![];

    // Claude CLI
    agents.push(detect_claude_cli(detailed).await);

    // OpenAI CLI
    agents.push(detect_openai_cli(detailed).await);

    // Aider
    agents.push(detect_generic_cli("aider", detailed).await);

    // Continue.dev
    agents.push(detect_generic_cli("continue", detailed).await);

    agents
}

async fn detect_claude_cli(detailed: bool) -> CliAgentStatus {
    let version = Command::new("claude")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    let installed = version.is_some();

    // Check login via config
    let config_path = dirs::config_dir().map(|p| {
        p.join("anthropic")
            .join("settings.json")
            .to_string_lossy()
            .to_string()
    });

    let logged_in = if let Some(ref path) = config_path {
        tokio::fs::metadata(path).await.is_ok()
    } else {
        false
    };

    let usage_tier = if detailed && logged_in {
        // Try to get tier from config
        if let Some(ref path) = config_path {
            tokio::fs::read_to_string(path)
                .await
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| {
                    v.get("tier")
                        .and_then(|t| t.as_str().map(|s| s.to_string()))
                })
        } else {
            None
        }
    } else {
        None
    };

    CliAgentStatus {
        name: "Claude CLI".to_string(),
        installed,
        version,
        logged_in,
        config_path: config_path.filter(|_| detailed),
        usage_tier,
    }
}

async fn detect_openai_cli(_detailed: bool) -> CliAgentStatus {
    let version = Command::new("openai")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    let installed = version.is_some();

    // Check if API key works
    let logged_in = if installed {
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            !key.is_empty()
        } else {
            false
        }
    } else {
        false
    };

    CliAgentStatus {
        name: "OpenAI CLI".to_string(),
        installed,
        version,
        logged_in,
        config_path: None,
        usage_tier: None,
    }
}

async fn detect_generic_cli(name: &str, _detailed: bool) -> CliAgentStatus {
    let version = Command::new(name)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    CliAgentStatus {
        name: name.to_string(),
        installed: version.is_some(),
        version,
        logged_in: false,
        config_path: None,
        usage_tier: None,
    }
}

fn detect_gpu() -> GpuStatus {
    // Try nvidia-smi first (Windows/Linux)
    let nvidia = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total,driver_version",
            "--format=csv,noheader",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok());

    if let Some(output) = nvidia {
        let parts: Vec<&str> = output.trim().split(',').map(|s| s.trim()).collect();
        if parts.len() >= 3 {
            let vram_str = parts[1]
                .replace("MiB", "")
                .replace("MB", "")
                .trim()
                .to_string();
            let vram_mb = vram_str.parse().ok();

            return GpuStatus {
                detected: true,
                vendor: Some("NVIDIA".to_string()),
                model: Some(parts[0].to_string()),
                vram_mb,
                driver_version: Some(parts[2].to_string()),
                cuda_available: Command::new("nvcc")
                    .arg("--version")
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false),
            };
        }
    }

    // Try AMD (rocm-smi)
    let amd = Command::new("rocm-smi")
        .arg("--showproductname")
        .output()
        .ok()
        .filter(|o| o.status.success());

    if amd.is_some() {
        return GpuStatus {
            detected: true,
            vendor: Some("AMD".to_string()),
            model: Some("ROCm GPU".to_string()),
            vram_mb: None,
            driver_version: None,
            cuda_available: false,
        };
    }

    // macOS Metal
    #[cfg(target_os = "macos")]
    {
        let metal = Command::new("system_profiler")
            .args(&["SPDisplaysDataType", "-json"])
            .output()
            .ok()
            .filter(|o| o.status.success());

        if metal.is_some() {
            return GpuStatus {
                detected: true,
                vendor: Some("Apple".to_string()),
                model: Some("Metal".to_string()),
                vram_mb: None,
                driver_version: None,
                cuda_available: false,
            };
        }
    }

    GpuStatus {
        detected: false,
        vendor: None,
        model: None,
        vram_mb: None,
        driver_version: None,
        cuda_available: false,
    }
}

async fn detect_docker() -> DockerStatus {
    let version = Command::new("docker")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    let installed = version.is_some();

    let running = if installed {
        Command::new("docker")
            .arg("info")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    } else {
        false
    };

    let containers = if running {
        Command::new("docker")
            .args(["ps", "--format", "{{.Names}}"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.lines().map(|l| l.to_string()).collect())
            .unwrap_or_default()
    } else {
        vec![]
    };

    DockerStatus {
        installed,
        running,
        version,
        containers,
    }
}

async fn detect_env_vars(detailed: bool) -> HashMap<String, EnvVarStatus> {
    let mut vars = HashMap::new();

    let keys = vec![
        "XAVIER_TOKEN",
        "XAVIER_API_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "GROQ_API_KEY",
        "DEEPSEEK_API_KEY",
        "GEMINI_API_KEY",
        "MINIMAX_API_KEY",
        "OLLAMA_HOST",
        "XAVIER_PROVIDER",
        "XAVIER_MODEL",
    ];

    for key in keys {
        let value = std::env::var(key).ok();
        let present = value.is_some();

        let masked_value = if detailed && present {
            value.map(|v| {
                if v.len() > 8 {
                    format!("{}...{}", &v[..4], &v[v.len() - 4..])
                } else {
                    "****".to_string()
                }
            })
        } else {
            None
        };

        vars.insert(
            key.to_string(),
            EnvVarStatus {
                present,
                masked_value,
                source: if present {
                    "env".to_string()
                } else {
                    "none".to_string()
                },
            },
        );
    }

    vars
}

pub fn gather_system_info() -> SystemInfo {
    SystemInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpus: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
        memory_mb: sysinfo::System::new_all().total_memory() / 1024,
        xavier_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Format scan result as table
#[allow(dead_code)]
pub fn format_as_table(result: &SystemScanResult) -> String {
    let mut lines = vec![
        "╔═══════════════════════════════════════════════════════════════╗".to_string(),
        "║                    SYSTEM SCAN RESULTS                        ║".to_string(),
        "╠═══════════════════════════════════════════════════════════════╣".to_string(),
    ];

    // Ollama
    lines.push(format!(
        "║  Ollama        {}  v{}  {} models",
        if result.ollama.running {
            "🟢"
        } else if result.ollama.installed {
            "🟡"
        } else {
            "🔴"
        },
        result.ollama.version.as_deref().unwrap_or("?"),
        result.ollama.models.len()
    ));

    // CLI Agents
    for agent in &result.cli_agents {
        lines.push(format!(
            "║  {:15} {}  v{}  {}",
            agent.name,
            if agent.installed {
                if agent.logged_in {
                    "🟢"
                } else {
                    "🟡"
                }
            } else {
                "🔴"
            },
            agent.version.as_deref().unwrap_or("?"),
            if agent.logged_in {
                "logged in"
            } else {
                "not logged in"
            }
        ));
    }

    // GPU
    lines.push(format!(
        "║  GPU           {}  {}",
        if result.gpu.detected { "🟢" } else { "🔴" },
        result.gpu.model.as_deref().unwrap_or("not detected")
    ));

    // Docker
    lines.push(format!(
        "║  Docker        {}  {}",
        if result.docker.running {
            "🟢"
        } else if result.docker.installed {
            "🟡"
        } else {
            "🔴"
        },
        result.docker.version.as_deref().unwrap_or("not installed")
    ));

    // Env vars summary
    let present_count = result.env_vars.values().filter(|v| v.present).count();
    lines.push(format!(
        "║  API Keys      {}/{} configured",
        present_count,
        result.env_vars.len()
    ));

    lines.push("╚═══════════════════════════════════════════════════════════════╝".to_string());
    lines.join("\n")
}

/// Format scan result as JSON
pub fn format_as_json(result: &SystemScanResult) -> String {
    serde_json::to_string_pretty(result).unwrap_or_else(|_| "{}".to_string())
}

/// Format scan result as Markdown
#[allow(dead_code)]
pub fn format_as_markdown(result: &SystemScanResult) -> String {
    let mut md = vec!["# System Scan Results\n".to_string()];

    md.push("## Ollama".to_string());
    md.push(format!(
        "- Status: {}",
        if result.ollama.running {
            "✅ Running"
        } else if result.ollama.installed {
            "⚠️ Installed but not running"
        } else {
            "❌ Not installed"
        }
    ));
    md.push(format!(
        "- Version: {}",
        result.ollama.version.as_deref().unwrap_or("N/A")
    ));
    md.push(format!("- Models: {}", result.ollama.models.join(", ")));

    md.push("\n## CLI Agents".to_string());
    for agent in &result.cli_agents {
        md.push(format!("### {}", agent.name));
        md.push(format!(
            "- Installed: {}",
            if agent.installed { "✅" } else { "❌" }
        ));
        md.push(format!(
            "- Version: {}",
            agent.version.as_deref().unwrap_or("N/A")
        ));
        md.push(format!(
            "- Logged in: {}",
            if agent.logged_in { "✅" } else { "❌" }
        ));
        if let Some(ref tier) = agent.usage_tier {
            md.push(format!("- Tier: {}", tier));
        }
    }

    md.push("\n## GPU".to_string());
    md.push(format!(
        "- Detected: {}",
        if result.gpu.detected { "✅" } else { "❌" }
    ));
    if let Some(ref vendor) = result.gpu.vendor {
        md.push(format!("- Vendor: {}", vendor));
    }
    if let Some(ref model) = result.gpu.model {
        md.push(format!("- Model: {}", model));
    }

    md.push("\n## Environment Variables".to_string());
    for (key, status) in &result.env_vars {
        md.push(format!(
            "- `{}`: {}",
            key,
            if status.present {
                "✅ set"
            } else {
                "❌ not set"
            }
        ));
    }

    md.join("\n")
}

#[cfg(test)]
mod scanner_tests {
    use super::*;
    use mockito;

    #[tokio::test]
    async fn test_ollama_detection_with_models() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        std::env::set_var("OLLAMA_HOST", &url);

        let _v_mock = server
            .mock("GET", "/api/version")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"version": "0.1.48"}"#)
            .create_async()
            .await;

        let _m_mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"models": [{"name": "qwen3-coder:latest"}, {"name": "llama3:latest"}]}"#)
            .create_async()
            .await;

        let status = detect_ollama().await;
        assert!(status.running);
        assert!(status.version.is_some());
        assert_eq!(status.version.unwrap(), "0.1.48");
        assert!(status.models.contains(&"qwen3-coder:latest".to_string()));
        assert!(status.models.contains(&"llama3:latest".to_string()));
    }

    #[tokio::test]
    async fn test_ollama_v1_models_fallback() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        std::env::set_var("OLLAMA_HOST", &url);

        let _v_mock = server
            .mock("GET", "/api/version")
            .with_status(200)
            .with_body(r#"{"version": "0.1.48"}"#)
            .create_async()
            .await;

        // /api/tags returns empty or fails
        let _m_tags_mock = server
            .mock("GET", "/api/tags")
            .with_status(404)
            .create_async()
            .await;

        // /v1/models (OpenAI compatible) succeeds
        let _m_v1_mock = server
            .mock("GET", "/v1/models")
            .with_status(200)
            .with_body(r#"{"data": [{"id": "qwen3-coder"}]}"#)
            .create_async()
            .await;

        let status = detect_ollama().await;
        assert!(status.running);
        assert!(status.models.contains(&"qwen3-coder".to_string()));
    }
}
