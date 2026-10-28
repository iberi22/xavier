use super::*;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;

struct Sandbox {
    root: tempfile::TempDir,
    saved: Vec<(&'static str, Option<OsString>)>,
    cwd: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let keys = [
            "HOME",
            "PATH",
            "CARGO_TARGET_DIR",
            "XAVIER_CODEGRAPH_BIN",
            "XAVIER_CONFIG_PATH",
            "XAVIER_RUNTIME_STATE_PATH",
            "XAVIER_DATA_DIR",
            "XAVIER_INSIDE_TEST",
            "XAVIER_CODEGRAPH_INSTALL_ACTIVE",
            "NEXTEST",
            "CARGO_TARGET_TMPDIR",
            "HTTPS_PROXY",
            "https_proxy",
            "NO_PROXY",
            "no_proxy",
        ];
        let saved = keys
            .iter()
            .map(|&key| (key, std::env::var_os(key)))
            .collect();
        let cwd = std::env::current_dir().unwrap();
        for key in keys {
            std::env::remove_var(key);
        }
        for (key, path) in [
            ("HOME", "home"),
            ("PATH", "bin"),
            ("CARGO_TARGET_DIR", "target"),
            ("XAVIER_CONFIG_PATH", "config.json"),
            ("XAVIER_RUNTIME_STATE_PATH", "runtime.json"),
            ("XAVIER_DATA_DIR", "data"),
        ] {
            std::env::set_var(key, root.path().join(path));
        }
        std::fs::create_dir_all(root.path().join("home")).unwrap();
        std::env::set_current_dir(root.path()).unwrap();
        Self { root, saved, cwd }
    }

