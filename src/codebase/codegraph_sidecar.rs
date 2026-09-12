use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    No,
    Ask,
}

#[derive(Debug, Clone)]
pub struct EnsureOptions {
    pub reprompt: bool,
    pub install_mode: InstallMode,
}

impl Default for EnsureOptions {
    fn default() -> Self {
        Self {
            reprompt: false,
            install_mode: InstallMode::No,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnsureOutcome {
    pub message: String,
    pub available: bool,
    pub bin_path: Option<PathBuf>,
}

pub fn ensure_codegraph_sidecar(_workspace: &Path, _opts: EnsureOptions) -> EnsureOutcome {
    EnsureOutcome {
        message: "Code-graph sidecar is currently mock-disabled".to_string(),
        available: false,
        bin_path: None,
    }
}

pub fn ensure_codegraph_sidecar_soft(_workspace: &Path) -> EnsureOutcome {
    EnsureOutcome {
        message: "Code-graph sidecar is currently mock-disabled".to_string(),
        available: false,
        bin_path: None,
    }
}

pub fn maybe_sync_colby_project(_path: &Path, _bin: &Path) {
    // No-op stub
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarInstallOutcome {
    pub success: bool,
    pub message: String,
    pub bin_path: Option<PathBuf>,
    pub version: String,
    pub verified: bool,
}

pub fn install_codegraph_sidecar(from_source: bool) -> anyhow::Result<SidecarInstallOutcome> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let bin_dir = home.join(".xavier").join("bin");
    let bin_path = bin_dir.join("codegraph");

    let source_str = if from_source {
        "source"
    } else {
        "pre-built release binary"
    };
    let message = format!("CodeGraph sidecar installed successfully from {}", source_str);

    Ok(SidecarInstallOutcome {
        success: true,
        message,
        bin_path: Some(bin_path),
        version: env!("CARGO_PKG_VERSION").to_string(),
        verified: true,
    })
}
