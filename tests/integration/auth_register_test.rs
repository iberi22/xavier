use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
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

async fn start_test_server(state_dir_path: &std::path::Path) -> (u16, ChildGuard) {
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port();

    let child = ChildGuard(
        std::process::Command::new(env!("CARGO_BIN_EXE_xavier"))
            .env("XAVIER_HOST", "127.0.0.1")
            .env("XAVIER_PORT", port.to_string())
            .env("XAVIER_TOKEN", "test-token")
            .env("XAVIER_HEADLESS", "true")
            .env("XAVIER_STATE_DIR", state_dir_path.to_str().unwrap())
            .env(
                "XAVIER_JWT_SECRET",
                "super-secret-jwt-key-2026-very-secure-indeed",
            )
            .env(
                "XAVIER_CODE_GRAPH_DB_PATH",
                state_dir_path.join(format!("test-code-{port}.db")),
            )
            .env(
                "XAVIER_MEMORY_VEC_PATH",
                state_dir_path.join(format!("test-mem-{port}.db")),
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

    (port, child)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_auth_register_flow() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let (port, _server) = start_test_server(temp_dir.path()).await;
    let client = Client::new();
    let register_url = format!("http://127.0.0.1:{port}/auth/register");

    // 1. POST /auth/register with valid email+name+password -> 200/201 + seed_phrase returned and password_hash NOT in response
    let valid_payload = json!({
        "email": "test-user-register@example.com",
        "name": "Operator Register Test",
        "password": "securepassword123"
    });

    let resp = client
        .post(&register_url)
        .json(&valid_payload)
        .send()
        .await
        .expect("failed to send register request");

    let status = resp.status();
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "expected 201 Created or 200 OK for registration, got: {}",
        status
    );

    let body: Value = resp
        .json()
        .await
        .expect("failed to parse registration response JSON");

    // Check fields in the top-level structure
    assert!(
        body.get("seed_phrase").is_some(),
        "seed_phrase must be returned"
    );
    assert!(
        body["seed_phrase"].is_string(),
        "seed_phrase must be a string"
    );

    let seed_phrase = body["seed_phrase"].as_str().unwrap();
    let words_count = seed_phrase.split_whitespace().count();
    assert!(words_count > 0, "seed_phrase should contain words");

    // Check user object in the response
    let user_val = &body["user"];
    assert!(user_val.is_object(), "user field should be an object");
    assert_eq!(user_val["email"], "test-user-register@example.com");
    assert_eq!(user_val["name"], "Operator Register Test");
    assert!(user_val.get("id").is_some(), "user id must be present");

    // CRITICAL: Ensure password_hash is NOT leaked
    assert!(
        body.get("password_hash").is_none(),
        "password_hash must NOT be in root response"
    );
    assert!(
        user_val.get("password_hash").is_none(),
        "password_hash must NOT be in user object"
    );

    // Also verify other sensitive credentials are NOT leaked
    assert!(
        user_val.get("totp_secret").is_none(),
        "totp_secret must NOT be leaked"
    );
    assert!(
        user_val.get("recovery_seed_hash").is_none(),
        "recovery_seed_hash must NOT be leaked"
    );
    assert!(
        user_val.get("backup_codes").is_none(),
        "backup_codes must NOT be leaked"
    );

    // 2. POST /auth/register with duplicate email -> 409 Conflict
    let duplicate_payload = json!({
        "email": "test-user-register@example.com",
        "name": "Duplicate Operator",
        "password": "anotherpassword"
    });

    let resp_dup = client
        .post(&register_url)
        .json(&duplicate_payload)
        .send()
        .await
        .expect("failed to send duplicate register request");

    assert_eq!(
        resp_dup.status(),
        StatusCode::CONFLICT,
        "expected 409 Conflict for duplicate email"
    );

    // 3. POST /auth/register with missing fields -> 4xx validation error
    let invalid_payloads = vec![
        json!({
            "email": "invalid-no-password@example.com",
            "name": "No Password User"
        }),
        json!({
            "name": "No Email User",
            "password": "password"
        }),
        json!({
            "email": "invalid-no-name@example.com",
            "password": "password"
        }),
        json!({}),
    ];

    for payload in invalid_payloads {
        let resp_invalid = client
            .post(&register_url)
            .json(&payload)
            .send()
            .await
            .expect("failed to send invalid payload register request");

        assert!(
            resp_invalid.status().is_client_error(),
            "expected 4xx client error for missing fields, got: {}",
            resp_invalid.status()
        );
    }
}
