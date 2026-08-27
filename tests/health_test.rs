use reqwest::{Client, StatusCode};
use serde_json::Value;
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
async fn test_health_endpoints_e2e() {
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port();

    let code_db = format!("data/health-test-code-{port}.db");
    let mem_db = format!("data/health-test-mem-{port}.db");

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

    let mode = body_v1["mode"].as_str().expect("mode should be a string");
    println!("Retrieved mode: {}", mode);

    let llm_reachable = body_v1["llm"]["reachable"]
        .as_bool()
        .expect("llm.reachable should be a boolean");
    println!("Retrieved llm.reachable: {}", llm_reachable);

    let embeddings_model = body_v1["embeddings"]["model"]
        .as_str()
        .expect("embeddings.model should be a string");
    assert!(
        !embeddings_model.is_empty(),
        "embeddings.model should not be empty"
    );
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

    // Maturity JSON shape checks
    let mesh_maturity = &body_v1["mesh"]["maturity"];
    assert_eq!(mesh_maturity["http_transport"].as_bool(), Some(true));
    assert_eq!(mesh_maturity["http_transport_percent"].as_u64(), Some(100));
    assert_eq!(mesh_maturity["libp2p"].as_bool(), Some(false));
    assert_eq!(mesh_maturity["libp2p_percent"].as_u64(), Some(10));
    assert_eq!(mesh_maturity["acl"].as_bool(), Some(true));
    assert_eq!(mesh_maturity["acl_percent"].as_u64(), Some(90));
    assert_eq!(mesh_maturity["tokenomics"].as_bool(), Some(true));
    assert_eq!(mesh_maturity["tokenomics_percent"].as_u64(), Some(40));
    assert_eq!(mesh_maturity["onchain_gov"].as_bool(), Some(false));
    assert_eq!(mesh_maturity["onchain_gov_percent"].as_u64(), Some(0));

    // 3. GET /v1/mesh/status (without accepted license) -> should return 403 Forbidden
    let resp_forbidden = client
        .get(format!("http://127.0.0.1:{port}/v1/mesh/status"))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp_forbidden.status(), StatusCode::FORBIDDEN);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_mesh_status_with_license_e2e() {
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port();

    // Create a temporary config file with mesh_accepted = true
    let temp_config_path = std::env::temp_dir().join(format!("xavier-test-config-{port}.json"));
    let config_json = serde_json::json!({
        "license": {
            "mesh_accepted": true,
            "license_type": "Xavier-Mesh-1.0"
        }
    });
    std::fs::write(&temp_config_path, config_json.to_string()).unwrap();

    let code_db = format!("data/health-test-code-{port}.db");
    let mem_db = format!("data/health-test-mem-{port}.db");

    let _child = ChildGuard {
        child: std::process::Command::new(env!("CARGO_BIN_EXE_xavier"))
            .env("XAVIER_HOST", "127.0.0.1")
            .env("XAVIER_PORT", port.to_string())
            .env("XAVIER_TOKEN", "test-token")
            .env("XAVIER_HEADLESS", "true")
            .env("XAVIER_CONFIG_PATH", temp_config_path.to_str().unwrap())
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
    assert!(started, "Server failed to start");

    // GET /v1/mesh/status (with accepted license)
    let resp = client
        .get(format!("http://127.0.0.1:{port}/v1/mesh/status"))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert_eq!(body["http_transport"].as_bool(), Some(true));
    assert_eq!(body["http_transport_percent"].as_u64(), Some(100));
    assert_eq!(body["libp2p"].as_bool(), Some(false));
    assert_eq!(body["libp2p_percent"].as_u64(), Some(10));
    assert_eq!(body["acl"].as_bool(), Some(true));
    assert_eq!(body["acl_percent"].as_u64(), Some(90));
    assert_eq!(body["tokenomics"].as_bool(), Some(true));
    assert_eq!(body["tokenomics_percent"].as_u64(), Some(40));
    assert_eq!(body["onchain_gov"].as_bool(), Some(false));
    assert_eq!(body["onchain_gov_percent"].as_u64(), Some(0));

    // Cleanup
    let _ = std::fs::remove_file(temp_config_path);
}