    fn candidate(&self, name: &str, bytes: &[u8], mode: u32) -> PathBuf {
        let path = self.root.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.cwd).unwrap();
        for (key, value) in &self.saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[test]
#[serial_test::serial]
fn resolver_skips_executable_text_candidate() {
    let sandbox = Sandbox::new();
    let text = sandbox.candidate("text", b"placeholder", 0o755);
    let valid = sandbox.candidate("home/.local/bin/codegraph", b"\x7fELFfixture", 0o755);
    std::env::set_var("XAVIER_CODEGRAPH_BIN", text);
    assert_eq!(resolve_codegraph_binary(), Some(valid));
}

#[test]
#[serial_test::serial]
fn resolver_skips_non_executable_elf_candidate() {
    let sandbox = Sandbox::new();
    let invalid = sandbox.candidate("not-executable", b"\x7fELFfixture", 0o644);
    let valid = sandbox.candidate("bin/code-graph", b"\x7fELFfixture", 0o755);
    std::env::set_var("XAVIER_CODEGRAPH_BIN", invalid);
    assert_eq!(resolve_codegraph_binary(), Some(valid));
}

#[test]
#[serial_test::serial]
fn resolver_accepts_executable_elf_candidate() {
    let sandbox = Sandbox::new();
    let valid = sandbox.candidate("valid", b"\x7fELFfixture", 0o755);
    std::env::set_var("XAVIER_CODEGRAPH_BIN", &valid);
    assert_eq!(resolve_codegraph_binary(), Some(valid));
}

#[tokio::test]
#[serial_test::serial]
async fn failed_download_leaves_no_binary() {
    let sandbox = Sandbox::new();
    // A closed local proxy forces a transport failure without contacting the network.
    std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:9");
    std::env::set_var("https_proxy", "http://127.0.0.1:9");
    let result = install_codegraph_sidecar(false).await;
    assert!(
        result.is_err(),
        "failed download must not install fallback bytes"
    );
    let bin_dir = sandbox.root.path().join("home/.local/bin");
    assert!(!bin_dir.join("codegraph").exists());
    if bin_dir.exists() {
        assert_eq!(std::fs::read_dir(bin_dir).unwrap().count(), 0);
    }
}

async fn stub_version(_: PathBuf) -> Result<String> {
    Ok("codegraph fixture".to_string())
}

#[tokio::test]
async fn checksum_missing_aborts_without_writing() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("codegraph");
    let result = download_sidecar(
        "stub://sidecar",
        &destination,
        |url| async move {
            if url.ends_with(".sha256") {
                anyhow::bail!("checksum unavailable");
            }
            Ok(b"\x7fELFfixture".to_vec())
        },
        stub_version,
    )
    .await;
    assert!(result.unwrap_err().to_string().contains("checksum"));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn checksum_mismatch_preserves_existing_binary() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("codegraph");
    std::fs::write(&destination, b"previous installation").unwrap();
    let result = download_sidecar(
        "stub://sidecar",
        &destination,
        |url| async move {
            Ok(if url.ends_with(".sha256") {
                vec![b'0'; 64]
            } else {
                b"\x7fELFfixture".to_vec()
            })
        },
        stub_version,
    )
    .await;
    assert!(result.unwrap_err().to_string().contains("mismatch"));
    assert_eq!(
        std::fs::read(destination).unwrap(),
        b"previous installation"
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn malformed_checksums_abort_without_writing() {
    for checksum in [b"".as_slice(), b"not-a-hash", &[b'z'; 64]] {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("codegraph");
        let result = install_bytes(
            b"\x7fELFfixture",
            Some(checksum),
            &destination,
            stub_version,
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("checksum"));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn verified_download_is_staged_and_installed_with_executable_mode() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("codegraph");
    let bytes = b"\x7fELFfixture";
    // Independently generated SHA-256 fixture for the eleven-byte ELF fixture.
    let digest = "26587dd4da66ca8d8a22b44abbea037a5e0a191ff97ff2a53adea144cbb51fcc";
    let mut requests = Vec::new();
    let result = download_sidecar(
        "stub://sidecar",
        &destination,
        |url| {
            requests.push(url.clone());
            async move {
                Ok(if url.ends_with(".sha256") {
                    format!("{digest}  codegraph\n").into_bytes()
                } else {
                    bytes.to_vec()
                })
            }
        },
        |staged| {
            assert_eq!(staged.parent(), Some(root.path()));
            assert_ne!(staged, destination);
            assert!(!destination.exists());
            assert_eq!(std::fs::read(&staged).unwrap(), bytes);
            stub_version(staged)
        },
    )
    .await
    .unwrap();
    assert_eq!(requests, ["stub://sidecar", "stub://sidecar.sha256"]);
    assert_eq!(result, destination);
    assert_eq!(std::fs::read(&result).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(result).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn version_failure_removes_staging_file_and_preserves_existing_binary() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("codegraph");
    std::fs::write(&destination, b"previous installation").unwrap();
    let result = install_bytes(b"\x7fELFfixture", None, &destination, |_| async {
        anyhow::bail!("version failed")
    })
    .await;
    assert!(result.is_err());
    assert_eq!(
        std::fs::read(destination).unwrap(),
        b"previous installation"
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn non_elf_download_is_rejected_before_execution() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("codegraph");
    let result = install_bytes(b"text file", None, &destination, |_| async {
        panic!("invalid binary must not be executed");
        #[allow(unreachable_code)]
        Ok(String::new())
    })
    .await;
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid code-graph binary"));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
#[serial_test::serial]
fn invalid_candidates_are_reported_and_doctor_warns() {
    let sandbox = Sandbox::new();
    let invalid = sandbox.candidate("bin/codegraph", b"placeholder", 0o755);
    let valid = sandbox.candidate("bin/code-graph", b"\x7fELFfixture", 0o755);
    std::env::set_var("XAVIER_CODEGRAPH_BIN", &invalid);
    let report = inspect_codegraph_candidates();
    assert_eq!(report.bin_path, Some(valid));
    assert_eq!(
        report.invalid_candidates.len(),
        1,
        "deduplicate the override and PATH"
    );
    assert_eq!(report.invalid_candidates[0].path, invalid);
    assert!(report.invalid_candidates[0].reason.contains("ELF"));
    let checks = crate::cli::handlers::doctor::check_codegraph_sidecar();
    assert_eq!(
        checks[0].status,
        crate::cli::handlers::doctor::CheckStatus::Warn
    );
    assert!(checks[0].hint.as_ref().unwrap().contains("Move"));
    assert_eq!(std::fs::read(invalid).unwrap(), b"placeholder");
}

#[tokio::test]
#[serial_test::serial]
async fn failed_install_command_returns_an_error() {
    let _sandbox = Sandbox::new();
    std::env::set_var("XAVIER_CODEGRAPH_INSTALL_ACTIVE", "1");
    let outcome = install_codegraph_sidecar_sync(false).unwrap();
    assert!(!outcome.success);
    assert!(!outcome.verified);
    let result = crate::cli::commands::code::handle_code_command(
        crate::cli::commands::enums::CodeCommand::Install {
            from_source: false,
            force: false,
            json: true,
        },
    )
    .await;
    assert!(
        result.is_err(),
        "CLI operation failure must propagate to the process exit code"
    );
}

#[test]
#[serial_test::serial]
fn resolver_skips_non_regular_file() {
    let sandbox = Sandbox::new();
    let directory = sandbox.root.path().join("directory");
    std::fs::create_dir(&directory).unwrap();
    std::env::set_var("XAVIER_CODEGRAPH_BIN", &directory);
    let report = inspect_codegraph_candidates();
    assert!(report.bin_path.is_none());
    assert_eq!(report.invalid_candidates[0].path, directory);
    assert!(report.invalid_candidates[0].reason.contains("regular file"));
}

#[tokio::test]
async fn version_probe_requires_success_and_has_a_deadline() {
    let root = tempfile::tempdir().unwrap();
    // These scripts stub only the version subprocess; they are never install/resolution candidates.
    let successful = root.path().join("successful-version");
    let failed = root.path().join("failed-version");
    let hanging = root.path().join("hanging-version");
    for (path, script) in [
        (&successful, "#!/bin/sh\n[ \"$#\" = 1 ] && [ \"$1\" = --version ] || exit 2\nprintf 'codegraph fixture\\n'\n"),
        (&failed, "#!/bin/sh\nexit 1\n"),
        (&hanging, "#!/bin/sh\nwhile :; do :; done\n"),
    ] {
        std::fs::write(path, script).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(
        probe_version(&successful, Duration::from_secs(1))
            .await
            .unwrap(),
        "codegraph fixture"
    );
    assert!(probe_version(&failed, Duration::from_secs(1))
        .await
        .unwrap_err()
        .to_string()
        .contains("failed"));
    let start = std::time::Instant::now();
    assert!(probe_version(&hanging, Duration::from_millis(30))
        .await
        .unwrap_err()
        .to_string()
        .contains("timed out"));
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
#[serial_test::serial]
fn repeated_resolution_warns_once_per_invalid_path() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tracing_subscriber::{
        layer::{Context, SubscriberExt},
        Layer,
    };
    struct Warnings(Arc<AtomicUsize>);
    impl<S: tracing::Subscriber> Layer<S> for Warnings {
        fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
            if *event.metadata().level() == tracing::Level::WARN {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    let sandbox = Sandbox::new();
    let invalid = sandbox.candidate("invalid", b"placeholder", 0o755);
    std::env::set_var("XAVIER_CODEGRAPH_BIN", invalid);
    let count = Arc::new(AtomicUsize::new(0));
    let subscriber = tracing_subscriber::registry().with(Warnings(count.clone()));
    tracing::subscriber::with_default(subscriber, || {
        assert!(resolve_codegraph_binary().is_none());
        assert!(resolve_codegraph_binary().is_none());
    });
    assert_eq!(count.load(Ordering::Relaxed), 1);
}
