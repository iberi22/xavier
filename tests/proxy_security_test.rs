use reqwest::Client;
use serde_json::json;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;

fn get_free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct XavierServer {
    child: Child,
    port: u16,
    token: String,
    _data_dir: tempfile::TempDir,
}

impl XavierServer {
    fn spawn_with_limit(limit: usize) -> Self {
        let port = get_free_port();
        let token = "test-token-123".to_string();
        let data_dir = tempfile::tempdir().unwrap();

        // Build the binary first to ensure it exists
        let status = Command::new("cargo")
            .args(&["build", "--bin", "xavier"])
            .status()
            .expect("failed to build xavier");
        assert!(status.success());

        let child = Command::new("./target/debug/xavier")
            .env("XAVIER_PORT", port.to_string())
            .env("XAVIER_TOKEN", &token)
            .env("XAVIER_PROXY_RATE_LIMIT", limit.to_string())
            .env("XAVIER_DATA_DIR", data_dir.path())
            .spawn()
            .expect("failed to spawn xavier");

        // Wait for server to be ready
        let mut attempts = 0;
        while attempts < 30 {
            if TcpListener::bind(format!("127.0.0.1:{}", port)).is_err() {
                // Port is in use, server might be up
                break;
            }
            std::thread::sleep(Duration::from_millis(1000));
            attempts += 1;
        }

        Self {
            child,
            port,
            token,
            _data_dir: data_dir,
        }
    }
}

impl Drop for XavierServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[tokio::test]
async fn test_proxy_auth_and_rate_limit_e2e() {
    let server = XavierServer::spawn_with_limit(5);
    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/v1/proxy/request", server.port);

    // 1. Test 401 Unauthorized
    let resp = client
        .post(&url)
        .json(&json!({
            "url": "https://httpbin.org/post",
            "method": "POST",
            "body": {"test": "data"}
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 401);

    // 2. Test 200 OK with token
    let resp = client
        .post(&url)
        .header("X-Xavier-Token", &server.token)
        .json(&json!({
            "url": "https://httpbin.org/post",
            "method": "POST",
            "body": {"test": "data"}
        }))
        .send()
        .await
        .unwrap();

    // It might be 500 if httpbin is down, but not 401.
    assert!(resp.status() == 200 || resp.status() == 500);

    // 3. Test Rate Limiting
    let mut success_count = 0;
    let mut limited_count = 0;

    // We already sent one request above.
    if resp.status().is_success() || resp.status() == 500 {
        success_count += 1;
    }

    for i in 0..10 {
        let resp = client
            .post(&url)
            .header("X-Xavier-Token", &server.token)
            .json(&json!({
                "url": "https://httpbin.org/post",
                "method": "POST",
                "body": {"test": "data"}
            }))
            .send()
            .await
            .unwrap();

        println!("Request {} status: {}", i, resp.status());
        if resp.status().is_success() || resp.status() == 500 {
            success_count += 1;
        } else if resp.status() == 429 {
            limited_count += 1;
        }
    }

    assert!(
        success_count <= 5,
        "Should not allow more than 5 requests, but allowed {}",
        success_count
    );
    assert!(
        limited_count >= 1,
        "Should have been rate limited eventually"
    );
}
