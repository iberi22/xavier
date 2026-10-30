use std::process::{Command, Stdio};
use std::time::Instant;
use tokio::time::{sleep, Duration};

struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// Run with: cargo test -p xavier --test e2e_chat_local -- --ignored --nocapture
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires compiled binary and may be slow in CI; run with --ignored --nocapture"]
async fn test_chat_falls_back_gracefully_when_llm_unavailable() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let data_dir = tempfile::tempdir().unwrap();
    let token = "test-e2e-token";

    let child = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .arg("http")
        .arg(port.to_string())
        .env("XAVIER_HOST", "127.0.0.1")
        .env("XAVIER_PORT", port.to_string())
        .env("XAVIER_TOKEN", token)
        .env("XAVIER_HEADLESS", "true")
        .env("XAVIER_MODEL_PROVIDER", "local")
        .env("XAVIER_LOCAL_LLM_URL", "http://127.0.0.1:1/v1") // puerto cerrado = fallo
        .env("XAVIER_LOCAL_LLM_MODEL", "test-model")
        .env("XAVIER_EMBEDDING_MODEL", "embeddinggemma")
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
    let mut _guard = ChildGuard(child);

    let client = reqwest::Client::new();
    let mut last_error = "no request attempted".to_string();
    let deadline = Instant::now() + Duration::from_secs(55);

    // Poll /ready
    while Instant::now() < deadline {
        // child is moved to _guard, so we check _guard.0
        if let Ok(Some(status)) = _guard.0.try_wait() {
            panic!("xavier http exited during startup with status {status}");
        }

        match client
            .get(format!("http://127.0.0.1:{port}/ready"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => break,
            Ok(resp) => last_error = format!("GET /ready -> HTTP {}", resp.status()),
            Err(err) => last_error = err.to_string(),
        }
        sleep(Duration::from_millis(500)).await;
    }

    if Instant::now() >= deadline {
        panic!("xavier http did not answer GET /ready with 2xx within 55s (last: {last_error})");
    }

    // Sembrar una memoria
    let _ = client
        .post(format!("http://127.0.0.1:{port}/memory/add"))
        .header("X-Xavier-Token", token)
        .json(&serde_json::json!({"content": "Xavier almacena recuerdos en sqlite-vec", "path": "seed/e2e"}))
        .send().await;

    // Enviar chat - el LLM fallara, esperamos respuesta util (no 500)
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

    // Assert headless chat fallback behaves as expected under Ola 4 (01 merged)
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
        .and_then(|arr| arr.first())
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

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
