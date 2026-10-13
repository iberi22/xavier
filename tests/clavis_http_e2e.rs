use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use tower::util::ServiceExt;

use xavier::adapters::inbound::http::routes::create_router;
use xavier::clavis::mask_key;
use xavier::secrets::vault::HardwareVault;
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

/// Installs an isolated vault (keyring stubbed via the `test-utils` feature) and
/// redirects HOME/XDG dirs into a tempdir; restores the environment on drop.
struct VaultGuard {
    vault: std::sync::Arc<HardwareVault>,
    #[allow(dead_code)]
    tmp_dir: tempfile::TempDir,
    original_home: Option<std::ffi::OsString>,
    original_data: Option<std::ffi::OsString>,
    original_xdg: Option<std::ffi::OsString>,
    original_dbus: Option<std::ffi::OsString>,
}

impl VaultGuard {
    fn new() -> Self {
        let tmp_dir = tempfile::tempdir().expect("create tempdir");
        let isolated_dir = tmp_dir.path().to_path_buf();

        let original_home = std::env::var_os("HOME");
        let original_data = std::env::var_os("XAVIER_DATA_DIR");
        let original_xdg = std::env::var_os("XDG_RUNTIME_DIR");
        let original_dbus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");

        std::env::set_var("HOME", &isolated_dir);
        std::env::set_var("XAVIER_DATA_DIR", &isolated_dir);
        let xdg_dir = tmp_dir.path().join("xdg");
        std::fs::create_dir_all(&xdg_dir).unwrap();
        std::env::set_var("XDG_RUNTIME_DIR", &xdg_dir);
        std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");

        let vault =
            HardwareVault::new("xavier-clavis").isolated(tmp_dir.path().to_path_buf(), [0u8; 32]);
        let arc_vault = std::sync::Arc::new(vault);
        xavier::adapters::inbound::http::handlers::clavis::set_clavis_vault(arc_vault.clone());

        Self {
            vault: arc_vault,
            tmp_dir,
            original_home,
            original_data,
            original_xdg,
            original_dbus,
        }
    }
}

impl Drop for VaultGuard {
    fn drop(&mut self) {
        if let Some(ref home) = self.original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(ref data) = self.original_data {
            std::env::set_var("XAVIER_DATA_DIR", data);
        } else {
            std::env::remove_var("XAVIER_DATA_DIR");
        }

        if let Some(ref xdg) = self.original_xdg {
            std::env::set_var("XDG_RUNTIME_DIR", xdg);
        } else {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }

        if let Some(ref dbus) = self.original_dbus {
            std::env::set_var("DBUS_SESSION_BUS_ADDRESS", dbus);
        } else {
            std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
        }

        // Ensure this drop guard refuses to work on a non-stubbed vault to prevent deleting real keyring entries
        assert_eq!(
            self.vault.keyring_stub_ok(),
            Some(false),
            "VaultGuard must only be used with a fully stubbed isolated vault"
        );

        // Reset the vault override to default (non-isolated) to prevent state bleeding across tests
        let fallback = HardwareVault::new("xavier-clavis");
        xavier::adapters::inbound::http::handlers::clavis::set_clavis_vault(std::sync::Arc::new(
            fallback,
        ));
    }
}

#[tokio::test]
#[serial_test::serial]
async fn test_put_then_get_round_trip() {
    let _tmp = VaultGuard::new();
    let secret_key = "api_key_openai";
    let _ = HardwareVault::new("xavier-clavis").delete_secret(secret_key);
    let secret_value = "sk-proj-e2e-test-secret-value-98765";

    // 1. PUT /v1/clavis/keys/api_key_openai -> HTTP 201 CREATED
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

    // 2. GET /v1/clavis/keys/api_key_openai -> HTTP 200 OK
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

    let _ = HardwareVault::new("xavier-clavis").delete_secret(secret_key);
}

#[tokio::test]
#[serial_test::serial]
async fn test_get_nonexistent_key_returns_404() {
    let _tmp = VaultGuard::new();

    let req = build_clavis_request(
        Method::GET,
        "/v1/clavis/keys/api_key_nonexistent_abc",
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
    let _tmp = VaultGuard::new();

    // 1. Missing claims/token -> HTTP 403 (or 401) FORBIDDEN
    let anon_req =
        build_clavis_request(Method::GET, "/v1/clavis/keys/api_key_protected", None, None);

    let anon_resp = create_router()
        .oneshot(anon_req)
        .await
        .expect("Request failed");

    assert!(
        anon_resp.status() == StatusCode::FORBIDDEN
            || anon_resp.status() == StatusCode::UNAUTHORIZED,
        "Unauthenticated access must be rejected with 401 or 403, got {}",
        anon_resp.status()
    );

    // 2. UserRole::User (non-admin) -> HTTP 403 FORBIDDEN
    let user_req = build_clavis_request(
        Method::GET,
        "/v1/clavis/keys/api_key_protected",
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
    let _tmp = VaultGuard::new();

    let req = build_clavis_request(
        Method::PUT,
        "/v1/clavis/keys/api_key_empty_val",
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
    assert!(parsed["message"]
        .as_str()
        .unwrap()
        .contains("value must not be empty"));
}

#[tokio::test]
#[serial_test::serial]
async fn test_log_safety_and_vault_isolation() {
    let tmp_guard = VaultGuard::new();
    let tmp = &tmp_guard.tmp_dir;
    let secret_key = "api_key_log_safety";
    let _ = HardwareVault::new("xavier-clavis").delete_secret(secret_key);
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
    let secrets_dir = tmp
        .path()
        .join(".xavier")
        .join("vaults")
        .join("xavier-clavis");
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

    let _ = HardwareVault::new("xavier-clavis").delete_secret(secret_key);
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
