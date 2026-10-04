//! Full-cycle E2E smoke tests for the Clavis API Key Vault.
//!
//! Verifies the complete lifecycle of an API key:
//! 1. Key generation via `keygen` / `keys_generate_with_vault` persists in `HardwareVault`.
//! 2. The key is readable via HTTP `GET /v1/clavis/keys/<name>` (with auth).
//! 3. Key revocation via `revoke_key` / `keys_revoke_with_vault` deletes it.
//! 4. Subsequent HTTP GET returns 404 (`clavis_key_not_found`).
//! 5. Secret values are never printed to stdout/logs in unmasked form.

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

use xavier::adapters::inbound::http::handlers::clavis::clavis_vault;
use xavier::adapters::inbound::http::routes::create_router;
use xavier::clavis;
use xavier::secrets::keygen::{self, KeyScope};
use xavier::secrets::vault::HardwareVault;
use xavier::security::auth::{Claims, UserRole};

/// Helper to build an HTTP request for router execution, with optional auth claims.
fn build_router_request(
    method: Method,
    uri: &str,
    body: Option<&str>,
    role: Option<UserRole>,
) -> Request<Body> {
    let mut builder = Request::builder().uri(uri).method(method);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let mut req = builder
        .body(match body {
            Some(b) => Body::from(b.to_string()),
            None => Body::empty(),
        })
        .expect("failed to build request");

    if let Some(role) = role {
        req.extensions_mut().insert(Claims::new(
            "e2e-admin".to_string(),
            "e2e-admin@example.com".to_string(),
            role,
            chrono::Duration::hours(1),
        ));
    }
    req
}

/// Helper to install an isolated test vault for hermetic execution.
fn setup_isolated_vault() -> (tempfile::TempDir, Arc<HardwareVault>) {
    let temp_dir = tempfile::tempdir().expect("failed to create tempdir");
    let isolated_dir = temp_dir.path().to_path_buf();
    std::env::set_var("HOME", &isolated_dir);
    std::env::set_var("XAVIER_DATA_DIR", &isolated_dir);
    let vault = clavis_vault();
    (temp_dir, vault)
}

#[tokio::test]
#[serial_test::serial]
async fn test_full_cycle_api_key_generate_http_read_revoke_404() {
    let (_temp_dir, vault) = setup_isolated_vault();

    let unique_suffix = ulid::Ulid::new().to_string().to_lowercase();
    let key_name = format!("smoke_key_{}", unique_suffix);

    // Step 1: Generate key using keygen and persist in HardwareVault
    let mut writer_buf = Vec::new();
    let generated = keygen::generate_key(
        vault.as_ref(),
        &key_name,
        KeyScope::Live,
        3600,
        chrono::Utc::now(),
    )
    .expect("key generation failed");

    // Format output line (mimicking `xavier keys generate` CLI output)
    let output_line = keygen::format_generate_output(&generated);
    writer_buf.extend_from_slice(output_line.as_bytes());

    let rendered_output = String::from_utf8(writer_buf).expect("valid utf-8 output");

    // Assert Step 1: Key generated, non-empty, starts with prefix
    assert!(
        generated.value.starts_with("xavier_live_"),
        "Generated key should start with 'xavier_live_', got: {}",
        generated.value
    );

    // Assert Step 4 part A: Output line must NOT contain plaintext key value
    assert!(
        !rendered_output.contains(&generated.value),
        "Generation output line MUST NOT contain the plaintext key! Output was: {}",
        rendered_output
    );
    assert!(
        rendered_output.contains("value=<hidden>"),
        "Output should contain 'value=<hidden>'"
    );
    assert!(
        rendered_output.contains("fingerprint=sha256:"),
        "Output should contain fingerprint"
    );

    // Step 2: Read key via HTTP GET /v1/clavis/keys/<entry_name>
    let vault_entry_name = keygen::value_entry(&key_name); // "api_key_<key_name>"
    let get_uri = format!("/v1/clavis/keys/{}", vault_entry_name);

    let get_req = build_router_request(Method::GET, &get_uri, None, Some(UserRole::Admin));
    let get_resp = create_router()
        .oneshot(get_req)
        .await
        .expect("HTTP GET request failed");

    assert_eq!(
        get_resp.status(),
        StatusCode::OK,
        "HTTP GET /v1/clavis/keys/{} should return 200 OK",
        vault_entry_name
    );

    let body_bytes = get_resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect body")
        .to_bytes();
    let get_json: serde_json::Value =
        serde_json::from_slice(&body_bytes).expect("failed to parse JSON response");

    // Assert Step 2: Key value read via HTTP matches generated value
    assert_eq!(
        get_json["value"], generated.value,
        "HTTP GET response value must match generated key value"
    );

    // Step 3: Revoke key via keygen::revoke_key
    keygen::revoke_key(vault.as_ref(), &key_name).expect("key revocation failed");

    // Assert Step 3: Subsequent HTTP GET returns 404
    let post_revoke_req = build_router_request(Method::GET, &get_uri, None, Some(UserRole::Admin));
    let post_revoke_resp = create_router()
        .oneshot(post_revoke_req)
        .await
        .expect("HTTP GET request failed");

    assert_eq!(
        post_revoke_resp.status(),
        StatusCode::NOT_FOUND,
        "HTTP GET after revocation MUST return 404 Not Found"
    );

    let post_revoke_bytes = post_revoke_resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect body")
        .to_bytes();
    let post_revoke_json: serde_json::Value =
        serde_json::from_slice(&post_revoke_bytes).expect("failed to parse JSON response");

    assert_eq!(
        post_revoke_json["code"], "clavis_key_not_found",
        "Error code must be 'clavis_key_not_found'"
    );

    // Step 4 part B: Masking check
    let log_msg = format!("Key generated: {}", generated.value);
    let masked_log = clavis::mask_log_message(&log_msg);
    assert!(
        !masked_log.contains(&generated.value),
        "Secret value MUST be masked in log messages! Log was: {}",
        masked_log
    );
}

