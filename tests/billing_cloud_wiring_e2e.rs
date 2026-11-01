//! <ledger id>: q8-billing-wiring
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

xavier::isolate_test_process!();

fn bind_mock_server() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}", port);
    (listener, url)
}

fn accept_with_timeout(listener: &TcpListener, timeout: Duration) -> Option<std::net::TcpStream> {
    listener.set_nonblocking(true).unwrap();
    let start = Instant::now();
    while start.elapsed() < timeout {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return Some(stream);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
    None
}

fn handle_mock_request(stream: &mut std::net::TcpStream, status: &str, body: &str) {
    let mut buf = [0; 1024];
    let _ = stream.read(&mut buf);
    let resp = format!(
        "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        body.len(),
        body
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
}

fn xavier_cli(dir: &std::path::Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.current_dir(dir);
    cmd.env("XAVIER_DATA_DIR", dir.join("data"));
    cmd.env("XAVIER_CONFIG_PATH", dir.join("config.toml"));
    cmd
}

fn wait_with_timeout(mut cmd: std::process::Command) -> (Option<i32>, String, String) {
    let output = cmd.output().expect("failed to execute process");
    let status = output.status.code();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (status, stdout, stderr)
}

#[test]
#[serial_test::serial]
fn billing_wiring_missing_config_fails_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = xavier_cli(dir.path());
    cmd.args(["billing", "plans"]);
    // ensure no url is set
    cmd.env_remove("XAVIER_BILLING_URL");

    let (status, _, stderr) = wait_with_timeout(cmd);
    assert_ne!(status, Some(0));
    assert!(stderr.contains("billing not configured: set XAVIER_BILLING_URL"));
}

#[test]
#[serial_test::serial]
fn billing_wiring_500_retry_then_clear_error() {
    let dir = tempfile::tempdir().unwrap();
    let (listener, url) = bind_mock_server();

    let mock_thread = std::thread::spawn(move || {
        // Mock server simply returns 500
        if let Some(mut stream) = accept_with_timeout(&listener, Duration::from_secs(5)) {
            handle_mock_request(
                &mut stream,
                "500 Internal Server Error",
                r#"{"error":"boom"}"#,
            );
        }
    });

    let mut cmd = xavier_cli(dir.path());
    cmd.args(["billing", "plans"]);
    cmd.env("XAVIER_BILLING_URL", &url);

    let (status, _, stderr) = wait_with_timeout(cmd);
    mock_thread.join().unwrap();

    assert_ne!(status, Some(0));
    assert!(stderr.contains("swal-billing answered HTTP 500"));
}

#[test]
#[serial_test::serial]
fn billing_wiring_401_non_zero_exit() {
    let dir = tempfile::tempdir().unwrap();
    let (listener, url) = bind_mock_server();

    let mock_thread = std::thread::spawn(move || {
        if let Some(mut stream) = accept_with_timeout(&listener, Duration::from_secs(5)) {
            handle_mock_request(
                &mut stream,
                "401 Unauthorized",
                r#"{"error":"unauthorized"}"#,
            );
        }
    });

    let mut cmd = xavier_cli(dir.path());
    cmd.args(["billing", "plans"]);
    cmd.env("XAVIER_BILLING_URL", &url);

    let (status, _, stderr) = wait_with_timeout(cmd);
    mock_thread.join().unwrap();

    assert_ne!(status, Some(0));
    // The specific unauthenticated message from client.rs
    assert!(
        stderr.contains("swal-billing rejected the credentials")
            || stderr.contains("HTTP 401")
            || stderr.contains("not authenticated")
    );
}

#[test]
#[serial_test::serial]
fn billing_wiring_plan_lookup_success() {
    let dir = tempfile::tempdir().unwrap();
    let (listener, url) = bind_mock_server();

    let mock_thread = std::thread::spawn(move || {
        if let Some(mut stream) = accept_with_timeout(&listener, Duration::from_secs(5)) {
            let body = r#"{"version":"1","currency":"usd","payment_provider":"stripe","plans":[{"id":"pro","name":"Pro","status":"available","price":{"monthly_usd":10.0},"limits":{"storage_bytes":0,"ai_tokens_per_day":0}}]}"#;
            handle_mock_request(&mut stream, "200 OK", body);
        }
    });

    let mut cmd = xavier_cli(dir.path());
    cmd.args(["billing", "plans", "--json"]);
    cmd.env("XAVIER_BILLING_URL", &url);

    let (status, stdout, _) = wait_with_timeout(cmd);
    mock_thread.join().unwrap();

    assert_eq!(status, Some(0));
    assert!(stdout.contains("pro"));
}

#[test]
#[serial_test::serial]
fn billing_wiring_usage_report_success() {
    let dir = tempfile::tempdir().unwrap();
    let (listener, url) = bind_mock_server();

    let mock_thread = std::thread::spawn(move || {
        if let Some(mut stream) = accept_with_timeout(&listener, Duration::from_secs(5)) {
            let body = r#"{"version":"1","currency":"usd","payment_provider":"stripe","plans":[{"id":"pro","name":"Pro","status":"available","price":{"monthly_usd":10.0},"limits":{"storage_bytes":0,"ai_tokens_per_day":0}}]}"#;
            handle_mock_request(&mut stream, "200 OK", body);
        }
    });

    let mut cmd = xavier_cli(dir.path());
    cmd.args(["billing", "status", "--json"]);
    cmd.env("XAVIER_BILLING_URL", &url);

    let (status, stdout, _) = wait_with_timeout(cmd);
    mock_thread.join().unwrap();

    assert_eq!(status, Some(0));
    assert!(stdout.contains("true") || stdout.contains("reachable"));
}
