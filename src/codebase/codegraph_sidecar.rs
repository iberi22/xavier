use anyhow::{anyhow, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvalidSidecarCandidate {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SidecarResolution {
    pub bin_path: Option<PathBuf>,
    pub invalid_candidates: Vec<InvalidSidecarCandidate>,
}

fn validate_binary(path: &Path) -> Result<()> {
    let metadata = std::fs::metadata(path)?;
    ensure!(metadata.is_file(), "not a regular file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(metadata.permissions().mode() & 0o111 != 0, "not executable");
    }
    #[cfg(target_os = "linux")]
    {
        let mut magic = [0; 4];
        std::fs::File::open(path)?
            .read_exact(&mut magic)
            .context("not an ELF executable")?;
        ensure!(magic == *b"\x7fELF", "not an ELF executable");
    }
    Ok(())
}

fn sidecar_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("XAVIER_CODEGRAPH_BIN") {
        candidates.push(PathBuf::from(path));
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let tmp = PathBuf::from("/tmp/xavier-target");
            if tmp.exists() {
                tmp
            } else {
                cwd.join("target")
            }
        });
    for root in [target_dir, cwd.join("target")] {
        for profile in ["release", "debug"] {
            candidates.push(root.join(profile).join("code-graph"));
        }
    }
    if let Some(home) = dirs::home_dir() {
        for dir in [home.join(".local/bin"), home.join(".xavier/plugins")] {
            for name in ["codegraph", "code-graph"] {
                candidates.push(dir.join(name));
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        // Preserve the existing preference for codegraph over code-graph on PATH.
        for name in ["codegraph", "code-graph"] {
            candidates.extend(std::env::split_paths(&path).map(|dir| dir.join(name)));
        }
    }
    candidates
}

fn warn_invalid_candidate(candidate: &InvalidSidecarCandidate) {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static WARNED: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    let mut warned = WARNED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let first_warning = warned.insert(candidate.path.clone());
    drop(warned);
    if first_warning {
        tracing::warn!(path = %candidate.path.display(), reason = %candidate.reason,
            "Skipping invalid code-graph sidecar candidate");
    }
}

/// Inspects every candidate without executing it, reporting invalid files once per path.
pub fn inspect_codegraph_candidates() -> SidecarResolution {
    let mut report = SidecarResolution {
        bin_path: None,
        invalid_candidates: Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();
    for path in sidecar_candidates() {
        let path = std::path::absolute(&path).unwrap_or(path);
        if !seen.insert(path.clone()) {
            continue;
        }
        if std::fs::symlink_metadata(&path)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            continue;
        }
        match validate_binary(&path) {
            Ok(()) => {
                if report.bin_path.is_none() {
                    report.bin_path = Some(path);
                }
            }
            Err(error) => {
                let candidate = InvalidSidecarCandidate {
                    path,
                    reason: error.to_string(),
                };
                warn_invalid_candidate(&candidate);
                report.invalid_candidates.push(candidate);
            }
        }
    }
    report
}

/// Resolves the first regular executable sidecar, requiring ELF magic on Linux.
pub fn resolve_codegraph_binary() -> Option<PathBuf> {
    inspect_codegraph_candidates().bin_path
}

const VERSION_TIMEOUT: Duration = Duration::from_secs(3);

async fn probe_version(path: &Path, timeout: Duration) -> Result<String> {
    let mut command = tokio::process::Command::new(path);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .context("code-graph --version timed out")??;
    ensure!(output.status.success(), "code-graph --version failed");
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string())
}

fn probe_version_sync(path: &Path) -> Result<String> {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?
                    .block_on(probe_version(path, VERSION_TIMEOUT))
            })
            .join()
            .map_err(|_| anyhow!("code-graph version probe panicked"))?
    })
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

    match probe_version_sync(&path) {
        Ok(version) => SidecarStatusReport {
            available: true,
            path: Some(path),
            version: Some(version.clone()),
            executable: true,
            message: format!("code-graph sidecar operational ({version})"),
        },
        Err(error) => SidecarStatusReport {
            available: false,
            path: Some(path),
            version: None,
            executable: true,
            message: format!("code-graph binary failed version check: {error}"),
        },
    }
}

