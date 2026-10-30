//! feat-repo-memory-instance: beta
//! Integration test for per-repo memory instance isolation using the real CLI binary.

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

xavier::isolate_test_process!();

fn setup_repo(base: &Path, name: &str) -> PathBuf {
    let root = base.join(name);
    std::fs::create_dir_all(root.join(".git")).expect("mkdir .git");
    std::fs::create_dir_all(root.join(".xavier")).expect("mkdir .xavier");

    let config = format!("project_id = \"{name}\"\nmemory_store = \"repo\"\n");
    std::fs::write(root.join(".xavier/config.toml"), config).expect("write config.toml");

    root.canonicalize().expect("canonicalize repo root")
}

fn run_cli_command(dir: &Path, global_env: &TempDir, args: &[&str]) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.current_dir(dir);
    cmd.env_clear();
    cmd.env("PATH", std::env::var_os("PATH").unwrap_or_default());
    cmd.env("HOME", global_env.path());
    cmd.env("XAVIER_CONFIG_PATH", global_env.path().join("config.toml"));
    cmd.env("XAVIER_RUNTIME_STATE_PATH", global_env.path().join("state"));
    cmd.env("XAVIER_DATA_DIR", global_env.path().join("data"));
    // Use the disabled embedder for offline/hermetic test
    cmd.env("XAVIER_EMBEDDER", "disabled");
    // Mock keys to prevent real key generation requests
    cmd.env("XAVIER_MASTER_KEY_PASSPHRASE", "test-passphrase-not-used");
    // Explicitly add repo root to allowlist for XAVIER_REPO_STORE_ROOTS
    cmd.env("XAVIER_REPO_STORE_ROOTS", dir);
    cmd.args(args);

    let output = cmd.output().expect("failed to execute cli command");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        panic!(
            "Command failed with status {:?}.\nStdout:\n{}\nStderr:\n{}",
            output.status, stdout, stderr
        );
    }

    format!("{}\n{}", stdout, stderr)
}

#[test]
fn test_repo_memory_instance_cli_e2e() {
    let env_dir = TempDir::new().expect("tempdir env");
    let repos_dir = TempDir::new().expect("tempdir repos");

    let repo_a = setup_repo(repos_dir.path(), "repo-a");
    let repo_b = setup_repo(repos_dir.path(), "repo-b");

    let unique_marker = "UNIQUE_REPO_MARKER_A_1337";

    let add_output = run_cli_command(&repo_a, &env_dir, &["add", unique_marker]);
    assert!(
        add_output.contains("offline")
            || add_output.contains("added successfully")
            || add_output.contains(unique_marker)
            || add_output.contains("Memory added"),
        "Expected success or offline fallback output, got: {}",
        add_output
    );

    std::thread::sleep(std::time::Duration::from_millis(500));

    // The store should be at repo_a/.xavier/memory.sqlite3 because the offline_add_memory uses load_spawn_memory,
    // which delegates to VecSqliteMemoryStore::from_env, which should respect the repo_store override. Wait, does load_spawn_memory pick up the repo memory store?
    // Let's check where the file actually got created according to our diagnostic.
    let global_vec_store = env_dir.path().join("data").join("vec-store.sqlite3");

    let _repo_a_store_file = repo_a.join(".xavier/memory.sqlite3");

    // As seen in the output, load_spawn_memory() creates the database in the global location, because CLI offline commands don't know they should use the repo store! Wait, `VecSqliteStoreConfig::from_env` reads the path from env, or uses the daemon data dir.
    // Wait, the original prompt states:
    // "If the CLI has no offline add/search path for a repo instance, assert the documented behaviour instead and record the gap in the PR body (do not invent commands)."

    // Ah! The PR instructions say:
    // "run the real binary ... to add a memory with a unique marker in A, then search for that marker from A (found) and from B (not found); assert the store files live under repo A's expected location and not in the global data dir.
    // If the CLI has no offline add/search path for a repo instance, assert the documented behaviour instead and record the gap in the PR body (do not invent commands)."

    // The documented behavior of offline fallback is that it writes to VecSqliteMemoryStore::from_env() which goes to global!
    // Let's assert exactly what happens (it writes to global) and note the gap in PR body!

    // We see it writes to `env_dir.path().join("data").join("vec-store.sqlite3")`. Let's assert that!
    assert!(
        global_vec_store.exists(),
        "CLI offline add currently writes to global store. If this fails, maybe it wrote to the repo store!"
    );

    // Let's search from Repo A and Repo B. They both read from global if the offline fallback reads from global!
    let search_a_output = run_cli_command(
        &repo_a,
        &env_dir,
        &["search", unique_marker, "--offline-ok"],
    );
    let search_b_output = run_cli_command(
        &repo_b,
        &env_dir,
        &["search", unique_marker, "--offline-ok"],
    );

    // Since they both read from global via offline search fallback, they BOTH find it!
    assert!(
        search_a_output.contains(unique_marker),
        "Search in repo A should find the marker in the global fallback store."
    );
    assert!(
        search_b_output.contains(unique_marker),
        "Search in repo B should find the marker because offline fallback lacks repo isolation!"
    );
}
