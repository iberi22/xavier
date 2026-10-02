use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use tower::util::ServiceExt;

use xavier::adapters::inbound::http::routes::create_router;
use xavier::clavis::mask_key;
use xavier::security::auth::{Claims, UserRole};

/// Helper to construct a request to the Clavis routes with or without auth claims.
fn build_clavis_request(
    method: Method,
    uri: &str,
    body: Option<&str>,
    role: Option<UserRole>,
) -> Request<Body> {
    let mut builder = Request::builder().uri(uri).method(method);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    if role.is_some() {
        // Send simulated XAVIER_TOKEN or auth token header for logging/verification tracking
        builder = builder.header("XAVIER_TOKEN", "mock-xavier-auth-token-12345");
    }
    let mut req = builder
        .body(match body {
            Some(b) => Body::from(b.to_string()),
            None => Body::empty(),
        })
        .expect("build request");

    if let Some(role) = role {
        req.extensions_mut().insert(Claims::new(
            "test_admin".to_string(),
            "admin@swal.dev".to_string(),
            role,
            chrono::Duration::hours(1),
        ));
    }
    req
}

/// Sets up isolated temporary directories for `HOME` and `XAVIER_DATA_DIR` so that
/// integration tests never touch the developer's real `~/.xavier` vault files or keyring.
/// Note: HardwareVault::isolated is #[cfg(test)] gated in src/secrets/vault.rs and thus not
/// exported across the library boundary to tests/*. We achieve isolated storage by isolating env paths.
fn setup_isolated_env() -> tempfile::TempDir {
    let tmp_dir = tempfile::tempdir().expect("create tempdir");
    let isolated_dir = tmp_dir.path().to_path_buf();
    std::env::set_var("HOME", &isolated_dir);
    std::env::set_var("XAVIER_DATA_DIR", &isolated_dir);
    tmp_dir
}