#[tokio::test]
#[serial_test::serial]
async fn test_clavis_http_api_vault_contract() {
    let (_temp_dir, vault) = setup_isolated_vault();
    let _ = vault.delete_secret("api_key_probe");

    // 1. PUT /v1/clavis/keys/api_key_probe -> 201 CREATED
    let put_req = build_router_request(
        Method::PUT,
        "/v1/clavis/keys/api_key_probe",
        Some(r#"{"value":"e2e_test_value_abc123"}"#),
        Some(UserRole::Admin),
    );
    let put_resp = create_router()
        .oneshot(put_req)
        .await
        .expect("PUT request failed");
    assert_eq!(put_resp.status(), StatusCode::CREATED);

    // Verify persisted directly in the underlying hardware vault
    assert_eq!(
        vault.get_secret("api_key_probe").unwrap(),
        "e2e_test_value_abc123"
    );

    // 2. GET /v1/clavis/keys/api_key_probe (authenticated) -> 200 OK
    let get_req = build_router_request(
        Method::GET,
        "/v1/clavis/keys/api_key_probe",
        None,
        Some(UserRole::Admin),
    );
    let get_resp = create_router()
        .oneshot(get_req)
        .await
        .expect("GET request failed");
    assert_eq!(get_resp.status(), StatusCode::OK);

    let body_bytes = get_resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let get_json: serde_json::Value = serde_json::from_slice(&body_bytes).expect("parse JSON");
    assert_eq!(get_json["value"], "e2e_test_value_abc123");

    // 3. GET /v1/clavis/keys/api_key_ghost -> 404 NOT FOUND with code "clavis_key_not_found"
    let ghost_req = build_router_request(
        Method::GET,
        "/v1/clavis/keys/api_key_ghost",
        None,
        Some(UserRole::Admin),
    );
    let ghost_resp = create_router()
        .oneshot(ghost_req)
        .await
        .expect("GET ghost failed");
    assert_eq!(ghost_resp.status(), StatusCode::NOT_FOUND);

    let ghost_bytes = ghost_resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let ghost_json: serde_json::Value = serde_json::from_slice(&ghost_bytes).expect("parse JSON");
    assert_eq!(ghost_json["code"], "clavis_key_not_found");

    // 4. GET /v1/clavis/keys/api_key_probe (unauthenticated / missing token) -> 403 FORBIDDEN
    let unauth_req = build_router_request(Method::GET, "/v1/clavis/keys/api_key_probe", None, None);
    let unauth_resp = create_router()
        .oneshot(unauth_req)
        .await
        .expect("GET unauth failed");
    assert_eq!(unauth_resp.status(), StatusCode::FORBIDDEN);

    let _ = vault.delete_secret("api_key_probe");
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
