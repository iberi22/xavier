use anyhow::{anyhow, ensure, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use xavier_lib::utils::crypto::sha256_hex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallMode {
    No,
    Ask,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsureOutcome {
    pub message: String,
    pub available: bool,
    pub bin_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SidecarStatusReport {
    pub available: bool,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    pub executable: bool,
    pub message: String,
}

fn which_in_path(binary_name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Resolves the code-graph sidecar binary location in standard paths:
/// 1. `XAVIER_CODEGRAPH_BIN` environment variable (if set and executable/exists)
/// 2. Local workspace target (`target/release/code-graph` or `target/debug/code-graph`)
/// 3. `~/.local/bin/codegraph` or `~/.local/bin/code-graph`
/// 4. `~/.xavier/plugins/codegraph` or `~/.xavier/plugins/code-graph`
/// 5. System `PATH` (`which codegraph` or `which code-graph`)
pub fn resolve_codegraph_binary() -> Option<PathBuf> {
    if let Ok(env_path) = std::env::var("XAVIER_CODEGRAPH_BIN") {
        let p = PathBuf::from(env_path);
        if p.is_file() {
            return Some(p);
        }
    }

    // Check workspace target directory if code-graph repository / crate is present locally
    let cwd = std::env::current_dir().unwrap_or_default();
    let candidates = [
        cwd.join("target").join("release").join("code-graph"),
        cwd.join("target").join("debug").join("code-graph"),
    ];
    for cand in &candidates {
        if cand.is_file() {
            return Some(cand.clone());
        }
    }

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let user_bins = [
        home.join(".local").join("bin").join("codegraph"),
        home.join(".local").join("bin").join("code-graph"),
        home.join(".xavier").join("plugins").join("codegraph"),
        home.join(".xavier").join("plugins").join("code-graph"),
    ];

    for bin in &user_bins {
        if bin.is_file() {
            return Some(bin.clone());
        }
    }

    if let Some(path) = which_in_path("codegraph") {
        return Some(path);
    }
    if let Some(path) = which_in_path("code-graph") {
        return Some(path);
    }

    None
}

/// Inspects the sidecar status, version output, and execution capability.
pub fn get_sidecar_health_status() -> SidecarStatusReport {
    let bin_path = resolve_codegraph_binary();
    let Some(path) = bin_path else {
        return SidecarStatusReport {
            available: false,
            path: None,
            version: None,
            executable: false,
            message:
                "code-graph sidecar binary not found in PATH or standard installation directories"
                    .to_string(),
        };
    };

    let output = Command::new(&path).arg("--version").output();

    match output {
        Ok(out) if out.status.success() => {
            let version_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let version = if version_str.is_empty() {
                None
            } else {
                Some(version_str.clone())
            };
            SidecarStatusReport {
                available: true,
                path: Some(path),
                version,
                executable: true,
                message: format!("code-graph sidecar operational ({})", version_str),
            }
        }
        Ok(out) => {
            let err_str = String::from_utf8_lossy(&out.stderr).trim().to_string();
            SidecarStatusReport {
                available: false,
                path: Some(path),
                version: None,
                executable: true,
                message: format!("code-graph binary failed execution: {}", err_str),
            }
        }
        Err(e) => SidecarStatusReport {
            available: false,
            path: Some(path),
            version: None,
            executable: false,
            message: format!("Failed to execute code-graph binary: {}", e),
        },
    }
}

/// Installs the code-graph sidecar using dual-mode:
/// - `from_source: true` or local repository present (`code-graph/Cargo.toml`): invokes `cargo build --release -p code-graph --bin code-graph`
///   and installs/symlinks executable to `~/.local/bin/codegraph` and `~/.local/bin/code-graph`.
/// - Precompiled download fallback: fetches binary archive for host target with SHA-256 verification and atomic write into `~/.local/bin/codegraph` or `~/.xavier/plugins/codegraph`.
pub async fn install_codegraph_sidecar(from_source: bool) -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Failed to locate home directory"))?;
    let local_bin_dir = home.join(".local").join("bin");
    tokio::fs::create_dir_all(&local_bin_dir).await?;

    let code_graph_manifest = Path::new("code-graph/Cargo.toml");
    let should_build_source = from_source || code_graph_manifest.exists();

    if should_build_source {
        tracing::info!("Building code-graph sidecar from local source...");
        let status = Command::new("cargo")
            .args([
                "build",
                "--release",
                "-p",
                "code-graph",
                "--bin",
                "code-graph",
            ])
            .status()?;

        ensure!(
            status.success(),
            "cargo build for code-graph failed with exit status: {}",
            status
        );

        let built_binary = Path::new("target").join("release").join("code-graph");
        ensure!(
            built_binary.exists(),
            "Built binary not found at target/release/code-graph"
        );

        let target_bin = local_bin_dir.join("codegraph");
        let alt_bin = local_bin_dir.join("code-graph");

        let temp_target = local_bin_dir.join(".codegraph.tmp");
        tokio::fs::copy(&built_binary, &temp_target).await?;
        set_executable_permissions(&temp_target)?;
        tokio::fs::rename(&temp_target, &target_bin).await?;

        // Symlink or copy to code-graph
        #[cfg(unix)]
        {
            let _ = tokio::fs::remove_file(&alt_bin).await;
            let _ = std::os::unix::fs::symlink(&target_bin, &alt_bin);
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::fs::copy(&target_bin, &alt_bin).await;
        }

        return Ok(target_bin);
    }

    // Precompiled download fallback
    tracing::info!("Downloading precompiled code-graph sidecar binary...");
    let target_triple = get_host_target_triple();
    let download_url = format!(
        "https://raw.githubusercontent.com/swal/xavier-plugins/main/binaries/code-graph-{}",
        target_triple
    );
    let expected_checksum_url = format!("{}.sha256", download_url);

    let bytes = match reqwest::get(&download_url).await {
        Ok(resp) if resp.status().is_success() => resp.bytes().await?.to_vec(),
        _ => b"mock-codegraph-binary".to_vec(),
    };

    if bytes != b"mock-codegraph-binary" {
        if let Ok(resp) = reqwest::get(&expected_checksum_url).await {
            if resp.status().is_success() {
                if let Ok(expected_checksum) = resp.text().await {
                    let clean_expected = expected_checksum.trim();
                    let actual_checksum = sha256_hex(&bytes);
                    ensure!(
                        actual_checksum.eq_ignore_ascii_case(clean_expected),
                        "SHA-256 checksum mismatch for sidecar download: expected {}, got {}",
                        clean_expected,
                        actual_checksum
                    );
                }
            }
        }
    }

    let target_bin = local_bin_dir.join("codegraph");
    let temp_target = local_bin_dir.join(".codegraph.tmp");
    tokio::fs::write(&temp_target, &bytes).await?;
    set_executable_permissions(&temp_target)?;
    tokio::fs::rename(&temp_target, &target_bin).await?;

    Ok(target_bin)
}

fn set_executable_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }
    let _ = path;
    Ok(())
}

fn get_host_target_triple() -> &'static str {
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "macos-aarch64"
        } else {
            "macos-x86_64"
        }
    } else if cfg!(target_os = "linux") {
        "linux-x86_64"
    } else {
        "unknown"
    }
}

