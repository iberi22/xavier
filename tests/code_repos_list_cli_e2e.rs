use code_graph::db::CodeGraphDB;
use std::process::Command;

xavier::isolate_test_process!();

#[test]
fn test_code_repos_list_cli_e2e() {
    let tmp = tempfile::tempdir().unwrap();

    let home = tmp.path().join("home");
    let config = tmp.path().join("config");
    let state = tmp.path().join("state");
    let data = tmp.path().join("data");

    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("config.toml"), "{}").unwrap();
    std::fs::create_dir_all(&state).unwrap();
    std::fs::create_dir_all(&data).unwrap();

    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(workspace.join(".git")).unwrap();

    let extra_root = tmp.path().join("extra_root");
    std::fs::create_dir_all(&extra_root).unwrap();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.current_dir(&workspace)
        .args(["code", "repos", "list", "--json"]);
    cmd.env("HOME", &home);
    cmd.env("XAVIER_CONFIG_PATH", config.join("config.toml"));
    cmd.env("XAVIER_RUNTIME_STATE_PATH", &state);
    cmd.env("XAVIER_DATA_DIR", &data);
    cmd.env_remove("XAVIER_CODE_GRAPH_DB_PATH");
    cmd.env("XAVIER_CODE_EXTRA_ROOTS", &extra_root);
    // we will set envs on cmd

    // Initial state: No DB
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.current_dir(&workspace)
        .args(["code", "repos", "list", "--json"]);
    cmd.env("HOME", &home);
    cmd.env("XAVIER_CONFIG_PATH", config.join("config.toml"));
    cmd.env("XAVIER_RUNTIME_STATE_PATH", &state);
    cmd.env("XAVIER_DATA_DIR", &data);
    cmd.env_remove("XAVIER_CODE_GRAPH_DB_PATH");
    cmd.env("XAVIER_CODE_EXTRA_ROOTS", &extra_root);

    let out = cmd.output().unwrap();
    assert!(out.status.success(), "Command failed: {:?}", out);

    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let count = json["count"].as_u64().unwrap();
    assert_eq!(
        count, 1,
        "Should only have extra_root since DB does not exist yet."
    );
    assert_eq!(
        json["repos"][0]["root"].as_str().unwrap(),
        extra_root.to_string_lossy()
    );
    assert_eq!(json["repos"][0]["source"].as_str().unwrap(), "env");

    let mut cmd_plain = Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd_plain
        .current_dir(&workspace)
        .args(["code", "repos", "list"]);
    cmd_plain.env("HOME", &home);
    cmd_plain.env("XAVIER_CONFIG_PATH", config.join("config.toml"));
    cmd_plain.env("XAVIER_RUNTIME_STATE_PATH", &state);
    cmd_plain.env("XAVIER_DATA_DIR", &data);
    cmd_plain.env_remove("XAVIER_CODE_GRAPH_DB_PATH");
    cmd_plain.env("XAVIER_CODE_EXTRA_ROOTS", &extra_root);

    let out = cmd_plain.output().unwrap();
    assert!(out.status.success(), "Command failed: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("extra_root"));

    // Ensure it didn't accidentally create the DB
    let db_path = workspace.join(".xavier").join("code_graph.db");
    assert!(
        !db_path.exists(),
        "Read-only command should not create the DB"
    );

    // Populate a DB
    std::fs::create_dir_all(workspace.join(".xavier")).unwrap();
    let db = CodeGraphDB::create_new(&db_path).unwrap();
    db.register_project("/app/service-a", "Service A").unwrap();
    db.register_project("/app/service-b", "Service B").unwrap();

    let src_dir = workspace.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();

    // Now run again with the populated DB from a nested path to verify repo root resolution
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.current_dir(&src_dir)
        .args(["code", "repos", "list", "--json"]);
    cmd.env("HOME", &home);
    cmd.env("XAVIER_CONFIG_PATH", config.join("config.toml"));
    cmd.env("XAVIER_RUNTIME_STATE_PATH", &state);
    cmd.env("XAVIER_DATA_DIR", &data);
    cmd.env_remove("XAVIER_CODE_GRAPH_DB_PATH");
    cmd.env("XAVIER_CODE_EXTRA_ROOTS", &extra_root);

    let out = cmd.output().unwrap();
    assert!(out.status.success(), "Command failed: {:?}", out);
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let count = json["count"].as_u64().unwrap();
    assert_eq!(count, 3, "2 from DB, 1 from ENV");

    let repos = json["repos"].as_array().unwrap();

    let repo_a = repos
        .iter()
        .find(|r| r["root"] == "/app/service-a")
        .unwrap();
    assert_eq!(repo_a["name"], "Service A");
    assert_eq!(repo_a["source"], "index");
    assert!(!repo_a["created_at"].is_null());

    let repo_b = repos
        .iter()
        .find(|r| r["root"] == "/app/service-b")
        .unwrap();
    assert_eq!(repo_b["name"], "Service B");
    assert_eq!(repo_b["source"], "index");
    assert!(!repo_b["created_at"].is_null());

    let repo_extra = repos
        .iter()
        .find(|r| r["root"].as_str().unwrap() == extra_root.to_string_lossy())
        .unwrap();
    assert_eq!(repo_extra["source"], "env");

    let mut cmd_plain = Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd_plain
        .current_dir(&workspace)
        .args(["code", "repos", "list"]);
    cmd_plain.env("HOME", &home);
    cmd_plain.env("XAVIER_CONFIG_PATH", config.join("config.toml"));
    cmd_plain.env("XAVIER_RUNTIME_STATE_PATH", &state);
    cmd_plain.env("XAVIER_DATA_DIR", &data);
    cmd_plain.env_remove("XAVIER_CODE_GRAPH_DB_PATH");
    cmd_plain.env("XAVIER_CODE_EXTRA_ROOTS", &extra_root);

    let out = cmd_plain.output().unwrap();
    assert!(out.status.success(), "Command failed: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Service A"));
    assert!(stdout.contains("Service B"));
    assert!(stdout.contains("extra_root"));

    // Explicit checkpoint wal for shutdown
    db.checkpoint_wal().unwrap();
    drop(db); // Drop the DB so we can corrupt the file

    // 3b. A DB under the configured data dir (what `code sync` writes) wins over
    // the legacy repo-local one.
    let canonical = CodeGraphDB::create_new(&data.join("code_graph.db")).unwrap();
    canonical
        .register_project("/app/canonical-svc", "Canonical Svc")
        .unwrap();
    canonical.checkpoint_wal().unwrap();
    drop(canonical);
    let mut cmd_canon = Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd_canon
        .current_dir(&src_dir)
        .args(["code", "repos", "list", "--json"]);
    cmd_canon.env("HOME", &home);
    cmd_canon.env("XAVIER_CONFIG_PATH", config.join("config.toml"));
    cmd_canon.env("XAVIER_RUNTIME_STATE_PATH", &state);
    cmd_canon.env("XAVIER_DATA_DIR", &data);
    cmd_canon.env_remove("XAVIER_CODE_GRAPH_DB_PATH");
    cmd_canon.env("XAVIER_CODE_EXTRA_ROOTS", &extra_root);
    let out_canon = cmd_canon.output().unwrap();
    assert!(
        out_canon.status.success(),
        "Command failed: {:?}",
        out_canon
    );
    let jc: serde_json::Value = serde_json::from_slice(&out_canon.stdout).unwrap();
    let idx: Vec<&str> = jc["repos"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["source"] == "index")
        .map(|r| r["root"].as_str().unwrap())
        .collect();
    assert_eq!(
        idx,
        vec!["/app/canonical-svc"],
        "data-dir DB must be used: {jc}"
    );
    std::fs::remove_file(data.join("code_graph.db")).unwrap();
    let _ = std::fs::remove_file(data.join("code_graph.db-wal"));
    let _ = std::fs::remove_file(data.join("code_graph.db-shm"));

    // 4. Test error propagation when DB is corrupt
    std::fs::write(
        &db_path,
        b"not a real sqlite database! this is garbage data",
    )
    .unwrap();

    let mut cmd_corrupt = Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd_corrupt
        .current_dir(&workspace)
        .args(["code", "repos", "list", "--json"]);
    cmd_corrupt.env("HOME", &home);
    cmd_corrupt.env("XAVIER_CONFIG_PATH", config.join("config.toml"));
    cmd_corrupt.env("XAVIER_RUNTIME_STATE_PATH", &state);
    cmd_corrupt.env("XAVIER_DATA_DIR", &data);
    cmd_corrupt.env_remove("XAVIER_CODE_GRAPH_DB_PATH");
    cmd_corrupt.env("XAVIER_CODE_EXTRA_ROOTS", &extra_root);

    let out_corrupt = cmd_corrupt.output().unwrap();
    assert!(
        !out_corrupt.status.success(),
        "Corrupt DB should result in a failed command"
    );

    let stderr = String::from_utf8_lossy(&out_corrupt.stderr);
    assert!(
        stderr.contains("file is not a database"),
        "Error should bubble up, stderr: {}",
        stderr
    );
}
