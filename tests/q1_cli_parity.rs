use serde_json::Value;

#[tokio::test]
async fn test_cli_parity_config_llm() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();

    // config llm set-model
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.env("XAVIER_STATE_DIR", state_dir.to_str().unwrap());
    cmd.env(
        "XAVIER_RUNTIME_STATE_PATH",
        temp.path().join("state.json").to_str().unwrap(),
    );
    cmd.env(
        "XAVIER_CONFIG_PATH",
        temp.path().join("config.json").to_str().unwrap(),
    );

    cmd.args(["config", "llm", "set-model", "test-model-42", "--json"]);
    let assert = cmd.output().unwrap();
    assert!(assert.status.success());
    let stdout = std::str::from_utf8(&assert.stdout).unwrap();
    let val: Value = serde_json::from_str(stdout).unwrap();
    assert_eq!(val["operation"], "set-model");
    assert_eq!(val["model"], "test-model-42");

    // config llm show
    let mut cmd_show = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd_show.env("XAVIER_STATE_DIR", state_dir.to_str().unwrap());
    cmd_show.env(
        "XAVIER_RUNTIME_STATE_PATH",
        temp.path().join("state.json").to_str().unwrap(),
    );
    cmd_show.env(
        "XAVIER_CONFIG_PATH",
        temp.path().join("config.json").to_str().unwrap(),
    );

    cmd_show.args(["config", "llm", "show", "--json"]);
    let assert_show = cmd_show.output().unwrap();
    assert!(assert_show.status.success());
    let stdout_show = std::str::from_utf8(&assert_show.stdout).unwrap();
    let val_show: Value = serde_json::from_str(stdout_show).unwrap();
    assert_eq!(val_show["operation"], "show");
    assert_eq!(val_show["entry"]["value"], "test-model-42");
}

#[tokio::test]
async fn test_cli_parity_index_and_memory() {
    xavier::isolate_test_process!();

    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();

    std::env::set_var("XAVIER_STATE_DIR", state_dir.to_str().unwrap());
    std::env::set_var(
        "XAVIER_CONFIG_PATH",
        temp.path().join("config.json").to_str().unwrap(),
    );
    std::fs::write(temp.path().join("config.json"), "{}").unwrap();

    let temp_router = tempfile::tempdir().unwrap();
    let state_dir = temp_router.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    std::env::set_var("XAVIER_STATE_DIR", state_dir.to_str().unwrap());
    std::env::set_var(
        "XAVIER_CONFIG_PATH",
        temp_router.path().join("config.json").to_str().unwrap(),
    );
    std::fs::write(temp_router.path().join("config.json"), "{}").unwrap();

    let app = axum::Router::new()
        .route(
            "/code/scan",
            axum::routing::post(|| async { axum::Json(serde_json::json!({ "status": "ok" })) }),
        )
        .route(
            "/code/sidecar/status",
            axum::routing::get(|| async { axum::Json(serde_json::json!({ "status": "ok" })) }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    // Test index status
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.env("XAVIER_STATE_DIR", state_dir.to_str().unwrap());
    cmd.env("XAVIER_URL", &url);
    cmd.env("XAVIER_TOKEN", "test-token");
    cmd.args(["index", "status", "--json"]);
    let assert = cmd.output().unwrap();
    assert!(assert.status.success());
    let stdout = std::str::from_utf8(&assert.stdout).unwrap();
    let val: Value = serde_json::from_str(stdout).unwrap();
    assert!(val.get("status").is_some());

    // Test index add
    let mut cmd2 = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd2.env("XAVIER_STATE_DIR", state_dir.to_str().unwrap());
    cmd2.env("XAVIER_URL", &url);
    cmd2.env("XAVIER_TOKEN", "test-token");
    cmd2.args(["index", "add", temp.path().to_str().unwrap(), "--json"]);
    let assert2 = cmd2.output().unwrap();
    assert!(assert2.status.success());
    let stdout2 = std::str::from_utf8(&assert2.stdout).unwrap();
    let val2: Value = serde_json::from_str(stdout2).unwrap();
    assert_eq!(val2["status"], "ok");

    // Test memory load
    let mut cmd3 = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd3.env("XAVIER_STATE_DIR", state_dir.to_str().unwrap());
    cmd3.env("XAVIER_URL", &url);
    cmd3.env("XAVIER_TOKEN", "test-token");
    cmd3.args(["memory", "load", temp.path().to_str().unwrap()]);
    assert!(cmd3.status().unwrap().success());
}
