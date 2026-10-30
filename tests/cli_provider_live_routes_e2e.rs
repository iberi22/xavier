use axum::{
    routing::{get, post},
    Json, Router,
};
use std::process::Command;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tokio::net::TcpListener;

xavier::isolate_test_process!();

#[derive(Clone, Default)]
struct RequestLog {
    switches: Arc<Mutex<Vec<serde_json::Value>>>,
    all_requests: Arc<Mutex<usize>>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn test_provider_live_routes() {
    let tmp_config = TempDir::new().unwrap();
    let tmp_data = TempDir::new().unwrap();

    let config_path = tmp_config.path().join("config.toml");
    std::fs::write(&config_path, "{}").unwrap();

    let request_log = RequestLog::default();
    let log_clone = request_log.clone();

    // Mock only the /v1/providers and /v1/providers/switch routes per requirements
    let log_middleware = request_log.clone();
    let router = Router::new()
        .route(
            "/v1/providers",
            get(|| async {
                Json(serde_json::json!({
                    "providers": [
                        {"name": "mock-provider", "configured": true}
                    ]
                }))
            }),
        )
        .route(
            "/v1/providers/switch",
            post(|Json(body): Json<serde_json::Value>| async move {
                log_clone.switches.lock().unwrap().push(body.clone());
                Json(serde_json::json!({
                    "success": true,
                    "message": "Switched provider",
                    "new": body.get("provider").unwrap_or(&serde_json::json!("")).as_str().unwrap_or("")
                }))
            }),
        )
        .route("/401/v1/providers/switch", post(|| async { axum::http::StatusCode::UNAUTHORIZED }))
        .route("/403/v1/providers/switch", post(|| async { axum::http::StatusCode::FORBIDDEN }))
        .route("/404/v1/providers/switch", post(|| async { axum::http::StatusCode::NOT_FOUND }))
        .route("/500/v1/providers/switch", post(|| async { axum::http::StatusCode::INTERNAL_SERVER_ERROR }))
        .fallback(
            axum::routing::any(|_req: axum::extract::Request| async move {
                // Return 404 for unhandled paths but test should assert we never reach here for 'auto'
                axum::http::StatusCode::NOT_FOUND
            })
        )
        // Layer last so it wraps every route and the fallback (axum only
        // layers routes that already exist when `.layer` is called).
        .layer(axum::middleware::from_fn(move |req: axum::extract::Request, next: axum::middleware::Next| {
            let log = log_middleware.clone();
            async move {
                *log.all_requests.lock().unwrap() += 1;
                next.run(req).await
            }
        }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let xavier_bin = env!("CARGO_BIN_EXE_xavier");

    let mut envs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    envs.insert("XAVIER_TOKEN".to_string(), "test_token".to_string());
    envs.insert(
        "XAVIER_DATA_DIR".to_string(),
        tmp_data.path().to_string_lossy().to_string(),
    );
    envs.insert(
        "HOME".to_string(),
        tmp_config.path().to_string_lossy().to_string(),
    );
    envs.insert(
        "XAVIER_CONFIG_PATH".to_string(),
        config_path.to_string_lossy().to_string(),
    );
    envs.insert(
        "XAVIER_RUNTIME_STATE_PATH".to_string(),
        tmp_data.path().to_string_lossy().to_string(),
    );
    envs.insert("XAVIER_HOST".to_string(), "127.0.0.1".to_string());
    envs.insert("XAVIER_PORT".to_string(), addr.port().to_string());

    let url = format!("http://127.0.0.1:{}", addr.port());

    // 1. List without json
    let output_list = Command::new(xavier_bin)
        .envs(&envs)
        .env("XAVIER_URL", &url)
        .env("XAVIER_CONFIG_PATH", &config_path)
        .env("XAVIER_RUNTIME_STATE_PATH", tmp_data.path())
        .arg("provider")
        .arg("list")
        .output()
        .unwrap();

    if output_list.status.code().unwrap() != 0 {
        println!("stdout: {}", String::from_utf8_lossy(&output_list.stdout));
        println!("stderr: {}", String::from_utf8_lossy(&output_list.stderr));
        assert_eq!(output_list.status.code().unwrap(), 0);
    }
    let stdout_list = String::from_utf8(output_list.stdout).unwrap();
    assert!(stdout_list.contains("mock-provider"));

    // 2. List with --json
    let output_list_json = Command::new(xavier_bin)
        .envs(&envs)
        .env("XAVIER_URL", &url)
        .env("XAVIER_CONFIG_PATH", &config_path)
        .env("XAVIER_RUNTIME_STATE_PATH", tmp_data.path())
        .arg("provider")
        .arg("list")
        .arg("--json")
        .output()
        .unwrap();

    assert_eq!(output_list_json.status.code().unwrap(), 0);
    let stdout_str = String::from_utf8(output_list_json.stdout).unwrap();
    let json_val: serde_json::Value =
        serde_json::from_str(&stdout_str).expect("Should be valid JSON");
    assert_eq!(
        json_val["providers"][0]["name"].as_str().unwrap(),
        "mock-provider"
    );

    let requests_after_list = *request_log.all_requests.lock().unwrap();
    assert!(
        requests_after_list >= 2,
        "provider list (plain + --json) must hit the mock, saw {requests_after_list} requests"
    );

    // 3. Set provider
    let output_set = Command::new(xavier_bin)
        .envs(&envs)
        .env("XAVIER_URL", &url)
        .env("XAVIER_CONFIG_PATH", &config_path)
        .env("XAVIER_RUNTIME_STATE_PATH", tmp_data.path())
        .arg("provider")
        .arg("set")
        .arg("test-openai")
        .output()
        .unwrap();

    assert_eq!(output_set.status.code().unwrap(), 0);

    {
        let switches = request_log.switches.lock().unwrap();
        assert_eq!(switches.len(), 1);
        assert_eq!(
            switches[0].get("provider").unwrap().as_str().unwrap(),
            "test-openai"
        );
    }
    let requests_after_set = *request_log.all_requests.lock().unwrap();
    assert!(
        requests_after_set > requests_after_list,
        "provider set must hit the mock"
    );

    // 3b. Set provider errors
    // HTTP 401
    let url_401 = format!("http://127.0.0.1:{}/401", addr.port());
    let output_set_401 = Command::new(xavier_bin)
        .envs(&envs)
        .env("XAVIER_URL", &url_401)
        .env("XAVIER_CONFIG_PATH", &config_path)
        .env("XAVIER_RUNTIME_STATE_PATH", tmp_data.path())
        .arg("provider")
        .arg("set")
        .arg("test-openai")
        .output()
        .unwrap();
    assert_eq!(output_set_401.status.code().unwrap(), 3);

    // HTTP 403
    let url_403 = format!("http://127.0.0.1:{}/403", addr.port());
    let output_set_403 = Command::new(xavier_bin)
        .envs(&envs)
        .env("XAVIER_URL", &url_403)
        .env("XAVIER_CONFIG_PATH", &config_path)
        .env("XAVIER_RUNTIME_STATE_PATH", tmp_data.path())
        .arg("provider")
        .arg("set")
        .arg("test-openai")
        .output()
        .unwrap();
    assert_eq!(output_set_403.status.code().unwrap(), 3);

    // HTTP 404
    let url_404 = format!("http://127.0.0.1:{}/404", addr.port());
    let output_set_404 = Command::new(xavier_bin)
        .envs(&envs)
        .env("XAVIER_URL", &url_404)
        .env("XAVIER_CONFIG_PATH", &config_path)
        .env("XAVIER_RUNTIME_STATE_PATH", tmp_data.path())
        .arg("provider")
        .arg("set")
        .arg("test-openai")
        .output()
        .unwrap();
    assert_eq!(output_set_404.status.code().unwrap(), 1);

    // HTTP 500
    let url_500 = format!("http://127.0.0.1:{}/500", addr.port());
    let output_set_500 = Command::new(xavier_bin)
        .envs(&envs)
        .env("XAVIER_URL", &url_500)
        .env("XAVIER_CONFIG_PATH", &config_path)
        .env("XAVIER_RUNTIME_STATE_PATH", tmp_data.path())
        .arg("provider")
        .arg("set")
        .arg("test-openai")
        .output()
        .unwrap();
    assert_eq!(output_set_500.status.code().unwrap(), 1);

    // Connection refused
    let dummy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dummy_port = dummy_listener.local_addr().unwrap().port();
    drop(dummy_listener);
    let url_closed = format!("http://127.0.0.1:{}", dummy_port);
    let output_set_closed = Command::new(xavier_bin)
        .envs(&envs)
        .env("XAVIER_URL", &url_closed)
        .env("XAVIER_CONFIG_PATH", &config_path)
        .env("XAVIER_RUNTIME_STATE_PATH", tmp_data.path())
        .arg("provider")
        .arg("set")
        .arg("test-openai")
        .output()
        .unwrap();
    assert_eq!(output_set_closed.status.code().unwrap(), 4);

    // 4. Auto strategy should fail non-zero
    let _auto_requests_before = *request_log.all_requests.lock().unwrap();
    let output_auto = Command::new(xavier_bin)
        .envs(&envs)
        .env("XAVIER_URL", &url)
        .env("XAVIER_CONFIG_PATH", &config_path)
        .env("XAVIER_RUNTIME_STATE_PATH", tmp_data.path())
        .arg("provider")
        .arg("auto")
        .arg("test-strategy")
        .output()
        .unwrap();

    assert_ne!(output_auto.status.code().unwrap(), 0);
    let stderr_str = String::from_utf8(output_auto.stderr).unwrap();
    let stdout_auto_str = String::from_utf8(output_auto.stdout).unwrap();
    assert!(
        stderr_str.contains("not available") || stdout_auto_str.contains("not available"),
        "Should print not available error message"
    );
    let auto_requests_after = *request_log.all_requests.lock().unwrap();
    assert_eq!(
        _auto_requests_before, auto_requests_after,
        "Auto should not make any network requests"
    );

    // 5. Missing route check - setup a mock WITHOUT the route to ensure it fails non-zero
    let router_empty = Router::new();
    let listener_empty = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr_empty = listener_empty.local_addr().unwrap();

    let server_task_empty = tokio::spawn(async move {
        axum::serve(listener_empty, router_empty).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let url_empty = format!("http://127.0.0.1:{}", addr_empty.port());

    let output_list_empty = Command::new(xavier_bin)
        .envs(&envs)
        .env("XAVIER_URL", &url_empty)
        .env("XAVIER_CONFIG_PATH", &config_path)
        .env("XAVIER_RUNTIME_STATE_PATH", tmp_data.path())
        .arg("provider")
        .arg("list")
        .output()
        .unwrap();

    assert_ne!(output_list_empty.status.code().unwrap(), 0);

    server_task.abort();
    server_task_empty.abort();
}
