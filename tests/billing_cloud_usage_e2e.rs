use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::tempdir;

xavier::isolate_test_process!();

fn xavier_cli(home: &std::path::Path) -> Command {
    let mut cmd = Command::new(
        std::env::var("CARGO_BIN_EXE_xavier")
            .unwrap_or_else(|_| env!("CARGO_BIN_EXE_xavier").to_string()),
    );
    cmd.env_clear();
    cmd.env("HOME", home.to_str().unwrap());
    cmd.env(
        "XAVIER_CONFIG_PATH",
        home.join(".config").join("xavier").to_str().unwrap(),
    );
    cmd.env(
        "XAVIER_RUNTIME_STATE_PATH",
        home.join(".local")
            .join("state")
            .join("xavier")
            .to_str()
            .unwrap(),
    );
    cmd.env(
        "XAVIER_DATA_DIR",
        home.join(".local")
            .join("share")
            .join("xavier")
            .to_str()
            .unwrap(),
    );
    cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
    cmd
}

fn bind_mock_server() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}", port);
    (listener, url)
}

fn handle_mock_request(
    stream: &mut std::net::TcpStream,
    status: &str,
    response_body: &str,
) -> (String, serde_json::Value) {
    let mut buf = Vec::new();
    let mut chunk = [0; 1024];
    let timeout = Duration::from_secs(2);
    let start = Instant::now();
    let _ = stream.set_read_timeout(Some(timeout));

    let mut header_end = 0;

    // Read headers
    while start.elapsed() < timeout {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    header_end = pos + 4;
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(_) => break,
        }
    }

    let request_str = String::from_utf8_lossy(&buf).to_string();
    let request_str_lower = request_str.to_lowercase();

    // Check for Content-Length
    let mut content_length = 0;
    for line in request_str_lower.lines() {
        if line.starts_with("content-length:") {
            if let Some(val) = line.split(':').nth(1) {
                content_length = val.trim().parse().unwrap_or(0);
            }
        }
    }

    // Read body
    while buf.len() < header_end + content_length && start.elapsed() < timeout {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(_) => break,
        }
    }

    let body_str = String::from_utf8_lossy(&buf[header_end..]).to_string();
    let json_body: serde_json::Value =
        serde_json::from_str(&body_str).unwrap_or(serde_json::Value::Null);

    let response = format!(
        "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        response_body.len(),
        response_body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Both);

    (request_str, json_body)
}

fn wait_with_timeout(mut cmd: Command) -> (Option<i32>, String, String) {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();

    let timeout = Duration::from_secs(30);
    let start = std::time::Instant::now();

    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let mut stdout = String::new();
            if let Some(mut out) = child.stdout.take() {
                let _ = out.read_to_string(&mut stdout);
            }
            let mut stderr = String::new();
            if let Some(mut err) = child.stderr.take() {
                let _ = err.read_to_string(&mut stderr);
            }
            return (status.code(), stdout, stderr);
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            panic!("Test command timed out after {:?}", timeout);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn billing_usage_success_resolves_tenant() {
    let home_dir = tempdir().unwrap();
    let (listener, url) = bind_mock_server();

    let usage_called = Arc::new(Mutex::new(false));
    let usage_called_clone = Arc::clone(&usage_called);

    let assert_called = Arc::new(Mutex::new(String::new()));
    let assert_called_clone = Arc::clone(&assert_called);

    let mock_thread = std::thread::spawn(move || {
        for _ in 0..2 {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = Vec::new();
                let mut chunk = [0; 1024];
                let timeout = Duration::from_secs(2);
                let start = Instant::now();
                let _ = stream.set_read_timeout(Some(timeout));

                let mut header_end = 0;
                while start.elapsed() < timeout {
                    match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                header_end = pos + 4;
                                break;
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                        Err(_) => break,
                    }
                }

                let req_str = String::from_utf8_lossy(&buf).to_string();
                let req_str_lower = req_str.to_lowercase();

                let mut content_length = 0;
                for line in req_str_lower.lines() {
                    if line.starts_with("content-length:") {
                        if let Some(val) = line.split(':').nth(1) {
                            content_length = val.trim().parse().unwrap_or(0);
                        }
                    }
                }

                while buf.len() < header_end + content_length && start.elapsed() < timeout {
                    match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                        Err(_) => break,
                    }
                }

                if req_str_lower.contains("/v1/cloud/usage") {
                    let body = r#"{"tenant":"acme","plan":"respaldo","bytes":1024,"chunks":2,"quota":5368709120,"ai_tokens_per_day":50000,"percent":0}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    if !req_str_lower.contains("authorization: bearer test-tenant-key") {
                        *assert_called_clone.lock().unwrap() =
                            format!("missing bearer. req_str was: {}", req_str);
                    }
                    let _ = stream.write_all(resp.as_bytes());
                    let _ = stream.flush();
                    *usage_called_clone.lock().unwrap() = true;
                } else if req_str_lower.contains("/v1/xavier/checkout") {
                    let body = r#"{"error":"not_available","plan":"pro","status":"coming_soon"}"#;
                    let resp = format!(
                        "HTTP/1.1 409 Conflict\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );

                    let body_str = String::from_utf8_lossy(&buf[header_end..]).to_string();
                    let json_body: serde_json::Value =
                        serde_json::from_str(&body_str).unwrap_or(serde_json::Value::Null);

                    if json_body.get("tenant").and_then(|v| v.as_str()) != Some("acme") {
                        *assert_called_clone.lock().unwrap() =
                            format!("missing tenant. json was: {}", json_body);
                    }

                    let _ = stream.write_all(resp.as_bytes());
                    let _ = stream.flush();
                } else {
                    let resp =
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    let _ = stream.write_all(resp.as_bytes());
                }
            }
        }
    });

    let mut cmd = xavier_cli(home_dir.path());
    cmd.args(["billing", "checkout", "pro", "--json"]);
    cmd.env("XAVIER_PGHEART_URL", &url);
    cmd.env("XAVIER_PGHEART_TOKEN", "test-tenant-key");
    cmd.env("XAVIER_BILLING_URL", &url);

    let (status, stdout, stderr) = wait_with_timeout(cmd);

    mock_thread.join().unwrap(); // Join thread before assertions to propagate thread panics

    let a_err = assert_called.lock().unwrap().clone();
    assert!(a_err.is_empty(), "Mock assertion failed: {}", a_err);

    assert!(
        *usage_called.lock().unwrap(),
        "usage endpoint was not called. Output: status: {:?}, stdout: {}, stderr: {}",
        status,
        stdout,
        stderr
    );

    assert_eq!(status, Some(0), "stderr: {stderr}, stdout: {stdout}");

    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    // In current main, checkout exposes only the resolved tenant (it hits checkout mock returning coming_soon).
    // It doesn't print usage output directly when checking out. We assert what it outputs currently.
    assert_eq!(json["status"], "coming_soon");
    assert_eq!(json["plan"], "pro");
}