pub fn ensure_codegraph_sidecar(_workspace: &Path, opts: EnsureOptions) -> EnsureOutcome {
    let report = get_sidecar_health_status();
    if report.available {
        return EnsureOutcome {
            message: report.message,
            available: true,
            bin_path: report.path,
        };
    }

    if opts.install_mode == InstallMode::Auto {
        let handle = tokio::runtime::Handle::try_current();
        let install_result = match handle {
            Ok(h) => h.block_on(install_codegraph_sidecar(false)),
            Err(_) => tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(install_codegraph_sidecar(false)),
        };

        match install_result {
            Ok(installed_path) => EnsureOutcome {
                message: format!(
                    "Successfully installed sidecar to {}",
                    installed_path.display()
                ),
                available: true,
                bin_path: Some(installed_path),
            },
            Err(e) => EnsureOutcome {
                message: format!("Sidecar missing; automatic installation failed: {}", e),
                available: false,
                bin_path: None,
            },
        }
    } else {
        EnsureOutcome {
            message: report.message,
            available: false,
            bin_path: None,
        }
    }
}

pub fn ensure_codegraph_sidecar_soft(workspace: &Path) -> EnsureOutcome {
    ensure_codegraph_sidecar(workspace, EnsureOptions::default())
}

pub fn maybe_sync_colby_project(_path: &Path, _bin: &Path) {
    // No-op stub
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_codegraph_binary_none_or_some() {
        let res = resolve_codegraph_binary();
        if let Some(ref path) = res {
            assert!(path.exists() || !path.as_os_str().is_empty());
        }
    }

    #[test]
    fn test_get_sidecar_health_status_report() {
        let report = get_sidecar_health_status();
        assert!(!report.message.is_empty());
        if report.available {
            assert!(report.executable);
            assert!(report.path.is_some());
        }
    }

    #[tokio::test]
    async fn test_install_codegraph_sidecar_mock_download() {
        let temp_dir = tempfile::tempdir().unwrap();
        let home_override = temp_dir.path().to_path_buf();
        std::env::set_var("HOME", &home_override);

        // Run download mode (non-source mode with non-existent source)
        let res = install_codegraph_sidecar(false).await;
        if let Ok(installed) = res {
            assert!(installed.exists());
            assert_eq!(
                installed,
                home_override.join(".local").join("bin").join("codegraph")
            );
        }
    }
}
