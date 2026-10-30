//! feat-repo-memory-instance: beta
//! Fallback coverage for the offline CLI path of per-repo memory instances.
//!
//! Documented gap: the offline `xavier add` / `xavier search --offline-ok`
//! fallback ignores `memory_store = "repo"` and always uses the global
//! vec-store under `XAVIER_DATA_DIR`. Two repos therefore see each other's
//! offline memories. This test pins that behaviour (global store written,
//! no repo store created, marker visible from both repos) so a future fix
//! that adds repo isolation to the offline path must update it on purpose.

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

xavier::isolate_test_process!();

fn git_init(root: &Path, home: &Path) {
    let status = Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", home.join("gitconfig-none"))
        .status()
        .expect("spawn git init");
    assert!(status.success(), "git init failed in {}", root.display());
    assert!(
        root.join(".git/HEAD").is_file(),
        "git init did not create .git/HEAD"
    );
}

fn setup_repo(base: &Path, home: &Path, name: &str) -> PathBuf {
    let root = base.join(name);
    std::fs::create_dir_all(&root).expect("mkdir repo");
    git_init(&root, home);
    std::fs::create_dir_all(root.join(".xavier")).expect("mkdir .xavier");
    let config = format!("project_id = \"{name}\"\nmemory_store = \"repo\"\n");
    std::fs::write(root.join(".xavier/config.toml"), config).expect("write config.toml");
    root.canonicalize().expect("canonicalize repo root")
}

fn run_cli(dir: &Path, env_dir: &Path, args: &[&str]) -> (String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .current_dir(dir)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", env_dir)
        .env("XAVIER_CONFIG_PATH", env_dir.join("config.toml"))
        .env("XAVIER_RUNTIME_STATE_PATH", env_dir.join("state"))
        .env("XAVIER_DATA_DIR", env_dir.join("data"))
        .env("XAVIER_EMBEDDER", "disabled")
        .env("XAVIER_MASTER_KEY_PASSPHRASE", "test-passphrase-not-used")
        .env("XAVIER_REPO_STORE_ROOTS", dir)
        // No daemon: force the offline fallback.
        .env("XAVIER_URL", "http://127.0.0.1:0")
        .args(args)
        .output()
        .expect("failed to execute xavier");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "xavier {args:?} failed: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );
    (stdout, stderr)
}

/// Returns true when the first JSON object in `output` has a result whose
/// content contains `marker`.
fn results_contain(output: &str, marker: &str) -> bool {
    let start = output.find('{').expect("output missing JSON object");
    let mut depth = 0usize;
    let mut end = None;
    for (i, c) in output[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let json = &output[start..=end.expect("unterminated JSON object")];
    let data: serde_json::Value =
        serde_json::from_str(json).unwrap_or_else(|e| panic!("invalid JSON ({e}): {json}"));
    data.get("results")
        .and_then(|r| r.as_array())
        .expect("results array")
        .iter()
        .any(|r| {
            r.get("content")
                .and_then(|c| c.as_str())
                .is_some_and(|c| c.contains(marker))
        })
}

fn assert_no_repo_store(repo: &Path, when: &str) {
    let store = repo.join(".xavier/memory.sqlite3");
    assert!(
        !store.exists(),
        "{when}: offline CLI unexpectedly created a repo store at {} (dual write?)",
        store.display()
    );
}

#[test]
fn offline_cli_add_search_uses_global_store_fallback() {
    let env_dir = TempDir::new().expect("tempdir env");
    let repos_dir = TempDir::new().expect("tempdir repos");
    let repo_a = setup_repo(repos_dir.path(), env_dir.path(), "repo-a");
    let repo_b = setup_repo(repos_dir.path(), env_dir.path(), "repo-b");
    let marker = "UNIQUE_REPO_MARKER_A_1337";

    run_cli(&repo_a, env_dir.path(), &["add", marker]);

    let global_store = env_dir.path().join("data").join("vec-store.sqlite3");
    assert!(
        global_store.is_file(),
        "offline add should write the global store at {}",
        global_store.display()
    );
    assert_no_repo_store(&repo_a, "after add in A");
    assert_no_repo_store(&repo_b, "after add in A");

    let (out_a, _) = run_cli(&repo_a, env_dir.path(), &["search", marker, "--offline-ok"]);
    let (out_b, _) = run_cli(&repo_b, env_dir.path(), &["search", marker, "--offline-ok"]);

    assert!(
        results_contain(&out_a, marker),
        "search from A must find the marker: {out_a}"
    );
    assert!(
        results_contain(&out_b, marker),
        "documented gap: offline fallback has no repo isolation, so B also finds A's marker: {out_b}"
    );
    assert_no_repo_store(&repo_a, "after searches");
    assert_no_repo_store(&repo_b, "after searches");
}
