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

#[test]
fn test_cli_index_self() {
    let mock = MockServer::new();

    // (a) `xavier index-self --json` hits POST /memory/index-self with token
    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .env_clear()
        .env("XAVIER_URL", &mock.url)
        .env("XAVIER_TOKEN", "my-test-token")
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
    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .env_clear()
        .env("XAVIER_URL", &mock.url)
        .env("XAVIER_TOKEN", "my-test-token")
        .args(["index-self", "--json"])
        .output()
        .expect("failed to execute");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(r#"{"ok":false,"error":"{\"error\":\"internal error\"}"}"#));

    {
        let mut reqs = mock.state.requests.lock().unwrap();
        assert_eq!(reqs.len(), 1);
        reqs.clear();
    }

    // (c) `xavier memory index-self` hits the same route
    *mock.state.status_code.lock().unwrap() = StatusCode::OK;
    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .env_clear()
        .env("XAVIER_URL", &mock.url)
        .env("XAVIER_TOKEN", "my-test-token")
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
