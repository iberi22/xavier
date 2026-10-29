use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;

xavier::isolate_test_process!();

// Integration test for `xavier billing status`

/// Create a Command invoking the Xavier CLI.
fn xavier_cli() -> Command {
    let cmd = Command::new(
        std::env::var("CARGO_BIN_EXE_xavier")
            .unwrap_or_else(|_| env!("CARGO_BIN_EXE_xavier").to_string()),
    );

    cmd
}

#[test]
fn billing_status_reachable() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}", port);

    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0; 1024];
            let mut read = 0;
            while read < buf.len() {
                let n = stream.read(&mut buf[read..]).unwrap_or(0);
                if n == 0 {
                    break;
                }
                read += n;
                if buf[..read].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
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

// The default fallback test is handled inside unit tests for formatting (since binding real 8787 is flaky)
