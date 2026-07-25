//! Environment detection for onboarding suggestions.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use xavier::secrets::vault::HardwareVault;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub installed: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub project_type: String,
    pub indicators: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingSuggestions {
    pub os: String,
    pub is_docker: bool,
    pub is_wsl: bool,
    pub tools: Vec<ToolInfo>,
    pub workspace: WorkspaceInfo,
    pub hardware: HardwareSpecs,
    pub model_recommendations: Vec<ModelRecommendation>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub os: String,
    pub hardware: HardwareSpecs,
    pub ollama: OllamaStatus,
    pub cli_agents: Vec<AgentInfo>,
    pub api_keys: Vec<ApiKeyInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaStatus {
    pub running: bool,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSpecs {
    pub ram_gb: f32,
    pub vram_gb: Option<f32>,
    pub gpu_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRecommendation {
    pub model: String,
    pub reason: String,
    pub capability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyInfo {
    pub name: String,
    pub detected: bool,
}

pub struct SystemScanner;

impl SystemScanner {
    /// Scan.
    pub async fn scan() -> ScanResult {
        let (gpu_name, vram_gb) = detect_gpu_and_vram();
        let ram_gb = detect_ram_gb();

        ScanResult {
            os: detect_os_detailed(),
            hardware: HardwareSpecs {
                ram_gb,
                vram_gb,
                gpu_name,
            },
            ollama: detect_ollama().await,
            cli_agents: detect_cli_agents(),
            api_keys: detect_api_keys(),
        }
    }
}

/// Detect os.
pub fn detect_os() -> String {
    std::env::consts::OS.to_string()
}

/// Detect os detailed.
pub fn detect_os_detailed() -> String {
    let os = std::env::consts::OS;
    match os {
        "windows" => {
            let output = Command::new("powershell")
                .args([
                    "-Command",
                    "(Get-CimInstance Win32_OperatingSystem).Caption",
                ])
                .output();
            if let Ok(out) = output {
                let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !name.is_empty() {
                    return name;
                }
            }
            "Windows".to_string()
        }
        "macos" => {
            let output = Command::new("sw_vers").arg("-productVersion").output();
            if let Ok(out) = output {
                let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
                return format!("macOS {}", version);
            }
            "macOS".to_string()
        }
        "linux" => {
            if let Ok(os_release) = std::fs::read_to_string("/etc/os-release") {
                for line in os_release.lines() {
                    if let Some(name) = line.strip_prefix("PRETTY_NAME=") {
                        return name.trim_matches('"').to_string();
                    }
                }
            }
            "Linux".to_string()
        }
        _ => os.to_string(),
    }
}

/// Detect gpu and vram.
pub fn detect_gpu_and_vram() -> (Option<String>, Option<f32>) {
    let os = std::env::consts::OS;
    let mut gpu_name = None;
    let mut vram_gb = None;

    if os == "windows" {
        // Try to get Name and AdapterRAM
        let output = Command::new("powershell")
            .args([
                "-Command",
                "Get-CimInstance Win32_VideoController | Select-Object Name, AdapterRAM | ConvertTo-Json"
            ])
            .output();

        if let Ok(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                // Could be an array of controllers or a single object
                let controller = if json.is_array() {
                    json.get(0)
                } else {
                    Some(&json)
                };

                if let Some(c) = controller {
                    if let Some(n) = c.get("Name").and_then(|v| v.as_str()) {
                        gpu_name = Some(n.to_string());
                    }
                    if let Some(ram) = c.get("AdapterRAM").and_then(|v| v.as_u64()) {
                        vram_gb = Some((ram as f64 / 1024.0 / 1024.0 / 1024.0) as f32);
                    }
                }
            }
        }
    } else if os == "linux" {
        // Try nvidia-smi first for detailed info
        let output = Command::new("nvidia-smi")
            .args([
                "--query-gpu=name,memory.total",
                "--format=csv,noheader,nounits",
            ])
            .output();
        if let Ok(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if let Some(line) = stdout.lines().next() {
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() == 2 {
                    gpu_name = Some(parts[0].trim().to_string());
                    if let Ok(mb) = parts[1].trim().parse::<f32>() {
                        vram_gb = Some(mb / 1024.0);
                    }
                }
            }
        }

        // Fallback to lspci if nvidia-smi fails
        if gpu_name.is_none() {
            let output = Command::new("sh")
                .arg("-c")
                .arg("lspci | grep -i vga")
                .output();
            if let Ok(out) = output {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if let Some(line) = stdout.lines().next() {
                    if let Some(pos) = line.find(": ") {
                        gpu_name = Some(line[pos + 2..].trim().to_string());
                    }
                }
            }
        }
    } else if os == "macos" {
        let output = Command::new("system_profiler")
            .arg("SPDisplaysDataType")
            .output();
        if let Ok(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                if let Some(pos) = line.find("Chipset Model: ") {
                    gpu_name = Some(line[pos + 15..].trim().to_string());
                }
                if let Some(pos) = line.find("VRAM (Total): ") {
                    let ram_str = line[pos + 14..].trim();
                    let num_str = ram_str.split(|c: char| !c.is_numeric()).next().unwrap_or("");
                    if let Ok(num) = num_str.parse::<f32>() {
                        if ram_str.contains("GB") {
                            vram_gb = Some(num);
                        } else if ram_str.contains("MB") {
                            vram_gb = Some(num / 1024.0);
                        }
                    }
                }
            }
        }
    }
    (gpu_name, vram_gb)
}

/// Detect ram gb.
pub fn detect_ram_gb() -> f32 {
    let os = std::env::consts::OS;
    if os == "windows" {
        let output = Command::new("powershell")
            .args([
                "-Command",
                "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
            ])
            .output();
        if let Ok(out) = output {
            let bytes_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Ok(bytes) = bytes_str.parse::<u64>() {
                return (bytes as f64 / 1024.0 / 1024.0 / 1024.0) as f32;
            }
        }
    } else if os == "linux" {
        let output = Command::new("free").arg("-m").output();
        if let Ok(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if let Some(mem_line) = stdout.lines().find(|l| l.starts_with("Mem:")) {
                let parts: Vec<&str> = mem_line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(mb) = parts[1].parse::<f32>() {
                        return mb / 1024.0;
                    }
                }
            }
        }
    } else if os == "macos" {
        let output = Command::new("sysctl").arg("-n").arg("hw.memsize").output();
        if let Ok(out) = output {
            let bytes_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Ok(bytes) = bytes_str.parse::<u64>() {
                return (bytes as f64 / 1024.0 / 1024.0 / 1024.0) as f32;
            }
        }
    }
    16.0 // Fallback assuming 16GB
}

/// Detect ollama.
pub async fn detect_ollama() -> OllamaStatus {
    let client = reqwest::Client::new();
    let resp = client.get("http://localhost:11434/api/tags").send().await;
    match resp {
        Ok(res) if res.status().is_success() => {
            #[derive(Deserialize)]
            struct OllamaTags {
                models: Vec<OllamaModel>,
            }
            #[derive(Deserialize)]
            struct OllamaModel {
                name: String,
            }
            if let Ok(tags) = res.json::<OllamaTags>().await {
                return OllamaStatus {
                    running: true,
                    models: tags.models.into_iter().map(|m| m.name).collect(),
                };
            }
            OllamaStatus {
                running: true,
                models: vec![],
            }
        }
        _ => OllamaStatus {
            running: false,
            models: vec![],
        },
    }
}

/// Detect cli agents.
pub fn detect_cli_agents() -> Vec<AgentInfo> {
    let agents = ["opencode", "codex", "claude", "copilot"];
    agents
        .iter()
        .map(|name| AgentInfo {
            name: name.to_string(),
            installed: check_command_exists(name),
        })
        .collect()
}

fn check_command_exists(cmd: &str) -> bool {
    #[cfg(windows)]
    let check_cmd = "where";
    #[cfg(not(windows))]
    let check_cmd = "which";

    Command::new(check_cmd)
        .arg(cmd)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Detect api keys.
pub fn detect_api_keys() -> Vec<ApiKeyInfo> {
    let keys = ["OPENAI", "GEMINI", "GROQ", "FIRECRAWL"];
    let vault = HardwareVault::new("xavier");

    keys.iter()
        .map(|name| {
            let env_name = format!("{}_API_KEY", name);
            let detected = std::env::var(&env_name).is_ok() || vault.get_secret(&env_name).is_ok();
            ApiKeyInfo {
                name: name.to_string(),
                detected,
            }
        })
        .collect()
}

/// Is wsl.
pub fn is_wsl() -> bool {
    if cfg!(target_os = "linux") {
        if let Ok(version) = std::fs::read_to_string("/proc/version") {
            return version.to_lowercase().contains("microsoft")
                || version.to_lowercase().contains("wsl");
        }
    }
    false
}

/// Is docker.
pub fn is_docker() -> bool {
    Path::new("/.dockerenv").exists()
}

/// Check tool.
pub fn check_tool(name: &str, arg: &str) -> ToolInfo {
    let output = Command::new(name).arg(arg).output();
    match output {
        Ok(out) if out.status.success() => {
            let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
            ToolInfo {
                name: name.to_string(),
                installed: true,
                version: Some(version),
            }
        }
        _ => ToolInfo {
            name: name.to_string(),
            installed: false,
            version: None,
        },
    }
}

/// Detect tools.
pub fn detect_tools() -> Vec<ToolInfo> {
    let tools_to_check = [
        ("rustc", "--version"),
        ("node", "--version"),
        ("python", "--version"),
        ("git", "--version"),
        ("docker", "--version"),
        ("ollama", "--version"),
    ];

    tools_to_check
        .iter()
        .map(|(name, arg)| check_tool(name, arg))
        .collect()
}

/// Detect workspace type.
pub fn detect_workspace_type(path: &Path) -> WorkspaceInfo {
    let mut indicators = Vec::new();
    let mut project_type = "Generic".to_string();

    if path.join("Cargo.toml").exists() {
        indicators.push("Cargo.toml".to_string());
        project_type = "Rust".to_string();
    }
    if path.join("package.json").exists() {
        indicators.push("package.json".to_string());
        if project_type == "Generic" {
            project_type = "Node.js".to_string();
        } else {
            project_type = format!("{} / Node.js", project_type);
        }
    }
    if path.join("go.mod").exists() {
        indicators.push("go.mod".to_string());
        project_type = "Go".to_string();
    }
    if path.join("requirements.txt").exists() || path.join("pyproject.toml").exists() {
        indicators.push("Python indicators".to_string());
        project_type = "Python".to_string();
    }
    if path.join(".git").exists() {
        indicators.push(".git".to_string());
    }

    WorkspaceInfo {
        project_type,
        indicators,
    }
}

/// Generate suggestions.
pub fn generate_suggestions(workspace_path: &Path) -> OnboardingSuggestions {
    let is_docker = is_docker();
    let is_wsl = is_wsl();
    let tools = detect_tools();
    let mut workspace = detect_workspace_type(workspace_path);

    // If no specific project type detected, try the current working directory
    if workspace.project_type == "Generic" {
        if let Ok(cwd) = std::env::current_dir() {
            let cwd_workspace = detect_workspace_type(&cwd);
            if cwd_workspace.project_type != "Generic" {
                workspace = cwd_workspace;
            }
        }
    }

    let (gpu_name, vram_gb) = detect_gpu_and_vram();
    let ram_gb = detect_ram_gb();
    let hardware = HardwareSpecs {
        ram_gb,
        vram_gb,
        gpu_name,
    };

    let mut recommendations = Vec::new();

    if workspace.project_type.contains("Rust") {
        recommendations.push("Rust project detected. Consider enabling code graph indexing (XAVIER_CODE_SCAN) for faster navigation.".to_string());
    }
    if workspace.project_type.contains("Node.js") {
        recommendations.push(
            "Node.js project detected. Xavier can help manage your npm dependencies and scripts."
                .to_string(),
        );
    }
    if workspace.project_type.contains("Python") {
        recommendations.push("Python project detected. Consider indexing your virtual environment for better symbol resolution.".to_string());
    }

    if !tools.iter().any(|t| t.name == "git" && t.installed) {
        recommendations.push("Git not found. It is highly recommended to use Git for version control and Xavier memory tracking.".to_string());
    }

    let os = detect_os();
    match os.as_str() {
        "windows" => {
            recommendations.push("Running on Windows. Ensure you have 'XAVIER_PORT' configured if 8006 is blocked.".to_string());
            recommendations.push("Consider using WSL2 for optimal performance with local LLMs and a more native development experience.".to_string());
        }
        "linux" => {
            if is_wsl {
                recommendations.push("WSL environment detected. Performance should be optimal for local model execution.".to_string());
            } else {
                recommendations.push("Linux environment detected. Performance should be optimal for local model execution with 'vec' backend.".to_string());
            }
        }
        "macos" => recommendations.push("macOS detected. Xavier supports Metal acceleration for compatible local models (e.g., via llama.cpp/Ollama).".to_string()),
        _ => {}
    }

    if tools.iter().any(|t| t.name == "docker" && t.installed) && !is_docker {
        recommendations.push("Docker detected. You can run Xavier in a container for better isolation and easy environment management.".to_string());
    }

    if tools.iter().any(|t| t.name == "ollama" && t.installed) {
        recommendations.push(
            "Ollama detected. Xavier can use it for running local LLMs with ease.".to_string(),
        );
        // Add specific recommendation for semantic compaction
        recommendations.push(
            "For Semantic Compaction tasks, a local model via Ollama is highly recommended to save on API token costs.".to_string(),
        );
    } else {
        recommendations.push(
            "No local LLM provider detected (like Ollama). For Semantic Compaction (which consumes many tokens), consider installing Ollama or providing an API key for a cloud model.".to_string(),
        );
    }

    let model_recommendations = generate_model_recommendations(&hardware);

    OnboardingSuggestions {
        os: detect_os_detailed(),
        is_docker,
        is_wsl,
        tools,
        workspace,
        hardware,
        model_recommendations,
        recommendations,
    }
}

/// Generate model recommendations.
pub fn generate_model_recommendations(hardware: &HardwareSpecs) -> Vec<ModelRecommendation> {
    let mut recs = Vec::new();
    let ram = hardware.ram_gb;
    let vram = hardware.vram_gb.unwrap_or(0.0);

    // canirun.ai logic adaptation
    if vram >= 16.0 || (ram >= 32.0 && vram >= 8.0) {
        recs.push(ModelRecommendation {
            model: "mixtral:8x7b-instruct-q4_K_M".to_string(),
            reason: "High-end hardware detected. Excellent reasoning capability.".to_string(),
            capability: "Complex agentic tasks, large context, expert coding".to_string(),
        });
        recs.push(ModelRecommendation {
            model: "command-r".to_string(),
            reason: "Plenty of VRAM for heavy models with long context.".to_string(),
            capability: "RAG, tool-use orchestration, large context".to_string(),
        });
    }

    if vram >= 8.0 || (ram >= 16.0) {
        recs.push(ModelRecommendation {
            model: "llama3:8b-instruct-q4_K_M".to_string(),
            reason: "Standard recommended hardware (8GB+ VRAM or 16GB+ RAM).".to_string(),
            capability: "General reasoning, coding, highly capable local agent".to_string(),
        });
        recs.push(ModelRecommendation {
            model: "qwen2.5:7b".to_string(),
            reason: "Fast and highly capable alternative to Llama 3.".to_string(),
            capability: "Coding, math, multilingual".to_string(),
        });
    }

    if vram >= 4.0 || ram >= 8.0 {
        recs.push(ModelRecommendation {
            model: "phi3:mini".to_string(),
            reason: "Low hardware footprint detected. Designed for limited RAM/VRAM.".to_string(),
            capability: "Basic reasoning, fast execution, low context window".to_string(),
        });
        recs.push(ModelRecommendation {
            model: "qwen2.5:1.5b".to_string(),
            reason: "Extremely lightweight. Perfect for low-end machines.".to_string(),
            capability: "Basic fast tasks, code autocomplete".to_string(),
        });
    }

    if recs.is_empty() {
        recs.push(ModelRecommendation {
            model: "tinydolphin".to_string(),
            reason: "Severely limited hardware detected.".to_string(),
            capability: "Basic formatting, extremely limited reasoning".to_string(),
        });
    }

    recs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_detect_os() {
        let os = detect_os();
        assert!(!os.is_empty());
    }

    #[test]
    fn test_detect_workspace_rust() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("Cargo.toml")).unwrap();
        let info = detect_workspace_type(dir.path());
        assert_eq!(info.project_type, "Rust");
        assert!(info.indicators.contains(&"Cargo.toml".to_string()));
    }

    #[test]
    fn test_is_docker_detection() {
        // Since we can't easily create /.dockerenv in a test environment without root,
        // we just verify it doesn't crash and returns a boolean.
        let _ = is_docker();
    }

    #[test]
    fn test_is_wsl_detection() {
        // Verify it doesn't crash.
        let _ = is_wsl();
    }

    #[test]
    fn test_generate_suggestions_basic() {
        let dir = tempdir().unwrap();
        let suggestions = generate_suggestions(dir.path());

        assert!(!suggestions.os.is_empty());
        // Should at least have OS specific recommendations
        assert!(!suggestions.recommendations.is_empty());
    }
}
