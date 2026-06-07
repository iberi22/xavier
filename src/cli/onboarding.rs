//! Environment detection for onboarding suggestions.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

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
    pub tools: Vec<ToolInfo>,
    pub workspace: WorkspaceInfo,
    pub recommendations: Vec<String>,
}

pub fn detect_os() -> String {
    std::env::consts::OS.to_string()
}

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

pub fn detect_tools() -> Vec<ToolInfo> {
    let tools_to_check = [
        ("rustc", "--version"),
        ("node", "--version"),
        ("python", "--version"),
        ("git", "--version"),
        ("docker", "--version"),
    ];

    tools_to_check
        .iter()
        .map(|(name, arg)| check_tool(name, arg))
        .collect()
}

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

pub fn generate_suggestions(workspace_path: &Path) -> OnboardingSuggestions {
    let os = detect_os();
    let tools = detect_tools();

    // Check the provided workspace path
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

    let mut recommendations = Vec::new();

    if workspace.project_type.contains("Rust") {
        recommendations.push("Rust project detected. Consider enabling code graph indexing (XAVIER_CODE_SCAN) for faster navigation.".to_string());
    }
    if workspace.project_type.contains("Node.js") {
        recommendations.push("Node.js project detected. Xavier can help manage your npm dependencies and scripts.".to_string());
    }
    if workspace.project_type.contains("Python") {
        recommendations.push("Python project detected. Consider indexing your virtual environment for better symbol resolution.".to_string());
    }

    if !tools.iter().any(|t| t.name == "git" && t.installed) {
        recommendations.push("Git not found. It is highly recommended to use Git for version control and Xavier memory tracking.".to_string());
    }

    match os.as_str() {
        "windows" => {
            recommendations.push("Running on Windows. Ensure you have 'XAVIER_PORT' configured if 8006 is blocked.".to_string());
            recommendations.push("Consider using WSL2 for optimal performance with local LLMs.".to_string());
        },
        "linux" => recommendations.push("Linux environment detected. Performance should be optimal for local model execution with 'vec' backend.".to_string()),
        "macos" => recommendations.push("macOS detected. Xavier supports Metal acceleration for compatible local models (e.g., via llama.cpp/Ollama).".to_string()),
        _ => {}
    }

    if tools.iter().any(|t| t.name == "docker" && t.installed) {
        recommendations.push("Docker detected. You can run Xavier in a container for better isolation.".to_string());
    }

    OnboardingSuggestions {
        os,
        tools,
        workspace,
        recommendations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;

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
}
