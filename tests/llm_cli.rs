//! Process-level contracts for provider configuration and unattended setup.
xavier::isolate_test_process!();

use axum::{body::Bytes, extract::State, http::StatusCode, routing::any, Json, Router};
use serde_json::{json, Value};
use std::future::IntoFuture;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::{Arc, Mutex},
};

struct Harness(tempfile::TempDir);
impl Harness {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.path().join(name)
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));
        cmd.env_clear()
            .current_dir(self.0.path())
            .stdin(Stdio::null())
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_STATE_HOME", self.path("state"))
            .env("XDG_DATA_HOME", self.path("share"))
            .env("XDG_CACHE_HOME", self.path("cache"))
            .env("XDG_RUNTIME_DIR", self.path("run"))
            .env("XAVIER_CONFIG_PATH", self.path("config.json"))
            .env("XAVIER_RUNTIME_STATE_PATH", self.path("runtime.json"))
            .env("XAVIER_DATA_DIR", self.path("data"))
            .env("XAVIER_URL", "http://127.0.0.1:0")
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", self.path("nobus").display()),
            )
            .args(args);
        cmd
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }
    fn set(&self, provider: &str, url: &str, scope: &str) {
        parsed(self.run(&[
            "llm",
            "set",
            "--provider",
            provider,
            "--model",
            "test-model",
            "--url",
            url,
            "--scope",
            scope,
            "--json",
        ]));
    }
}
fn parsed(output: Output) -> Value {
    assert!(
        output.status.success(),
        "exit {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout must contain exactly one JSON document")
}
#[derive(Clone)]
struct StubState {
    code: StatusCode,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
    headers: Arc<Mutex<Vec<axum::http::HeaderMap>>>,
    hang: bool,
    invalid_completion: bool,
}
struct Stub {
    url: String,
    state: StubState,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Stub {
    fn new(code: StatusCode, invalid_completion: bool) -> Self {
        Self::with_hang(code, invalid_completion, false)
    }
    fn with_hang(code: StatusCode, invalid_completion: bool, hang: bool) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let state = StubState {
            code,
            requests: Arc::default(),
            invalid_completion,
            headers: Arc::default(),
            hang,
        };
        let app = Router::new()
            .fallback(any(
                |State(state): State<StubState>,
                 uri: axum::http::Uri,
                 headers: axum::http::HeaderMap,
                 body: Bytes| async move {
                    state.headers.lock().unwrap().push(headers);
                    if state.hang {
                        std::future::pending::<()>().await;
                    }
                    let path = uri.path().to_string();
                    let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    state.requests.lock().unwrap().push((path.clone(), request));
                    let response = if path == "/health" {
                        json!({"llm": {"configured": true}})
                    } else if !state.code.is_success() {
                        json!({"error": "synthetic-secret-do-not-echo"})
                    } else if path.ends_with("/models") {
                        json!({"data": [{"id":"test-model"}, {"id":"other-model"}]})
                    } else if state.invalid_completion {
                        json!({"error":"synthetic-secret-do-not-echo"})
                    } else if path.ends_with("/messages") {
                        json!({"content": [{"type":"text", "text":"ok"}]})
                    } else {
                        json!({"choices": [{"message": {"content":"ok"}}]})
                    };
                    (state.code, Json(response))
                },
            ))
            .with_state(state.clone());
        let (stop, rx) = tokio::sync::oneshot::channel();
        let thread = std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(async move {
                    tokio::select! {
                        result = axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), app).into_future() => { result.unwrap(); }
                        _ = rx => {}
                    }
                })
        });
        Self {
            url,
            state,
            stop: Some(stop),
            thread: Some(thread),
        }
    }
}
impl Drop for Stub {
    fn drop(&mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.thread.take().unwrap().join().unwrap();
    }
}