/// Installs the code-graph sidecar using dual-mode:
/// - `from_source: true` or local repository present (`code-graph/Cargo.toml`): invokes `cargo build --release -p code-graph --bin code-graph`
///   and installs/symlinks executable to `~/.local/bin/codegraph` and `~/.local/bin/code-graph`.
/// - Precompiled download fallback: fetches binary archive for host target with SHA-256 verification and atomic write into `~/.local/bin/codegraph` or `~/.xavier/plugins/codegraph`.
pub async fn install_codegraph_sidecar(from_source: bool) -> Result<PathBuf> {
    // 2026-09-15 (issue #2260): guard de reentrada duro.
    //
    // El guard anterior miraba `XAVIER_INSIDE_TEST` (que NADIE setea en el repo)
    // y `CARGO_PKG_NAME == "code-graph"` (que vale `xavier` cuando corre un test
    // o el servidor del crate xavier). Resultado: no aplicaba nunca al vector real
    // (`ensure_codegraph_sidecar` desde `tests/codegraph_installer_e2e.rs` y desde
    // `start_http_server`), y cada invocacion lanzaba un `cargo build --release -p
    // code-graph` completo. En cascada (el sidecar re-resuelve el sidecar) eso produjo
    // una fork bomb de 19 procesos `codegraph` y 59 `cargo` (load 27).
    //
    // Ahora: una variable de entorno HEREDABLE por los hijos marca "ya estamos dentro
    // de una instalacion/construccion del sidecar". Si el hijo la ve, NO vuelve a
    // construir; devuelve el binario existente o falla limpio con mensaje accionable.
    const GUARD_ENV: &str = "XAVIER_CODEGRAPH_INSTALL_ACTIVE";
    let already_installing = std::env::var(GUARD_ENV).is_ok();

    if already_installing {
        if let Some(existing) = resolve_codegraph_binary() {
            probe_version(&existing, VERSION_TIMEOUT).await?;
            return Ok(existing);
        }
        return Err(anyhow!(
            "recursive code-graph sidecar install blocked ({} is set) and no existing \
             binary found; refusing to spawn a nested `cargo build` (issue #2260)",
            GUARD_ENV
        ));
    }

    // Guard contra tests y cascadas de subprocesos: nunca construir desde un test.
    // `cfg!(test)` no aplica a integration tests, por eso ademas comprobamos las
    // banderas de entorno que usa la maquinaria de tests de Rust/Cargo.
    let inside_test = std::env::var("XAVIER_INSIDE_TEST").is_ok()
        || std::env::var("CARGO_PKG_NAME")
            .map(|p| p == "code-graph")
            .unwrap_or(false)
        || std::env::var("NEXTEST").is_ok()
        || std::env::var("CARGO_TARGET_TMPDIR").is_ok();
    if inside_test {
        if let Some(existing) = resolve_codegraph_binary() {
            probe_version(&existing, VERSION_TIMEOUT).await?;
            return Ok(existing);
        }
        return Err(anyhow!(
            "code-graph sidecar missing and install skipped inside a test \
             (issue #2260); pre-install the sidecar or provide it on PATH"
        ));
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow!("Failed to locate home directory"))?;
    let local_bin_dir = home.join(".local").join("bin");

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
            .stdin(Stdio::null())
            .stdout(Stdio::from(std::io::stderr()))
            .env("XAVIER_INSIDE_BUILD", "1")
            // Heredable por los hijos: cualquier proceso lanzado por este build
            // vera la bandera y no volvera a construir (issue #2260).
            .env(GUARD_ENV, "1")
            .status()?;

        ensure!(
            status.success(),
            "cargo build for code-graph failed with exit status: {}",
            status
        );

        let cwd = std::env::current_dir().unwrap_or_default();
        let target_dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let tmp_xavier = PathBuf::from("/tmp/xavier-target");
                if tmp_xavier.exists() {
                    tmp_xavier
                } else {
                    cwd.join("target")
                }
            });

        let built_binary = if target_dir.join("release").join("code-graph").exists() {
            target_dir.join("release").join("code-graph")
        } else if Path::new("target")
            .join("release")
            .join("code-graph")
            .exists()
        {
            Path::new("target").join("release").join("code-graph")
        } else if target_dir.join("debug").join("code-graph").exists() {
            target_dir.join("debug").join("code-graph")
        } else {
            Path::new("target").join("debug").join("code-graph")
        };

        ensure!(
            built_binary.exists(),
            "Built binary not found at target/release/code-graph or {}",
            target_dir.join("release").join("code-graph").display()
        );

        let target_bin = local_bin_dir.join("codegraph");
        let alt_bin = local_bin_dir.join("code-graph");

        validate_binary(&built_binary).context("Invalid built code-graph binary")?;
        let bytes = std::fs::read(&built_binary)?;
        install_bytes(&bytes, None, &target_bin, |path| async move {
            probe_version(&path, VERSION_TIMEOUT).await
        })
        .await?;

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
    let target_bin = local_bin_dir.join("codegraph");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    download_sidecar(
        &download_url,
        &target_bin,
        |url| {
            let client = client.clone();
            async move {
                Ok(client
                    .get(url)
                    .send()
                    .await?
                    .error_for_status()?
                    .bytes()
                    .await?
                    .to_vec())
            }
        },
        |path| async move { probe_version(&path, VERSION_TIMEOUT).await },
    )
    .await
}

