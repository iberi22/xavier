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
