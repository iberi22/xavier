use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const SECRET: &str = "onboarding-fixture-secret";

struct Sandbox(tempfile::TempDir);
impl Sandbox {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }

    fn command(&self, url: &str, args: &[&str]) -> Command {
        let root = self.0.path();
        let mut command = Command::new(env!("CARGO_BIN_EXE_xavier"));
        command.env_clear().current_dir(root).stdin(Stdio::null());
        for (key, suffix) in [
            ("HOME", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "share"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_RUNTIME_DIR", "run"),
            ("XAVIER_CONFIG_PATH", "config.json"),
            ("XAVIER_RUNTIME_STATE_PATH", "runtime.json"),
            ("XAVIER_DATA_DIR", "data"),
            ("XAVIER_STATE_DIR", "state"),
            ("XAVIER_LOG_DIR", "logs"),
        ] {
            command.env(key, root.join(suffix));
        }
        command
            .env("XAVIER_URL", url)
            .env("XAVIER_MCP_PORT", "18123")
            .env("XAVIER_TOKEN", SECRET)
            .env("NO_PROXY", "127.0.0.1")
            .args(args);
        command
    }

    fn run(&self, url: &str, args: &[&str]) -> Output {
        self.command(url, args).output().unwrap()
    }
}

#[cfg(unix)]
#[test]
fn explicit_confirmation_still_refuses_terminal_secret_output() {
    use std::os::fd::FromRawFd;
    let sandbox = Sandbox::new();
    for args in [
        vec![
            "agent",
            "onboard",
            "--reveal",
            "--i-understand-output-is-not-a-tty",
        ],
        vec![
            "token",
            "issue",
            "--agent",
            "reader",
            "--scope",
            "read",
            "--ttl",
            "1h",
            "--i-understand-output-is-not-a-tty",
        ],
    ] {
        let (mut master, mut slave) = (-1, -1);
        // openpty initializes both descriptors on success; each File owns one descriptor.
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                )
            },
            0
        );
        let _master = unsafe { std::fs::File::from_raw_fd(master) };
        let slave = unsafe { std::fs::File::from_raw_fd(slave) };
        let output = sandbox
            .command("http://127.0.0.1:1", &args)
            .stdout(slave)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("non-terminal"), "{stderr}");
        assert!(!stderr.contains(SECRET));
    }
}

struct Stub {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    worker: Option<JoinHandle<()>>,
}
impl Stub {
    fn new(status: u16, body: Value) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let worker = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                if socket.read(&mut byte).unwrap_or(0) == 0 {
                    return;
                }
                request.push(byte[0]);
            }
            let headers = String::from_utf8(request.clone()).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            let mut body_bytes = vec![0; length];
            socket.read_exact(&mut body_bytes).unwrap();
            request.extend(body_bytes);
            captured
                .lock()
                .unwrap()
                .push(String::from_utf8(request).unwrap());
            let body = body.to_string();
            let _ = write!(socket, "HTTP/1.1 {status} Stub\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        });
        Self {
            url,
            requests,
            worker: Some(worker),
        }
    }
}
impl Drop for Stub {
    fn drop(&mut self) {
        let _ = TcpStream::connect(self.url.trim_start_matches("http://"));
        self.worker.take().unwrap().join().unwrap();
    }
}

fn document(output: &Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout must be a single JSON document")
}

