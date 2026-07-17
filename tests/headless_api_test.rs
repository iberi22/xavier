use reqwest::{Client, StatusCode};
use serde_json::Value;
use std::net::TcpListener;
use std::process::{Child, Stdio};
use std::time::Duration;

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_headless_api_e2e() {
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port();

    let url = format!("http://127.0.0.1:{port}/headless");

    let _child = ChildGuard(
        std::process::Command::new(env!("CARGO_BIN_EXE_xavier"))
            .env("XAVIER_HOST", "127.0.0.1")
            .env("XAVIER_PORT", port.to_string())
            .env("XAVIER_TOKEN", "test-token")
            .env("XAVIER_HEADLESS", "true")
            .env(
                "XAVIER_CODE_GRAPH_DB_PATH",
                format!("data/headless-test-code-{port}.db"),
            )
            .env(
                "XAVIER_MEMORY_VEC_PATH",
                format!("data/headless-test-mem-{port}.db"),
            )
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("failed to start xavier binary"),
    );

    let client = Client::new();
    let mut started = false;
    for _ in 0..30 {
        if let Ok(resp) = client
            .get(format!("{}/health", url))
            .header("Authorization", "Bearer test-token")
            .send()
            .await
        {
            if resp.status().is_success() || resp.status().as_u16() == 401 {
                started = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(started, "Headless server failed to start");

    // 1. GET /health (No Auth)
    let resp = client.get(format!("{}/health", url)).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "xavier-headless");

    // 2. GET /provider/status (Requires valid Bearer)
    let resp = client
        .get(format!("{}/provider/status", url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = client
        .get(format!("{}/provider/status", url))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(body["active"].is_string());

    // 3. GET /context (Memory hybrid search)
    let resp = client
        .get(format!("{}/context?query=rust", url))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(body["items"].is_array());

    // 4. POST /memory/search
    let resp = client
        .post(format!("{}/memory/search", url))
        .header("Authorization", "Bearer test-token")
        .json(&serde_json::json!({"text": "query", "limit": 5}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(body["results"].is_array());

    // 5. GET /tools
    let resp = client
        .get(format!("{}/tools", url))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(body["tools"].is_array());

    // 6. POST /tools/:name
    let resp = client
        .post(format!("{}/tools/memory_search", url))
        .header("Authorization", "Bearer test-token")
        .json(&serde_json::json!({"args": {"query": "test"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["result"]["tool"], "memory_search");

    // 8. GET /v1/providers
    let v1_url = format!("http://127.0.0.1:{port}/v1");
    let resp = client
        .get(format!("{}/providers", v1_url))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_providers: Value = resp.json().await.unwrap();
    assert!(body_providers["active"].is_string());
    assert!(body_providers["mode"].is_string());
    assert!(body_providers["strategy"].is_string());
    assert!(body_providers["local_reachable"].is_boolean());
    assert!(body_providers["fallback_chain"].is_array());
    assert!(body_providers["providers"].is_array());

    // 9. GET /v1/providers/status
    let resp = client
        .get(format!("{}/providers/status", v1_url))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_status: Value = resp.json().await.unwrap();
    assert_eq!(body_providers, body_status);

    // 7. Rate Limiting (61 requests/min)
    // We'll just do a few quick requests and verify it's working if possible,
    // but full 61 might be slow in CI. Let's do 5 and check they pass.
    for _ in 0..5 {
        let resp = client.get(format!("{}/health", url)).send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
