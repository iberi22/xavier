use reqwest::{Client, StatusCode};
use std::net::TcpListener;
use std::process::{Child, Stdio};
use std::time::Duration;

struct ChildGuard {
    child: Child,
    db_paths: Vec<String>,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        for path in &self.db_paths {
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_file(format!("{}-wal", path));
            let _ = std::fs::remove_file(format!("{}-shm", path));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_headless_chat_completions() {
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port();

    let url = format!("http://127.0.0.1:{port}/v1/chat/completions");

    let code_db = format!("data/headless-chat-code-{port}.db");
    let mem_db = format!("data/headless-chat-mem-{port}.db");

    let _child = ChildGuard {
        child: std::process::Command::new(env!("CARGO_BIN_EXE_xavier"))
            .env("XAVIER_HOST", "127.0.0.1")
            .env("XAVIER_PORT", port.to_string())
            .env("XAVIER_TOKEN", "test-token")
            .env("XAVIER_HEADLESS", "true")
            .env("XAVIER_CODE_GRAPH_DB_PATH", &code_db)
            .env("XAVIER_MEMORY_VEC_PATH", &mem_db)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("failed to start xavier binary"),
        db_paths: vec![code_db, mem_db],
    };

    let client = Client::new();
    let mut started = false;
    for _ in 0..30 {
        if let Ok(resp) = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await
        {
            if resp.status().is_success() {
                started = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(started, "Headless server failed to start");

    // 1. POST /v1/chat/completions (No Auth) -> 401
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Quasar nebula zephyr orchid."}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 2. POST /v1/chat/completions (Valid Token, No Lease)
    // Note: This will reach the proxy use case but likely fail with 429 (RateLimited)
    // because no providers are configured in the test environment.
    let resp = client
        .post(&url)
        .header("X-Xavier-Token", "test-token")
        .json(&serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Quasar nebula zephyr orchid."}]
        }))
        .send()
        .await
        .unwrap();

    // We mostly care that it's NOT a 404 or 401.
    assert!(
        resp.status() == StatusCode::TOO_MANY_REQUESTS
            || resp.status() == StatusCode::INTERNAL_SERVER_ERROR
            || resp.status() == StatusCode::OK
    );

    // 3. POST /v1/chat/completions (Valid Token, Invalid Lease) -> 403 (FORBIDDEN)
    let resp = client
        .post(&url)
        .header("X-Xavier-Token", "test-token")
        .json(&serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Quasar nebula zephyr orchid."}],
            "lease_token": "invalid-lease"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