#[test]
fn onboard_reports_endpoints_health_and_client_env_references() {
    let sandbox = Sandbox::new();
    for (client, reference) in [
        ("generic", "${XAVIER_TOKEN}"),
        ("claude", "${XAVIER_TOKEN}"),
        ("codex", "bearer_token_env_var"),
        ("opencode", "{env:XAVIER_TOKEN}"),
    ] {
        let stub = Stub::new(200, json!({"status":"healthy"}));
        let output = sandbox.run(
            &stub.url,
            &["agent", "onboard", "--json", "--client", client],
        );
        let data = document(&output, 0);
        assert_eq!(data["schema_version"], 1);
        assert_eq!(data["base_url"], stub.url);
        assert_eq!(data["mcp_url"], "http://127.0.0.1:18123/mcp");
        assert_eq!(data["health"]["status"], "healthy");
        assert_eq!(
            data["token_source"],
            json!({"kind":"env", "reference":"XAVIER_TOKEN"})
        );
        assert!(data["mcp_config"].as_str().unwrap().contains(reference));
        if client == "opencode" {
            let config: Value = serde_json::from_str(data["mcp_config"].as_str().unwrap()).unwrap();
            assert_eq!(config["mcp"]["xavier"]["type"], "remote");
            assert_eq!(config["mcp"]["xavier"]["oauth"], false);
            assert_eq!(
                config["mcp"]["xavier"]["headers"]["Authorization"],
                "Bearer {env:XAVIER_TOKEN}"
            );
        } else if client != "codex" {
            let config: Value = serde_json::from_str(data["mcp_config"].as_str().unwrap()).unwrap();
            assert_eq!(config["mcpServers"]["xavier"]["type"], "http");
            assert_eq!(
                config["mcpServers"]["xavier"]["headers"]["Authorization"],
                "Bearer ${XAVIER_TOKEN}"
            );
        }
        assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(SECRET));
    }
}

#[test]
fn onboard_reveal_requires_explicit_confirmation() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(
        "http://127.0.0.1:1",
        &["agent", "onboard", "--reveal", "--json"],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--i-understand-output-is-not-a-tty"));
    let stub = Stub::new(200, json!({"status":"ok"}));
    let output = sandbox.run(
        &stub.url,
        &[
            "agent",
            "onboard",
            "--json",
            "--reveal",
            "--i-understand-output-is-not-a-tty",
        ],
    );
    assert_eq!(document(&output, 0)["token"], SECRET);
    assert!(!String::from_utf8_lossy(&output.stderr).contains(SECRET));
}

#[test]
fn issue_writes_private_file_and_only_metadata_to_json() {
    let sandbox = Sandbox::new();
    let stub = Stub::new(
        201,
        json!({"status":"ok", "token":"xav_issued_fixture", "metadata":{
            "id":"key-1", "name":"reader", "scopes":["read"],
            "expires_at":"2030-01-01T00:00:00Z", "created_at":"2026-01-01T00:00:00Z", "last_used_at":null, "partial_hash":"abc..."
        }}),
    );
    let path = sandbox.0.path().join("agent.key");
    let output = sandbox.run(
        &stub.url,
        &[
            "token",
            "issue",
            "--agent",
            "reader",
            "--scope",
            "read",
            "--ttl",
            "2h",
            "--json",
            "--out-file",
            path.to_str().unwrap(),
        ],
    );
    let data = document(&output, 0);
    assert_eq!(data["metadata"]["id"], "key-1");
    assert_eq!(data["metadata"]["agent"], "reader");
    assert_eq!(data["metadata"]["scope"], "read");
    assert_eq!(data["metadata"]["expires_at"], "2030-01-01T00:00:00Z");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("xav_issued_fixture"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("xav_issued_fixture"));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap().trim(),
        "xav_issued_fixture"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let requests = stub.requests.lock().unwrap();
    assert!(requests[0].starts_with("POST /security/tokens "));
    let body: Value = serde_json::from_str(requests[0].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["name"], "reader");
    assert_eq!(body["scopes"], json!(["read"]));
    let expiry =
        chrono::DateTime::parse_from_rfc3339(body["expires_at"].as_str().unwrap()).unwrap();
    assert!((expiry.timestamp() - chrono::Utc::now().timestamp() - 7200).abs() < 10);
}

