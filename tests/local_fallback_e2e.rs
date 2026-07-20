use std::process::{Command, Stdio};
use tokio::time::{sleep, Duration};

struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// Run with: cargo test -p xavier --test local_fallback_e2e -- --ignored --nocapture
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore] // requires compiled binary and may be slow in CI
async fn test_local_fallback_e2e_flow() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let data_dir = tempfile::tempdir().unwrap();
    let token = "test-e2e-token";

    // Set XAVIER_JWT_SECRET to prevent authentication failure at boot
    let child = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .arg("http")
        .arg(port.to_string())
        .env("XAVIER_HOST", "127.0.0.1")
        .env("XAVIER_PORT", port.to_string())
        .env("XAVIER_TOKEN", token)
        .env("XAVIER_HEADLESS", "true")
        .env("XAVIER_MODEL_PROVIDER", "local")
        .env("XAVIER_LOCAL_LLM_URL", "http://127.0.0.1:1/v1") // closed port to trigger failure
        .env("XAVIER_LOCAL_LLM_MODEL", "qwen3-coder")
        .env("XAVIER_EMBEDDING_MODEL", "embeddinggemma")
        .env("XAVIER_JWT_SECRET", "super-secret-e2e-jwt-key")
        .env(
            "XAVIER_CODE_GRAPH_DB_PATH",
            data_dir.path().join(format!("code-{port}.db")),
        )
        .env(
            "XAVIER_MEMORY_VEC_PATH",
            data_dir.path().join(format!("vec-{port}.db")),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn xavier");
    let _guard = ChildGuard(child);

    // Poll /health until server responds
    let client = reqwest::Client::new();
    let mut server_up = false;
    for _ in 0..30 {
        if client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            server_up = true;
            break;
        }
        sleep(Duration::from_millis(500)).await;
    }
    assert!(server_up, "Server did not start in 15s");

    // Seed a memory record
    let _ = client
        .post(format!("http://127.0.0.1:{port}/memory/add"))
        .header("X-Xavier-Token", token)
        .json(&serde_json::json!({
            "content": "Xavier almacena recuerdos en sqlite-vec",
            "path": "seed/e2e_fallback"
        }))
        .send()
        .await;

    // Send chat completion request - primary proxy falls back, expect 200 with local fallback execution
    let resp = client
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .header("X-Xavier-Token", token)
        .json(&serde_json::json!({
            "model": "auto",
            "messages": [{"role": "user", "content": "donde guarda Xavier los recuerdos?"}]
        }))
        .send()
        .await
        .unwrap();

    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or_default();

    assert_eq!(
        status.as_u16(),
        200,
        "Expected HTTP 200 but got {}: {:?}",
        status,
        body
    );

    let model = body.get("model").and_then(|m| m.as_str()).unwrap_or("");
    assert_eq!(
        model, "memory-fallback",
        "Expected model 'memory-fallback' but got '{}'",
        model
    );

    let content = body
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.get(0))
        .and_then(|first| first.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|cnt| cnt.as_str())
        .unwrap_or("");

    assert!(
        content.contains("Modo memoria") || content.contains("[Modo memoria"),
        "Expected content to contain memory fallback marker, but got: '{}'",
        content
    );
}