#[test]
fn set_status_persists_runtime_and_reports_health_and_sources() {
    let h = Harness::new();
    let stub = Stub::new(StatusCode::OK, false);
    h.set("local", &stub.url, "general");
    assert!(!h.path("config.json").exists());
    let saved: Value = serde_json::from_slice(&fs::read(h.path("runtime.json")).unwrap()).unwrap();
    assert_eq!(saved["models"]["local_llm_model"], "test-model");
    let status = parsed(
        h.command(&["llm", "status", "--json"])
            .env("XAVIER_URL", stub.url.trim_end_matches("/v1"))
            .output()
            .unwrap(),
    );
    assert_eq!(status["schema_version"], 1);
    assert_eq!(status["provider"], "local");
    assert_eq!(status["model"], "test-model");
    assert_eq!(status["base_url"], stub.url);
    assert_eq!(status["source"]["model"], "runtime");
    assert_eq!(status["api_key"], "unset");
    assert_eq!(status["daemon"]["llm_configured"], true);
}
#[test]
fn status_respects_environment_precedence() {
    let h = Harness::new();
    h.set("local", "http://127.0.0.1:1/v1", "general");
    let status = parsed(
        h.command(&["llm", "status", "--json"])
            .env("XAVIER_LOCAL_LLM_MODEL", "override-model")
            .output()
            .unwrap(),
    );
    assert_eq!(status["model"], "override-model");
    assert_eq!(status["source"]["model"], "env:XAVIER_LOCAL_LLM_MODEL");
    assert_eq!(status["daemon"]["reachable"], false);
    assert!(status["daemon"]["llm_configured"].is_null());
}
#[test]
fn explain_scope_persists_separately_and_inherits_until_set() {
    let h = Harness::new();
    h.set("local", "http://127.0.0.1:1/v1", "general");
    assert_eq!(
        parsed(h.run(&["llm", "status", "--scope", "explain", "--json"]))["model"],
        "test-model"
    );
    h.set("anthropic", "http://127.0.0.1:2/v1", "explain");
    let status = parsed(h.run(&["llm", "status", "--scope", "explain", "--json"]));
    assert_eq!(status["provider"], "anthropic");
    assert_eq!(status["base_url"], "http://127.0.0.1:2/v1");
    assert_eq!(status["source"]["provider"], "runtime");
    assert_eq!(
        parsed(h.run(&["llm", "status", "--json"]))["provider"],
        "local"
    );
}
#[test]
fn list_models_uses_configured_endpoint_and_json_only() {
    for provider in ["local", "openai", "openrouter", "groq", "anthropic"] {
        let h = Harness::new();
        let stub = Stub::new(StatusCode::OK, false);
        h.set(provider, &stub.url, "general");
        let value = parsed(h.run(&["llm", "list-models", "--json"]));
        assert_eq!(value["models"], json!(["test-model", "other-model"]));
        assert_eq!(stub.state.requests.lock().unwrap()[0].0, "/v1/models");
    }
}
#[test]
fn test_probes_models_then_one_token_completion_for_both_protocols() {
    for (provider, scope, path) in [
        ("local", "general", "/v1/chat/completions"),
        ("anthropic", "explain", "/v1/messages"),
    ] {
        let h = Harness::new();
        let stub = Stub::new(StatusCode::OK, false);
        h.set(provider, &stub.url, scope);
        let result = parsed(h.run(&["llm", "test", "--scope", scope, "--json"]));
        assert_eq!(result["ok"], true);
        let requests = stub.state.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].0, "/v1/models");
        assert_eq!(requests[1].0, path);
        assert_eq!(requests[1].1["max_tokens"], 1);
        assert_eq!(requests[1].1["model"], "test-model");
    }
}
#[test]
fn provider_failures_have_honest_codes_without_echoing_response_bodies() {
    for (status, expected) in [
        (StatusCode::UNAUTHORIZED, 3),
        (StatusCode::FORBIDDEN, 3),
        (StatusCode::INTERNAL_SERVER_ERROR, 1),
    ] {
        let h = Harness::new();
        let stub = Stub::new(status, false);
        h.set("local", &stub.url, "general");
        for action in ["test", "list-models"] {
            let out = h.run(&["llm", action, "--json"]);
            assert_eq!(out.status.code(), Some(expected));
            assert!(!String::from_utf8_lossy(&out.stderr).contains("synthetic-secret"));
            let value: Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(value["ok"], false);
        }
    }
    let h = Harness::new();
    h.set("local", "http://127.0.0.1:0/v1", "general");
    assert_eq!(h.run(&["llm", "test", "--json"]).status.code(), Some(4));
}
#[test]
fn test_rejects_success_status_with_invalid_completion_body() {
    let h = Harness::new();
    let stub = Stub::new(StatusCode::OK, true);
    h.set("local", &stub.url, "general");
    assert_eq!(h.run(&["llm", "test", "--json"]).status.code(), Some(1));
}
#[test]
fn key_file_and_stdin_use_vault_without_plaintext_output_or_runtime_storage() {
    for stdin in [false, true] {
        let h = Harness::new();
        let key = "synthetic-secret-do-not-echo";
        fs::write(h.path("key-input"), key).unwrap();
        let mut cmd = h.command(&[
            "llm",
            "set",
            "--provider",
            "groq",
            "--model",
            "test-model",
            "--json",
        ]);
        if stdin {
            cmd.arg("--api-key-stdin").stdin(Stdio::piped());
        } else {
            cmd.arg("--api-key-file").arg(h.path("key-input"));
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        if stdin {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(key.as_bytes())
                .unwrap();
        }
        let output = child.wait_with_output().unwrap();
        assert!(!String::from_utf8_lossy(&output.stdout).contains(key));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(key));
        parsed(output);
        assert!(!fs::read_to_string(h.path("runtime.json"))
            .unwrap()
            .contains(key));
        let status = parsed(h.run(&["llm", "status", "--json"]));
        assert_eq!(status["api_key"], "set");
        assert_eq!(status["source"]["api_key"], "vault");
    }
}
#[test]
fn argv_credentials_are_rejected_without_echoing_them() {
    let h = Harness::new();
    for args in [
        vec![
            "llm",
            "set",
            "--provider",
            "openai",
            "--model",
            "m",
            "--api-key=synthetic-secret-do-not-echo",
        ],
        vec![
            "llm",
            "set",
            "--provider",
            "local",
            "--model",
            "m",
            "--url",
            "http://user:synthetic-secret-do-not-echo@localhost/v1",
        ],
    ] {
        let out = h.run(&args);
        assert_eq!(out.status.code(), Some(2));
        assert!(!String::from_utf8_lossy(&out.stderr).contains("synthetic-secret"));
        assert!(!h.path("runtime.json").exists());
    }
}
#[test]
fn unattended_setup_saves_without_prompting_or_downloading() {
    for yes in [false, true] {
        let h = Harness::new();
        let mut args = vec!["setup", "--local", "--model", "test-model", "--json"];
        if yes {
            args.push("--yes");
        }
        let result = parsed(h.run(&args));
        assert_eq!(result["restart_required"], true);
        let saved: Value =
            serde_json::from_slice(&fs::read(h.path("runtime.json")).unwrap()).unwrap();
        assert_eq!(saved["models"]["local_llm_model"], "test-model");
        assert!(!h.path(".env").exists());
        assert!(!h.path("config.json").exists());
    }
}
#[test]
fn unattended_setup_missing_inputs_fails_with_usage_hint() {
    let h = Harness::new();
    for args in [
        vec!["setup"],
        vec!["setup", "--yes"],
        vec!["setup", "--local", "--yes"],
        vec!["setup", "--local"],
    ] {
        let out = h.run(&args);
        assert_eq!(out.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&out.stderr).contains("--model"));
        assert!(!h.path("runtime.json").exists());
    }
}

