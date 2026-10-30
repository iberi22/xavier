use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

xavier::isolate_test_process!();

/// Runs the real `xavier` binary fully isolated: temp HOME/config/state/data,
/// explicit memory DB path and a private TMPDIR so "temp project" detection is
/// deterministic.
fn run_projects(home: &Path, db_path: &Path, tmp_root: &Path, extra: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.env("HOME", home)
        .env("XAVIER_CONFIG_PATH", home.join("config.toml"))
        .env("XAVIER_RUNTIME_STATE_PATH", home.join("runtime-state.json"))
        .env("XAVIER_DATA_DIR", home.join("data"))
        .env("XAVIER_MEMORY_VEC_PATH", db_path)
        .env("TMPDIR", tmp_root)
        .env_remove("XAVIER_TOKEN")
        .arg("projects")
        .arg("list")
        .args(extra);
    cmd.output().expect("run xavier projects list")
}

#[test]
fn test_projects_list_cli() {
    let home = TempDir::new().unwrap();
    std::fs::create_dir_all(home.path().join("data")).unwrap();
    let tmp_root = TempDir::new().unwrap();
    let temp_proj = tmp_root.path().join("scratch-proj");
    std::fs::create_dir_all(&temp_proj).unwrap();
    let temp_proj_path = temp_proj.to_str().unwrap().to_string();

    // Minimal memory DB with three workspaces, one living under TMPDIR.
    let db_path = home.path().join("data").join("xavier_memory_vec.db");
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute("CREATE TABLE memory_records (workspace_id TEXT)", [])
            .unwrap();
        for ws in [
            "/home/user/my_proj_1",
            "/home/user/my_proj_2",
            temp_proj_path.as_str(),
        ] {
            conn.execute(
                "INSERT INTO memory_records (workspace_id) VALUES (?1)",
                [ws],
            )
            .unwrap();
        }
    }

    // Default: temp project hidden and counted.
    let out = run_projects(home.path(), &db_path, tmp_root.path(), &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("/home/user/my_proj_1"), "stdout: {stdout}");
    assert!(stdout.contains("/home/user/my_proj_2"), "stdout: {stdout}");
    assert!(
        stdout.contains("(1 hidden temp projects)"),
        "stdout: {stdout}"
    );
    assert!(!stdout.contains(&temp_proj_path), "stdout: {stdout}");

    // --all: everything listed.
    let out = run_projects(home.path(), &db_path, tmp_root.path(), &["--all"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    assert!(stdout.contains("/home/user/my_proj_1"), "stdout: {stdout}");
    assert!(stdout.contains("/home/user/my_proj_2"), "stdout: {stdout}");
    assert!(stdout.contains(&temp_proj_path), "stdout: {stdout}");

    // --json: machine-readable, temp project filtered.
    let out = run_projects(home.path(), &db_path, tmp_root.path(), &["--json"]);
    assert!(out.status.success());
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("projects list --json emits valid JSON");
    let arr = json["projects"].as_array().unwrap();
    assert_eq!(arr.len(), 2, "json: {json}");
    assert_eq!(json["filtered"].as_u64(), Some(1), "json: {json}");
}