async fn download_sidecar<F, Fut, V, VFut>(
    url: &str,
    destination: &Path,
    mut fetch: F,
    version_check: V,
) -> Result<PathBuf>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>>>,
    V: FnOnce(PathBuf) -> VFut,
    VFut: std::future::Future<Output = Result<String>>,
{
    let bytes = fetch(url.to_string())
        .await
        .context("Sidecar download failed")?;
    let checksum = fetch(format!("{url}.sha256"))
        .await
        .context("Sidecar checksum download failed")?;
    install_bytes(&bytes, Some(&checksum), destination, version_check).await
}

async fn install_bytes<V, VFut>(
    bytes: &[u8],
    checksum: Option<&[u8]>,
    destination: &Path,
    version_check: V,
) -> Result<PathBuf>
where
    V: FnOnce(PathBuf) -> VFut,
    VFut: std::future::Future<Output = Result<String>>,
{
    if let Some(checksum) = checksum {
        let text = std::str::from_utf8(checksum).context("Invalid SHA-256 checksum file")?;
        let digest = text.split_whitespace().next().unwrap_or_default();
        ensure!(
            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid SHA-256 checksum file"
        );
        ensure!(
            sha256_hex(bytes).eq_ignore_ascii_case(digest),
            "SHA-256 checksum mismatch"
        );
    }
    let directory = destination
        .parent()
        .ok_or_else(|| anyhow!("Missing sidecar destination directory"))?;
    std::fs::create_dir_all(directory)?;
    let mut staged = tempfile::Builder::new()
        .prefix(".codegraph-")
        .tempfile_in(directory)?;
    staged.write_all(bytes)?;
    set_executable_permissions(staged.path())?;
    staged.as_file().sync_all()?;
    // Close the writable handle before executing the staged binary.
    let staged = staged.into_temp_path();
    validate_binary(&staged).context("Invalid code-graph binary")?;
    version_check(staged.to_path_buf()).await?;
    staged.persist(destination).map_err(|e| e.error)?;
    Ok(destination.to_path_buf())
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarInstallOutcome {
    pub success: bool,
    pub message: String,
    pub bin_path: Option<PathBuf>,
    pub version: String,
    pub verified: bool,
    pub chosen_binary: Option<PathBuf>,
    pub invalid_candidates: Vec<InvalidSidecarCandidate>,
}

pub fn install_codegraph_sidecar_sync(from_source: bool) -> Result<SidecarInstallOutcome> {
    let resolution = inspect_codegraph_candidates();
    let path_res = if let Ok(_handle) = tokio::runtime::Handle::try_current() {
        std::thread::scope(|s| {
            s.spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(install_codegraph_sidecar(from_source))
            })
            .join()
            .unwrap()
        })
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(install_codegraph_sidecar(from_source))
    };

    let path_res =
        path_res.and_then(|path| probe_version_sync(&path).map(|version| (path, version)));
    match path_res {
        Ok((bin_path, version)) => Ok(SidecarInstallOutcome {
            success: true,
            message: format!(
                "CodeGraph sidecar installed successfully at {}",
                bin_path.display()
            ),
            chosen_binary: Some(bin_path.clone()),
            bin_path: Some(bin_path),
            version,
            verified: true,
            invalid_candidates: resolution.invalid_candidates,
        }),
        Err(e) => Ok(SidecarInstallOutcome {
            success: false,
            message: format!("CodeGraph sidecar install failed: {}", e),
            bin_path: None,
            version: env!("CARGO_PKG_VERSION").to_string(),
            verified: false,
            chosen_binary: resolution.bin_path,
            invalid_candidates: resolution.invalid_candidates,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mark this whole lib-test binary as "inside a test" before `main`.
    ///
    /// `install_codegraph_sidecar` already refuses to spawn
    /// `cargo build --release -p code-graph` when `XAVIER_INSIDE_TEST` is set
    /// (issue #2260 cascade guard), but nothing in the repo ever sets it, so any
    /// unit test that reaches the install path release-builds the sidecar from
    /// inside the suite — repo rule: unit tests never run `cargo build`.
    /// Under nextest the `NEXTEST` variable already covers this; this hook
    /// covers plain `cargo test`, where sibling tests matched by the
    /// `codegraph_sidecar` filter (e.g.
    /// `cli::commands::code::tests::test_install_codegraph_sidecar_json_output`)
    /// used to build. Same `.init_array` mechanism as
    /// `crate::isolate_test_process!()`.
    #[cfg(target_os = "linux")]
    #[allow(unsafe_code)]
    const _: () = {
        extern "C" fn __xavier_inside_test_init() {
            std::env::set_var("XAVIER_INSIDE_TEST", "1");
        }
        #[used]
        #[link_section = ".init_array"]
        static __XAVIER_INSIDE_TEST_INIT: extern "C" fn() = __xavier_inside_test_init;
    };

    /// Restores the process-global state a test overrides (env vars + cwd), even
    /// if the test panics, so the override cannot leak into the next test that
    /// shares this process (`--test-threads=1`).
    struct EnvRestore {
        vars: Vec<(&'static str, Option<std::ffi::OsString>)>,
        cwd: PathBuf,
    }

    impl EnvRestore {
        fn new(vars: &[&'static str]) -> Self {
            let saved = vars
                .iter()
                .map(|k| (*k, std::env::var_os(k)))
                .collect::<Vec<_>>();
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            Self { vars: saved, cwd }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (key, value) in &self.vars {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
            let _ = std::env::set_current_dir(&self.cwd);
        }
    }

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
        let destination = temp_dir.path().join("codegraph");
        let bytes = b"\x7fELFfixture";
        let checksum = "26587dd4da66ca8d8a22b44abbea037a5e0a191ff97ff2a53adea144cbb51fcc";
        let installed = download_sidecar(
            "stub://sidecar",
            &destination,
            |url| async move {
                Ok(if url.ends_with(".sha256") {
                    checksum.as_bytes().to_vec()
                } else {
                    bytes.to_vec()
                })
            },
            |_| async { Ok("codegraph fixture".to_string()) },
        )
        .await
        .unwrap();
        assert_eq!(installed, destination);
        assert_eq!(std::fs::read(installed).unwrap(), bytes);
    }

    /// Regresión issue #2260: el guard de reentrada debe abortar la instalación
    /// anidada en vez de lanzar otro `cargo build --release -p code-graph`.
    ///
    /// Sin el fix, esta llamada (con la bandera puesta) intentaría construir de
    /// nuevo, que es exactamente lo que produjo la fork bomb (19 codegraph / 59 cargo).
    #[tokio::test]
    async fn test_recursive_install_is_blocked_by_guard() {
        // Bandera heredable puesta => el hijo nunca debe construir.
        std::env::set_var("XAVIER_CODEGRAPH_INSTALL_ACTIVE", "1");
        let res = install_codegraph_sidecar(true).await;
        std::env::remove_var("XAVIER_CODEGRAPH_INSTALL_ACTIVE");

        match res {
            // Si ya hay un binario resoluble, el guard lo devuelve sin construir.
            Ok(path) => assert!(!path.as_os_str().is_empty()),
            // Si no lo hay, debe fallar limpio con mensaje accionable, NO construir.
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("recursive") || msg.contains("refusing"),
                    "el guard debe explicar el bloqueo de recursión, mensaje: {}",
                    msg
                );
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "codegraph_sidecar_tests.rs"]
mod hardening_tests;
