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
async fn test_health_endpoints_e2e() {
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port();

    let _child = ChildGuard(
        std::process::Command::new(env!("CARGO_BIN_EXE_xavier"))
            .env("XAVIER_HOST", "127.0.0.1")
            .env("XAVIER_PORT", port.to_string())
            .env("XAVIER_TOKEN", "test-token")
            .env("XAVIER_HEADLESS", "true")
            .env(
                "XAVIER_CODE_GRAPH_DB_PATH",
                format!("data/health-test-code-{port}.db"),
            )
            .env(
                "XAVIER_MEMORY_VEC_PATH",
                format!("data/health-test-mem-{port}.db"),
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
    assert!(started, "Server failed to start");

    // 1. GET /health (unauthenticated)
    let resp = client
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    // Check basic top-level keys
    assert!(body.get("mode").is_some());
    assert!(body.get("system").is_some());

    // 2. GET /v1/system/health (requires valid Bearer token / X-Xavier-Token)
    let resp = client
        .get(format!("http://127.0.0.1:{port}/v1/system/health"))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body_v1: Value = resp.json().await.unwrap();

    // Assert specified JSON shape and values for /v1/system/health:
    // - mode (string)
    // - llm.reachable (bool)
    // - embeddings.model (string no vacío)
    // - uptime_secs (número >= 0)

    let mode = body_v1["mode"]
        .as_str()
        .expect("mode should be a string");
    println!("Retrieved mode: {}", mode);

    let llm_reachable = body_v1["llm"]["reachable"]
        .as_bool()
        .expect("llm.reachable should be a boolean");
    println!("Retrieved llm.reachable: {}", llm_reachable);

    let embeddings_model = body_v1["embeddings"]["model"]
        .as_str()
        .expect("embeddings.model should be a string");
    assert!(!embeddings_model.is_empty(), "embeddings.model should not be empty");
    println!("Retrieved embeddings.model: {}", embeddings_model);

    let uptime_secs = body_v1["uptime_secs"]
        .as_u64()
        .expect("uptime_secs should be a number >= 0");
    println!("Retrieved uptime_secs: {}", uptime_secs);

    // Assert rest of the expected fields exist to ensure no regressions
    assert_eq!(body_v1["service"], "xavier-headless");
    assert!(body_v1["version"].is_string());
    assert!(body_v1["status"].is_string());

    assert!(body_v1["llm"]["provider"].is_string());
    assert!(body_v1["llm"]["model"].is_string());
    assert!(body_v1["llm"]["endpoint"].is_string());
    assert!(body_v1["llm"]["status"].is_string());

    assert!(body_v1["embeddings"]["provider"].is_string());
    assert!(body_v1["embeddings"]["status"].is_string());
    assert!(body_v1["embeddings"]["reachable"].is_boolean());
    assert!(body_v1["embeddings"]["latency_ms"].is_number());
    assert!(body_v1["embeddings"]["error_rate"].is_number());

    assert!(body_v1["vector_db"]["backend"].is_string());
    assert!(body_v1["vector_db"]["path"].is_string());
    assert!(body_v1["vector_db"]["status"].is_string());

    // Backward-compatibility keys
    assert!(body_v1["database"].is_object());
    assert!(body_v1["system"].is_object());
    assert!(body_v1["mesh"].is_object());
}
