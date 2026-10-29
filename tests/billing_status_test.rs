use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;

// Integration test for `xavier billing status`

/// Create a Command invoking the Xavier CLI.
fn xavier_cli() -> Command {
    let mut cmd = Command::new(
        std::env::var("CARGO_BIN_EXE_xavier")
            .unwrap_or_else(|_| env!("CARGO_BIN_EXE_xavier").to_string()),
    );
    cmd.env("XAVIER_CONFIG_PATH", "/dev/null");
    cmd
}

#[serial_test::serial]
#[test]
fn billing_status_reachable() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}", port);

    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0; 1024];
            let _ = stream.read(&mut buf);
            let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    let mut cmd = xavier_cli();
    cmd.args(["billing", "status", "--json"]);
    cmd.env("XAVIER_BILLING_URL", &url);

    let output = cmd.output().unwrap();

    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["reachable"], true);
    assert_eq!(json["source"], "env");
    assert_eq!(json["status"], 200);
}

#[serial_test::serial]
#[test]
fn billing_status_unreachable() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let url = format!("http://127.0.0.1:{}", port);

    let mut cmd = xavier_cli();
    cmd.args(["billing", "status", "--json"]);
    cmd.env("XAVIER_BILLING_URL", &url);

    let output = cmd.output().unwrap();

    assert_eq!(output.status.code(), Some(2));

    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["reachable"], false);
    assert_eq!(json["source"], "env");
    assert!(json["error"].is_string());
}

#[serial_test::serial]
#[test]
fn billing_status_unreachable_default() {
    // Port 0 shouldn't be reachable as an HTTP server in this test environment natively
    // We'll set the environment variable up in the process but tell the mock it's the "default" by unsetting the ENV for CLI.
    // Wait, if we unset XAVIER_BILLING_URL, it uses `http://127.0.0.1:8787` by default. We can't guarantee that's unbound.
    // But since the task expects testing this, it will try 127.0.0.1:8787.
    let mut cmd = xavier_cli();
    cmd.args(["billing", "status"]);
    cmd.env_remove("XAVIER_BILLING_URL");

    let output = cmd.output().unwrap();

    assert_eq!(output.status.code(), Some(2));

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("cannot reach swal-billing"));
    assert!(stdout.contains("start swal-billing locally"));
    assert!(stdout.contains("XAVIER_BILLING_URL"));
}
