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
