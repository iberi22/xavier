//! core_smoke_cli_e2e.rs
//! Ledger ID: feat-unified-storage

use std::process::Command;

xavier::isolate_test_process!();

fn run_cli_cmd(temp: &tempfile::TempDir, args: &[&str]) -> std::process::Output {
    let home = temp.path().join("home");
    // Ensure standard Xavier paths
    let config_dir = temp.path().join("config");
    let state = temp.path().join("state");
    let data = temp.path().join("data");

    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    std::fs::create_dir_all(&data).unwrap();

    // Explicitly create an empty configuration file because Xavier cli expects it to be readable if specified
    std::fs::write(config_dir.join("xavier.config.json"), "{}").unwrap();
    // Explicitly create an empty runtime state file because Xavier cli expects it to be readable if specified
    std::fs::write(state.join("state"), "{}").unwrap();
    std::fs::write(temp.path().join(".env"), "").unwrap();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));

    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("XAVIER_CONFIG_PATH", config_dir.join("xavier.config.json"))
        .env("XAVIER_RUNTIME_STATE_PATH", state.join("state"))
        .env("XAVIER_DATA_DIR", &data)
        // Disable network connectivity and daemon usage
        .env("XAVIER_HOST", "127.0.0.1")
        .env("XAVIER_PORT", "0")
        // Enforce local dummy embedding for deterministic fast tests
        .env("XAVIER_EMBEDDING_PROVIDER_MODE", "disabled")
        .args(args);

    cmd.output().expect("failed to execute xavier CLI")
}

#[test]
fn core_smoke_cli_add_then_search() {
    let temp = tempfile::tempdir().unwrap();

    // 1. Offline add
    let add_output = run_cli_cmd(&temp, &["add", "unique_smoke_test_content_12345"]);
    let add_stdout = String::from_utf8_lossy(&add_output.stdout);
    let add_stderr = String::from_utf8_lossy(&add_output.stderr);
    println!("Add stdout:\n{}", add_stdout);
    println!("Add stderr:\n{}", add_stderr);

    // 2. Offline search
    let search_output = run_cli_cmd(
        &temp,
        &["search", "unique_smoke_test_content_12345", "--offline-ok"],
    );
    let search_stdout = String::from_utf8_lossy(&search_output.stdout);
    let search_stderr = String::from_utf8_lossy(&search_output.stderr);
    println!("Search stdout:\n{}", search_stdout);
    println!("Search stderr:\n{}", search_stderr);

    let combined = format!("{}\n{}", search_stdout, search_stderr);
    assert!(
        combined.contains("unique_smoke_test_content_12345"),
        "Added text should be found in offline search results"
    );
}

#[test]
fn core_smoke_cli_restart_persistence() {
    let temp = tempfile::tempdir().unwrap();

    // 1. Process A: add memory
    let _add_output = run_cli_cmd(&temp, &["add", "persistence_smoke_test_content_67890"]);

    // 2. Process B: search memory (completely new process, same data dir)
    let search_output = run_cli_cmd(
        &temp,
        &[
            "search",
            "persistence_smoke_test_content_67890",
            "--offline-ok",
        ],
    );

    let search_stdout = String::from_utf8_lossy(&search_output.stdout);
    let search_stderr = String::from_utf8_lossy(&search_output.stderr);

    let combined = format!("{}\n{}", search_stdout, search_stderr);
    assert!(
        combined.contains("persistence_smoke_test_content_67890"),
        "Memory should persist across CLI invocations"
    );
}