#[test]
fn probes_timeout_and_send_provider_auth_headers() {
    let h = Harness::new();
    let stub = Stub::with_hang(StatusCode::OK, false, true);
    h.set("local", &stub.url, "general");
    let start = std::time::Instant::now();
    assert_eq!(
        h.run(&["llm", "list-models", "--timeout-secs", "1", "--json"])
            .status
            .code(),
        Some(4)
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
    for (provider, scope) in [("openai", "general"), ("anthropic", "explain")] {
        let h = Harness::new();
        let stub = Stub::new(StatusCode::OK, false);
        fs::write(h.path("key-input"), "synthetic-test-key").unwrap();
        parsed(
            h.command(&[
                "llm",
                "set",
                "--provider",
                provider,
                "--model",
                "test-model",
                "--url",
                &stub.url,
                "--scope",
                scope,
                "--api-key-file",
            ])
            .arg(h.path("key-input"))
            .output()
            .unwrap(),
        );
        parsed(h.run(&["llm", "test", "--scope", scope, "--json"]));
        for headers in stub.state.headers.lock().unwrap().iter() {
            if provider == "anthropic" {
                assert_eq!(headers["x-api-key"], "synthetic-test-key");
                assert_eq!(headers["anthropic-version"], "2023-06-01");
            } else {
                assert_eq!(headers["authorization"], "Bearer synthetic-test-key");
            }
        }
        if scope == "explain" {
            assert_eq!(
                parsed(h.run(&["llm", "status", "--json"]))["api_key"],
                "unset"
            );
        }
    }
}
#[test]
fn set_validates_inputs_and_replaces_stale_openrouter_model() {
    let h = Harness::new();
    for args in [
        vec!["llm", "set", "--provider", "openai", "--model", ""],
        vec![
            "llm",
            "set",
            "--provider",
            "openai",
            "--model",
            "m",
            "--api-key-file",
            "missing",
            "--api-key-stdin",
        ],
    ] {
        assert_eq!(h.run(&args).status.code(), Some(2));
        assert!(!h.path("runtime.json").exists());
    }
    fs::write(
        h.path("runtime.json"),
        r#"{"models":{"cloud_llm_model":"stale-model"}}"#,
    )
    .unwrap();
    parsed(h.run(&[
        "llm",
        "set",
        "--provider",
        "openrouter",
        "--model",
        "fresh-model",
        "--json",
    ]));
    assert_eq!(
        parsed(h.run(&["llm", "status", "--json"]))["model"],
        "fresh-model"
    );
}

#[test]
fn provider_switch_resets_shared_urls_and_status_matches_daemon_precedence() {
    for scope in ["general", "explain"] {
        let h = Harness::new();
        h.set("groq", "http://127.0.0.1:1/v1", scope);
        parsed(h.run(&[
            "llm",
            "set",
            "--provider",
            "openai",
            "--model",
            "fresh",
            "--scope",
            scope,
            "--json",
        ]));
        let status = parsed(h.run(&["llm", "status", "--scope", scope, "--json"]));
        assert_eq!(status["base_url"], "https://api.openai.com/v1");
    }
    let h = Harness::new();
    h.set("local", "http://127.0.0.1:1/v1", "general");
    let status = parsed(
        h.command(&["llm", "status", "--json"])
            .env("OPENROUTER_API_KEY", "synthetic-test-key")
            .env("XAVIER_LOCAL_ANTHROPIC_URL", "http://127.0.0.1:2")
            .output()
            .unwrap(),
    );
    assert_eq!(status["provider"], "local");
    assert_eq!(status["source"]["provider"], "runtime");
    assert_eq!(status["source"]["base_url"], "runtime");
    let status = parsed(
        h.command(&["llm", "status", "--json"])
            .env("XAVIER_LOCAL_LLM_API_KEY", "ollama")
            .output()
            .unwrap(),
    );
    assert_eq!(status["api_key"], "set");
    assert_eq!(status["source"]["api_key"], "env:XAVIER_LOCAL_LLM_API_KEY");
}
#[test]
fn setters_repair_old_urls_without_seeding_repository_config() {
    let h = Harness::new();
    fs::write(
        h.path("runtime.json"),
        r#"{"models":{"provider":"local","local_llm_url":"invalid"}}"#,
    )
    .unwrap();
    h.set("local", "http://127.0.0.1:1/v1", "general");
    fs::create_dir_all(h.path("config")).unwrap();
    fs::write(h.path("config/xavier.config.example.json"), "{}").unwrap();
    parsed(
        h.command(&[
            "llm",
            "set",
            "--provider",
            "local",
            "--model",
            "test-model",
            "--json",
        ])
        .env_remove("XAVIER_CONFIG_PATH")
        .output()
        .unwrap(),
    );
    assert!(!h.path("config/xavier.config.json").exists());
}
#[test]
fn response_body_timeout_has_unreachable_exit_code() {
    use std::io::Read;
    let h = Harness::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let (stop, rx) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut buf = [0; 4096];
        let _ = stream.read(&mut buf).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n",
            )
            .unwrap();
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    });
    h.set("local", &url, "general");
    let output = h.run(&["llm", "list-models", "--timeout-secs", "1", "--json"]);
    stop.send(()).unwrap();
    server.join().unwrap();
    assert_eq!(output.status.code(), Some(4));
}