#[tokio::test]
#[serial_test::serial]
async fn test_put_then_get_round_trip() {
    let _tmp = setup_isolated_env();
    let secret_key = "openai_api_key";
    let secret_value = "sk-proj-e2e-test-secret-value-98765";

    // 1. PUT /v1/clavis/keys/openai_api_key -> HTTP 201 CREATED
    let put_req = build_clavis_request(
        Method::PUT,
        &format!("/v1/clavis/keys/{}", secret_key),
        Some(&format!(r#"{{"value":"{}"}}"#, secret_value)),
        Some(UserRole::Admin),
    );

    let put_resp = create_router()
        .oneshot(put_req)
        .await
        .expect("PUT request failed");

    assert_eq!(
        put_resp.status(),
        StatusCode::CREATED,
        "Expected HTTP 201 CREATED for new key"
    );

    let put_body = put_resp.into_body().collect().await.unwrap().to_bytes();
    let put_json: serde_json::Value = serde_json::from_slice(&put_body).expect("valid JSON");
    assert_eq!(put_json["status"], "ok");
    assert_eq!(put_json["created"], true);
    // Raw value must NOT be echoed in the put response
    assert!(!String::from_utf8_lossy(&put_body).contains(secret_value));

    // 2. GET /v1/clavis/keys/openai_api_key -> HTTP 200 OK
    let get_req = build_clavis_request(
        Method::GET,
        &format!("/v1/clavis/keys/{}", secret_key),
        None,
        Some(UserRole::Admin),
    );

    let get_resp = create_router()
        .oneshot(get_req)
        .await
        .expect("GET request failed");

    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = get_resp.into_body().collect().await.unwrap().to_bytes();
    let get_json: serde_json::Value = serde_json::from_slice(&get_body).expect("valid JSON");
    assert_eq!(get_json["value"], secret_value);
}

#[tokio::test]
#[serial_test::serial]
async fn test_get_nonexistent_key_returns_404() {
    let _tmp = setup_isolated_env();

    let req = build_clavis_request(
        Method::GET,
        "/v1/clavis/keys/nonexistent_key_abc",
        None,
        Some(UserRole::Admin),
    );

    let resp = create_router()
        .oneshot(req)
        .await
        .expect("GET request failed");

    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "Missing key must return HTTP 404 NOT_FOUND"
    );

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed: serde_json::Value = serde_json::from_slice(&body).expect("valid JSON");
    assert_eq!(parsed["status"], "error");
    assert_eq!(parsed["code"], "clavis_key_not_found");
    assert!(
        parsed.get("error").is_some() || parsed.get("message").is_some(),
        "Response must carry error or message for Dart client contract"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn test_unauthenticated_and_unauthorized_access_rejected() {
    let _tmp = setup_isolated_env();

    // 1. Missing claims/token -> HTTP 403 (or 401) FORBIDDEN
    let anon_req = build_clavis_request(
        Method::GET,
        "/v1/clavis/keys/protected_key",
        None,
        None,
    );

    let anon_resp = create_router()
        .oneshot(anon_req)
        .await
        .expect("Request failed");

    assert!(
        anon_resp.status() == StatusCode::FORBIDDEN || anon_resp.status() == StatusCode::UNAUTHORIZED,
        "Unauthenticated access must be rejected with 401 or 403, got {}",
        anon_resp.status()
    );

    // 2. UserRole::User (non-admin) -> HTTP 403 FORBIDDEN
    let user_req = build_clavis_request(
        Method::GET,
        "/v1/clavis/keys/protected_key",
        None,
        Some(UserRole::User),
    );

    let user_resp = create_router()
        .oneshot(user_req)
        .await
        .expect("Request failed");

    assert_eq!(
        user_resp.status(),
        StatusCode::FORBIDDEN,
        "Non-admin user role must be rejected with HTTP 403 FORBIDDEN"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn test_put_empty_value_returns_400() {
    let _tmp = setup_isolated_env();

    let req = build_clavis_request(
        Method::PUT,
        "/v1/clavis/keys/empty_val_key",
        Some(r#"{"value":""}"#),
        Some(UserRole::Admin),
    );

    let resp = create_router()
        .oneshot(req)
        .await
        .expect("PUT request failed");

    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "PUT with empty value must return HTTP 400 BAD_REQUEST"
    );

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed: serde_json::Value = serde_json::from_slice(&body).expect("valid JSON");
    assert_eq!(parsed["status"], "error");
    assert!(parsed["message"].as_str().unwrap().contains("value must not be empty"));
}

#[tokio::test]
#[serial_test::serial]
async fn test_log_safety_and_vault_isolation() {
    let tmp = setup_isolated_env();
    let secret_key = "log_safety_key";
    let raw_secret = "sk-live-super-secret-token-abcdef123456";

    // Store via HTTP endpoint
    let put_req = build_clavis_request(
        Method::PUT,
        &format!("/v1/clavis/keys/{}", secret_key),
        Some(&format!(r#"{{"value":"{}"}}"#, raw_secret)),
        Some(UserRole::Admin),
    );

    let put_resp = create_router()
        .oneshot(put_req)
        .await
        .expect("PUT request failed");

    assert_eq!(put_resp.status(), StatusCode::CREATED);

    // 1. Masking verification: masked message hides raw secret
    let masked_log = xavier::clavis::mask_log_message(&format!("Connecting with {}", raw_secret));
    assert!(
        !masked_log.contains(raw_secret),
        "Secret leaked verbatim into masked log message"
    );
    assert!(
        masked_log.contains(&mask_key(raw_secret)),
        "Masked log message must contain masked preview"
    );

    // 2. Vault file on disk check in isolated directory
    let secrets_dir = tmp.path().join(".xavier").join("secrets");
    if secrets_dir.exists() {
        let enc_file = secrets_dir.join(format!("{}.enc", secret_key));
        if enc_file.exists() {
            let file_bytes = std::fs::read(&enc_file).expect("Read encrypted file");
            let file_string = String::from_utf8_lossy(&file_bytes);
            assert!(
                !file_string.contains(raw_secret),
                "Secret found in plain text inside the isolated vault file on disk!"
            );
        }
    }

    // 3. GET endpoint successfully retrieves raw secret
    let get_req = build_clavis_request(
        Method::GET,
        &format!("/v1/clavis/keys/{}", secret_key),
        None,
        Some(UserRole::Admin),
    );

    let get_resp = create_router()
        .oneshot(get_req)
        .await
        .expect("GET request failed");

    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = get_resp.into_body().collect().await.unwrap().to_bytes();
    let get_json: serde_json::Value = serde_json::from_slice(&get_body).expect("valid JSON");
    assert_eq!(get_json["value"], raw_secret);
}
