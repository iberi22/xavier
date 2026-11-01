use axum::{routing::post, Router};
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

xavier::isolate_test_process!();

#[tokio::test]
async fn test_llm_routing_config() {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path();

    // Create channels to capture requests
    let (tx, mut _rx) = mpsc::channel::<Value>(10);

    // 1. Mock server
    let mock_app = Router::new().route(
        "/v1/chat/completions",
        post(move |body: axum::extract::Json<Value>| async move {
            tx.send(body.0.clone()).await.unwrap();
            axum::Json(json!({
                "id": "chatcmpl-123",
                "object": "chat.completion",
                "created": 1677652288,
                "model": body["model"],
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello! I am a mock.",
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 9,
                    "completion_tokens": 12,
                    "total_tokens": 21
                }
            }))
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mock_url = format!("http://127.0.0.1:{}/v1", addr.port());

    tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let run_cli = |args: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xavier"));
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", root_path)
            .env("XDG_CONFIG_HOME", root_path.join("xdg-config"))
            .env("XDG_STATE_HOME", root_path.join("xdg-state"))
            .env("XAVIER_CONFIG_PATH", root_path.join("config.json"))
            .env("XAVIER_RUNTIME_STATE_PATH", root_path.join("runtime.json"))
            .env("XAVIER_DATA_DIR", root_path.join("data"))
            .env("XAVIER_URL", "http://127.0.0.1:0")
            // configure mock API
            .env("XAVIER_OPENAI_URL", &mock_url)
            .env("XAVIER_OPENAI_API_KEY", "mock")
            .env("XAVIER_LOCAL_LLM_URL", &mock_url)
            .current_dir(root_path)
            .stdin(Stdio::null())
            .args(args);

        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "CLI failed with status {:?} stderr: {} stdout: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        let value: Value =
            serde_json::from_slice(&output.stdout).expect("stdout is one JSON document");
        value
    };

    // Initialize config implicitly by setting a route
    let set_output = run_cli(&[
        "config",
        "llm",
        "route",
        "set",
        "summarize",
        "openai/mock-model",
        "--json",
    ]);
    assert_eq!(set_output["operation"], "set_route");
    assert_eq!(set_output["task"], "summarize");
    assert_eq!(set_output["target"], "openai/mock-model");

    let list_output = run_cli(&["config", "llm", "route", "list", "--json"]);
    assert_eq!(list_output["operation"], "list_routes");
    assert_eq!(list_output["routes"]["summarize"], "openai/mock-model");
}