#[test]
fn billing_usage_500_fails_checkout() {
    let home_dir = tempdir().unwrap();
    let (listener, url) = bind_mock_server();

    let mock_thread = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            handle_mock_request(
                &mut stream,
                "500 Internal Server Error",
                r#"{"error":"boom"}"#,
            );
        }
    });

    let mut cmd = xavier_cli(home_dir.path());
    cmd.args(["billing", "checkout", "pro", "--json"]);
    cmd.env("XAVIER_PGHEART_URL", &url);
    cmd.env("XAVIER_PGHEART_TOKEN", "test-tenant-key");
    cmd.env("XAVIER_BILLING_URL", &url);

    let (status, _, stderr) = wait_with_timeout(cmd);
    mock_thread.join().unwrap();

    assert_ne!(status, Some(0));
    assert!(stderr.contains("cannot resolve checkout tenant"));
    assert!(stderr.contains("HTTP 500"));
}

#[test]
fn billing_usage_401_unauthorized() {
    let home_dir = tempdir().unwrap();
    let (listener, url) = bind_mock_server();

    let mock_thread = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            handle_mock_request(
                &mut stream,
                "401 Unauthorized",
                r#"{"error":"unauthorized"}"#,
            );
        }
    });

    let mut cmd = xavier_cli(home_dir.path());
    cmd.args(["billing", "checkout", "pro", "--json"]);
    cmd.env("XAVIER_PGHEART_URL", &url);
    cmd.env("XAVIER_PGHEART_TOKEN", "bad-key");
    cmd.env("XAVIER_BILLING_URL", &url);

    let (status, _, stderr) = wait_with_timeout(cmd);
    mock_thread.join().unwrap();

    assert_ne!(status, Some(0));
    assert!(stderr.contains("cannot resolve checkout tenant"));
    assert!(stderr.contains("rejected the credentials (HTTP 401)"));
}

#[test]
fn billing_usage_missing_token_pins_permissive_checkout() {
    let home_dir = tempdir().unwrap();
    let (listener, url) = bind_mock_server();

    let assert_called = Arc::new(Mutex::new(String::new()));
    let assert_called_clone = Arc::clone(&assert_called);

    let mock_thread = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let (req_str, json) = handle_mock_request(
                &mut stream,
                "409 Conflict",
                r#"{"error":"not_available","plan":"pro","status":"coming_soon"}"#,
            );

            if !req_str.contains("POST /v1/xavier/checkout") {
                *assert_called_clone.lock().unwrap() =
                    format!("wrong path. req_str was: {}", req_str);
            } else if !json
                .get("tenant")
                .unwrap_or(&serde_json::Value::Null)
                .is_null()
            {
                *assert_called_clone.lock().unwrap() =
                    format!("tenant was not absent. json was: {}", json);
            }
        }
    });

    let mut cmd = xavier_cli(home_dir.path());
    cmd.args(["billing", "checkout", "pro", "--json"]);
    cmd.env("XAVIER_PGHEART_URL", &url);
    cmd.env("XAVIER_BILLING_URL", &url);

    let (status, stdout, _) = wait_with_timeout(cmd);
    mock_thread.join().unwrap();

    let a_err = assert_called.lock().unwrap().clone();
    assert!(a_err.is_empty(), "Mock assertion failed: {}", a_err);

    // Current checkout deliberately permits missing tokens, so this tests that behavior
    // rather than failing with a configuration error. A non-zero config error for a missing token is out of scope for now.
    assert_eq!(status, Some(0));

    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["status"], "coming_soon");
    assert_eq!(json["plan"], "pro");
}

#[test]
fn billing_usage_server_down() {
    let home_dir = tempdir().unwrap();

    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let url = format!("http://127.0.0.1:{}", port);

    let mut cmd = xavier_cli(home_dir.path());
    cmd.args(["billing", "checkout", "pro", "--json"]);
    cmd.env("XAVIER_PGHEART_URL", &url);
    cmd.env("XAVIER_PGHEART_TOKEN", "bad-key");
    cmd.env("XAVIER_BILLING_URL", &url);

    let (status, _, stderr) = wait_with_timeout(cmd);
    assert_ne!(status, Some(0));
    assert!(stderr.contains("cannot reach xavier-cloud"));
}
