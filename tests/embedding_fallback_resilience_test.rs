//! Resilience tests for embedding fallback paths under offline / degraded conditions.

use axum::{routing::get, Json, Router};
use serde_json::{json, Value};
use serial_test::serial;
use tokio::net::TcpListener;

use xavier::embedding::build_embedder_from_env;
use xavier::server::alerts::SYSTEM_ALERTS;

struct EnvGuard {
    vars: Vec<&'static str>,
}

impl EnvGuard {
    fn new(vars: Vec<&'static str>) -> Self {
        Self { vars }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for var in &self.vars {
            std::env::remove_var(var);
        }
    }
}

/// Starts a mock Ollama server that handles GET `/v1/models` and returns the specified models.
async fn start_mock_ollama_server(models: Vec<&'static str>) -> String {
    let app = Router::new().route(
        "/v1/models",
        get(move || {
            let model_objs: Vec<Value> = models
                .iter()
                .map(|m| json!({"id": m, "object": "model"}))
                .collect();
            async move {
                Json(json!({
                    "object": "list",
                    "data": model_objs
                }))
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind TcpListener");
    let addr = listener.local_addr().expect("failed to get local_addr");
    let url = format!("http://{}", addr);

    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    url
}

#[tokio::test]
#[serial]
async fn test_fallback_selection() {
    SYSTEM_ALERTS.clear();

    let env_vars = vec![
        "XAVIER_EMBEDDING_MODEL",
        "XAVIER_OLLAMA_MODEL",
        "XAVIER_OLLAMA_URL",
        "_XAVIER_TEST_OLLAMA_PROBE_URL",
        "XAVIER_EMBEDCACHE_ENABLED",
        "XAVIER_EMBEDDING_PROVIDER_MODE",
    ];
    let _guard = EnvGuard::new(env_vars);

    let mock_url = start_mock_ollama_server(vec!["nomic-embed-text:latest"]).await;

    std::env::set_var("XAVIER_EMBEDDING_MODEL", "embeddinggemma");
    std::env::set_var("XAVIER_OLLAMA_MODEL", "nomic-embed-text:latest");
    std::env::set_var("XAVIER_OLLAMA_URL", format!("{}/api/embed", mock_url));
    std::env::set_var(
        "_XAVIER_TEST_OLLAMA_PROBE_URL",
        format!("{}/v1/models", mock_url),
    );
    std::env::set_var("XAVIER_EMBEDCACHE_ENABLED", "false");

    let embedder_res = build_embedder_from_env().await;
    assert!(
        embedder_res.is_ok(),
        "build_sync() should succeed when fallback model is available"
    );

    let embedder = embedder_res.unwrap();
    assert_eq!(
        embedder.dimension(),
        768,
        "fallback embedder dimension should match 768"
    );
}

#[tokio::test]
#[serial]
async fn test_no_system_alerts_on_fallback() {
    SYSTEM_ALERTS.clear();

    let env_vars = vec![
        "XAVIER_EMBEDDING_MODEL",
        "XAVIER_OLLAMA_MODEL",
        "XAVIER_OLLAMA_URL",
        "_XAVIER_TEST_OLLAMA_PROBE_URL",
        "XAVIER_EMBEDCACHE_ENABLED",
        "XAVIER_EMBEDDING_PROVIDER_MODE",
    ];
    let _guard = EnvGuard::new(env_vars);

    let mock_url = start_mock_ollama_server(vec!["nomic-embed-text:latest"]).await;

    std::env::set_var("XAVIER_EMBEDDING_MODEL", "embeddinggemma");
    std::env::set_var("XAVIER_OLLAMA_MODEL", "nomic-embed-text:latest");
    std::env::set_var("XAVIER_OLLAMA_URL", format!("{}/api/embed", mock_url));
    std::env::set_var(
        "_XAVIER_TEST_OLLAMA_PROBE_URL",
        format!("{}/v1/models", mock_url),
    );
    std::env::set_var("XAVIER_EMBEDCACHE_ENABLED", "false");

    let embedder_res = build_embedder_from_env().await;
    assert!(
        embedder_res.is_ok(),
        "build_sync() should succeed on fallback selection"
    );

    let alerts = SYSTEM_ALERTS.get_alerts();
    let embedding_alerts: Vec<String> = alerts
        .into_iter()
        .filter(|a| a.component == "embedding")
        .map(|a| format!("[{}] {}", a.level, a.message))
        .collect();

    assert!(
        embedding_alerts.is_empty(),
        "expected no embedding system alerts on successful fallback, got {:?}",
        embedding_alerts
    );
}

#[tokio::test]
#[serial]
async fn test_full_degraded_path() {
    SYSTEM_ALERTS.clear();

    let env_vars = vec![
        "XAVIER_EMBEDDING_MODEL",
        "XAVIER_OLLAMA_MODEL",
        "XAVIER_OLLAMA_URL",
        "_XAVIER_TEST_OLLAMA_PROBE_URL",
        "XAVIER_EMBEDCACHE_ENABLED",
        "XAVIER_EMBEDDING_PROVIDER_MODE",
        "XAVIER_GLLM_MODEL",
    ];
    let _guard = EnvGuard::new(env_vars);

    let mock_url = start_mock_ollama_server(vec![]).await;

    std::env::set_var(
        "_XAVIER_TEST_OLLAMA_PROBE_URL",
        format!("{}/v1/models", mock_url),
    );
    std::env::set_var("XAVIER_EMBEDCACHE_ENABLED", "false");
    std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "local-gllm");
    std::env::set_var("XAVIER_GLLM_MODEL", "minilm");

    let embedder_res = build_embedder_from_env().await;
    assert!(
        embedder_res.is_ok(),
        "build_embedder_from_env should return NoopEmbedder on degraded path"
    );

    let alerts = SYSTEM_ALERTS.get_alerts();
    let error_alerts: Vec<_> = alerts
        .iter()
        .filter(|a| a.component == "embedding" && a.level.eq_ignore_ascii_case("error"))
        .collect();

    assert_eq!(
        error_alerts.len(),
        1,
        "expected exactly one severity Error alert in SYSTEM_ALERTS on full degraded path, got: {:?}",
        alerts
            .iter()
            .map(|a| format!("[{}] {}", a.level, a.message))
            .collect::<Vec<_>>()
    );
}
