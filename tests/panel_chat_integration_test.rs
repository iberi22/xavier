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
async fn test_panel_chat_integration() {
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port();

    let url = format!("http://127.0.0.1:{port}/panel/api/chat");

    let _child = ChildGuard(
        std::process::Command::new(env!("CARGO_BIN_EXE_xavier"))
            .env("XAVIER_HOST", "127.0.0.1")
            .env("XAVIER_PORT", port.to_string())
            .env("XAVIER_TOKEN", "test-token")
            .env("XAVIER_HEADLESS", "true")
            .env(
                "XAVIER_CODE_GRAPH_DB_PATH",
                format!("data/panel-test-code-{port}.db"),
            )
            .env(
                "XAVIER_MEMORY_VEC_PATH",
                format!("data/panel-test-mem-{port}.db"),
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

    // Send POST /panel/api/chat
    let resp = client
        .post(&url)
        .header("X-Xavier-Token", "test-token")
        .json(&serde_json::json!({
            "message": "Hola, explain xavier memory"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    // Check thread and messages structure
    assert!(body["thread"]["id"].is_string());
    let messages = body["messages"].as_array().expect("messages is array");
    assert_eq!(messages.len(), 2);

    // First message: user
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "Hola, explain xavier memory");

    // Second message: assistant (should NOT be stub format!("Structured Xavier response for: ..."))
    assert_eq!(messages[1]["role"], "assistant");
    let assistant_content = messages[1]["content"].as_str().expect("content is string");

    // It shouldn't contain the old hardcoded synthetic string "Structured Xavier response for:"
    assert!(!assistant_content.contains("Structured Xavier response for:"));

    // Since LLM will likely fail or fall back (due to no keys/mocked LLM),
    // it will contain the fallback pattern or degraded message.
    assert!(
        assistant_content.contains("[LLM no disponible") ||
        assistant_content.contains("[Modo memoria") ||
        assistant_content.contains("respuesta")
    );

    // Let's also check the openui_lang block structure contains the Timing / Status
    let openui_lang = messages[1]["openui_lang"].as_str().expect("openui_lang is string");
    assert!(openui_lang.contains("<SectionBlock title=\"Xavier"));
    assert!(openui_lang.contains("<InfoCard title=\"Status\""));

    // Let's check metadata structure
    let meta_str = messages[1]["metadata"].as_str().expect("metadata is string");
    let meta: Value = serde_json::from_str(meta_str).expect("metadata is valid JSON");
    assert!(meta["timings"]["total_ms"].is_number());
}