#[test]
fn keys_list_without_name_returns_only_metadata() {
    let sandbox = Sandbox::new();
    let stub = Stub::new(
        200,
        json!({"status":"ok", "tokens":[{
            "id":"key-1", "name":"reader", "scopes":["read"], "expires_at":null,
            "created_at":"2026-01-01T00:00:00Z", "last_used_at":null, "partial_hash":"abc...",
            "token":"must-not-escape", "hash":"must-not-escape"
        }]}),
    );
    let output = sandbox.run(&stub.url, &["keys", "list", "--json"]);
    let data = document(&output, 0);
    assert_eq!(data["keys"][0]["id"], "key-1");
    assert_eq!(data["keys"][0]["agent"], "reader");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("must-not-escape"));
    assert!(stub.requests.lock().unwrap()[0].starts_with("GET /security/tokens "));
}

#[test]
fn auth_and_connection_failures_have_distinct_exit_codes() {
    let sandbox = Sandbox::new();
    for status in [401, 403] {
        for args in [
            vec!["keys", "list", "--json"],
            vec!["agent", "onboard", "--json"],
            vec![
                "token",
                "issue",
                "--agent",
                "reader",
                "--scope",
                "read",
                "--ttl",
                "1h",
                "--json",
                "--out-file",
                "issued.key",
            ],
        ] {
            let stub = Stub::new(status, json!({"error":SECRET}));
            let output = sandbox.run(&stub.url, &args);
            assert_eq!(document(&output, 3)["error"]["code"], 3);
            assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET));
            assert!(!String::from_utf8_lossy(&output.stderr).contains(SECRET));
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let output = sandbox.run(&url, &["keys", "list", "--json"]);
    assert_eq!(document(&output, 4)["error"]["code"], 4);
}

#[test]
fn issue_rejects_invalid_ttl_and_json_without_secret_file() {
    let sandbox = Sandbox::new();
    for args in [
        vec![
            "token",
            "issue",
            "--agent",
            "reader",
            "--scope",
            "read",
            "--ttl",
            "0s",
            "--out-file",
            "key",
        ],
        vec![
            "token", "issue", "--agent", "reader", "--scope", "write", "--ttl", "1h", "--json",
        ],
    ] {
        let output = sandbox.run("http://127.0.0.1:1", &args);
        assert_eq!(output.status.code(), Some(2));
        assert!(!sandbox.0.path().join("key").exists());
    }
}

#[test]
fn issue_refuses_existing_file_before_issuing_a_key() {
    let sandbox = Sandbox::new();
    let path = sandbox.0.path().join("existing.key");
    std::fs::write(&path, "preserve-existing-content").unwrap();
    let output = sandbox.run(
        "http://127.0.0.1:1",
        &[
            "token",
            "issue",
            "--agent",
            "reader",
            "--scope",
            "read",
            "--ttl",
            "1h",
            "--out-file",
            path.to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(document(&output, 1)["error"]["code"], 1);
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "preserve-existing-content"
    );
}

#[test]
fn issue_to_stdout_requires_confirmation_and_keeps_metadata_separate() {
    let sandbox = Sandbox::new();
    let args = [
        "token", "issue", "--agent", "reader", "--scope", "write", "--ttl", "1h",
    ];
    let output = sandbox.run("http://127.0.0.1:1", &args);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--out-file"));
    let stub = Stub::new(
        201,
        json!({"status":"ok", "token":"xav_stdout_fixture", "metadata":{
            "id":"key-2", "name":"reader", "scopes":["write"],
            "expires_at":"2030-01-01T00:00:00Z", "created_at":"2026-01-01T00:00:00Z", "last_used_at":null, "partial_hash":"def..."
        }}),
    );
    let mut confirmed = args.to_vec();
    confirmed.push("--i-understand-output-is-not-a-tty");
    let output = sandbox.run(&stub.url, &confirmed);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "xav_stdout_fixture\n"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("xav_stdout_fixture"));
    assert!(stderr.contains("key-2"));
    let requests = stub.requests.lock().unwrap();
    let body: Value = serde_json::from_str(requests[0].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["scopes"], json!(["write"]));
}
