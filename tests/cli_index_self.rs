use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Router,
};
use serde_json::json;
use std::net::TcpListener;
use std::process::Command;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct MockState {
    requests: Arc<Mutex<Vec<(String, String)>>>, // (path, token)
    status_code: Arc<Mutex<StatusCode>>,
}

struct MockServer {
    url: String,
    state: MockState,
    _stop: tokio::sync::oneshot::Sender<()>,
    _thread: std::thread::JoinHandle<()>,
}

impl MockServer {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);
        listener.set_nonblocking(true).unwrap();
        let state = MockState::default();
        let state_clone = state.clone();

        let app = Router::new()
            .route(
                "/memory/index-self",
                post(
                    |State(state): State<MockState>, headers: HeaderMap, _body: Bytes| async move {
                        let token = headers
                            .get("x-xavier-token")
                            .map(|v| v.to_str().unwrap_or("").to_string())
                            .unwrap_or_default();
                        state
                            .requests
                            .lock()
                            .unwrap()
                            .push(("/memory/index-self".to_string(), token));

                        let code = *state.status_code.lock().unwrap();
                        if code.is_success() {
                            (code, axum::Json(json!({"indexed": true, "doc_count": 42})))
                        } else {
                            (code, axum::Json(json!({"error": "internal error"})))
                        }
                    },
                ),
            )
            .with_state(state_clone);

        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let _thread = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                axum::serve(listener, app)
                    .with_graceful_shutdown(async move {
                        rx.await.ok();
                    })
                    .await
                    .unwrap();
            });
        });

        Self {
            url,
            state,
            _stop: tx,
            _thread,
        }
    }
}

/// Child command with a clean environment that still carries the sandboxed
/// HOME/XDG/XAVIER_* dirs installed by `isolate_test_process!` (so the child
/// never reads or writes real user config/logs) and runs in a temp cwd.
fn xavier_cmd(mock_url: &str, cwd: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.env_clear().current_dir(cwd);
    for (key, value) in std::env::vars_os() {
        let k = key.to_string_lossy();
        if k == "PATH"
            || k == "HOME"
            || k.starts_with("XDG_")
            || (k.starts_with("XAVIER_") && k != "XAVIER_URL" && k != "XAVIER_TOKEN")
        {
            cmd.env(&key, &value);
        }
    }
    assert!(
        std::env::var_os("HOME").is_some(),
        "isolate_test_process! must have sandboxed HOME"
    );
    cmd.env("XAVIER_URL", mock_url)
        .env("XAVIER_TOKEN", "my-test-token");
    cmd
}

#[test]
fn test_cli_index_self() {
    let mock = MockServer::new();
    let cwd = tempfile::tempdir().expect("temp cwd");

    // (a) `xavier index-self --json` hits POST /memory/index-self with token
    let output = xavier_cmd(&mock.url, cwd.path())
        .args(["index-self", "--json"])
        .output()
        .expect("failed to execute");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), r#"{"doc_count":42,"indexed":true}"#);

    {
        let mut reqs = mock.state.requests.lock().unwrap();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].0, "/memory/index-self");
        assert_eq!(reqs[0].1, "my-test-token");
        reqs.clear();
    }

    // (b) server 500 -> non-zero exit and {"ok":false,"error":...} on stdout
    *mock.state.status_code.lock().unwrap() = StatusCode::INTERNAL_SERVER_ERROR;
    let output = xavier_cmd(&mock.url, cwd.path())
        .args(["index-self", "--json"])
        .output()
        .expect("failed to execute");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("JSON on stdout");
    assert_eq!(v["ok"], false);
    assert!(v["error"].as_str().unwrap().contains("internal error"));

    {
        let mut reqs = mock.state.requests.lock().unwrap();
        assert_eq!(reqs.len(), 1);
        reqs.clear();
    }

    // (c) `xavier memory index-self` hits the same route
    *mock.state.status_code.lock().unwrap() = StatusCode::OK;
    let output = xavier_cmd(&mock.url, cwd.path())
        .args(["memory", "index-self", "--json"])
        .output()
        .expect("failed to execute");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), r#"{"doc_count":42,"indexed":true}"#);

    {
        let reqs = mock.state.requests.lock().unwrap();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].0, "/memory/index-self");
    }
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();

/// Connection failure (nothing listening) with --json still emits one JSON
/// object on stdout and exits nonzero, for both entry points.
#[test]
fn test_cli_index_self_json_on_connection_failure() {
    let cwd = tempfile::tempdir().expect("temp cwd");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    drop(listener);
    for args in [
        &["index-self", "--json"][..],
        &["memory", "index-self", "--json"][..],
    ] {
        let output = xavier_cmd(&dead_url, cwd.path())
            .args(args)
            .output()
            .expect("failed to execute");
        assert!(!output.status.success(), "{:?} must fail", args);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let v: serde_json::Value = serde_json::from_str(stdout.trim())
            .unwrap_or_else(|e| panic!("{:?}: stdout not JSON ({e}): {stdout}", args));
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()));
    }
}
