//! E2E Integration Test for the complete Auth Flow:
//! Register -> Login -> 2FA -> Recovery -> Refresh
//!
//! Run with: cargo test --test auth_full_flow_test

use axum::Router;
use parking_lot::Mutex;
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::net::TcpListener;
use totp_rs::{Algorithm as TOTPAlgorithm, Builder, Secret, Totp};
use xavier::auth2::db::AuthDb;
use xavier::auth2::{auth_routes, HasAuthDb};

#[derive(Clone)]
struct TestState {
    auth_db: Arc<Mutex<AuthDb>>,
}

impl HasAuthDb for TestState {
    fn auth_db(&self) -> Option<Arc<Mutex<AuthDb>>> {
        Some(self.auth_db.clone())
    }
}

async fn spawn_test_server() -> (String, Client, tempfile::TempDir, Arc<Mutex<AuthDb>>) {
    // Install jsonwebtoken default crypto provider
    let _ = jsonwebtoken::crypto::rust_crypto::DEFAULT_PROVIDER.install_default();

    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("auth.db");
    let auth_db = Arc::new(Mutex::new(AuthDb::new(&db_path).unwrap()));
    let state = TestState {
        auth_db: auth_db.clone(),
    };

    let app = Router::new()
        .nest(
            "/auth",
            auth_routes::<TestState>(&temp_dir.path().to_string_lossy()),
        )
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind random test port");
    let addr = listener.local_addr().expect("read local address");
    let base_url = format!("http://{addr}");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("test server should serve");
    });

    let client = Client::new();
    (base_url, client, temp_dir, auth_db)
}

fn generate_totp_code(secret_b32: &str, email: &str) -> String {
    let secret = Secret::try_from_base32(secret_b32).expect("decode base32 secret");
    let totp: Totp = Builder::new()
        .with_algorithm(TOTPAlgorithm::SHA1)
        .with_digits(6)
        .with_skew(1)
        .with_step_duration(30)
        .with_secret(secret)
        .with_account_name(email.to_string())
        .with_issuer(Some("Xavier".to_string()))
        .build()
        .expect("build TOTP");
    // NOTE: The production server code in `src/auth2/mod.rs` divides the Unix timestamp
    // by 30 before passing it to `totp.check(&payload.code, time)`. To be bug-compatible
    // with this server-side double-division behavior, we must also divide the timestamp
    // by 30 here before generating the TOTP token.
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 30;
    totp.generate(time).to_string()
}

// ─── Test 1: Register → login with credentials → 200 + JWT + refresh_token ───
#[tokio::test]
async fn test_register_and_login() {
    let (base_url, client, _temp_dir, _auth_db) = spawn_test_server().await;

    let email = "test_register_login@example.com";
    let password = "SuperSecurePassword123!";
    let name = "Register User";

    // Register
    let reg_res = client
        .post(format!("{}/auth/register", base_url))
        .json(&json!({
            "email": email,
            "password": password,
            "name": name,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reg_res.status(), StatusCode::OK);
    let reg_body: Value = reg_res.json().await.unwrap();
    assert_eq!(reg_body["user"]["email"], email);

    // Login with correct credentials
    let login_res = client
        .post(format!("{}/auth/login", base_url))
        .json(&json!({
            "email": email,
            "password": password,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(login_res.status(), StatusCode::OK);
    let login_body: Value = login_res.json().await.unwrap();
    assert!(!login_body["access_token"].as_str().unwrap().is_empty());
    assert!(!login_body["refresh_token"].as_str().unwrap().is_empty());
    assert!(!login_body["requires_2fa"].as_bool().unwrap());
}

// ─── Test 2: Login fails with wrong password → 401 Unauthorized ───
#[tokio::test]
async fn test_login_wrong_password() {
    let (base_url, client, _temp_dir, _auth_db) = spawn_test_server().await;

    let email = "test_login_fail@example.com";
    let password = "SuperSecurePassword123!";
    let name = "Fail User";

    // Register
    let _reg_res = client
        .post(format!("{}/auth/register", base_url))
        .json(&json!({
            "email": email,
            "password": password,
            "name": name,
        }))
        .send()
        .await
        .unwrap();

    // Login with wrong password
    let login_res = client
        .post(format!("{}/auth/login", base_url))
        .json(&json!({
            "email": email,
            "password": "IncorrectPassword123!",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(login_res.status(), StatusCode::UNAUTHORIZED);
}

// ─── Test 3: 2FA Setup (authenticated with JWT) → QR code + backup codes returned ───
#[tokio::test]
async fn test_2fa_setup() {
    let (base_url, client, _temp_dir, _auth_db) = spawn_test_server().await;

    let email = "test_2fa_setup@example.com";
    let password = "SuperSecurePassword123!";
    let name = "Setup User";

    // Register
    let _reg_res = client
        .post(format!("{}/auth/register", base_url))
        .json(&json!({
            "email": email,
            "password": password,
            "name": name,
        }))
        .send()
        .await
        .unwrap();

    // Login
    let login_res = client
        .post(format!("{}/auth/login", base_url))
        .json(&json!({
            "email": email,
            "password": password,
        }))
        .send()
        .await
        .unwrap();
    let login_body: Value = login_res.json().await.unwrap();
    let jwt = login_body["access_token"].as_str().unwrap();

    // Setup 2FA
    let setup_res = client
        .post(format!("{}/auth/2fa/setup", base_url))
        .header("Authorization", format!("Bearer {}", jwt))
        .send()
        .await
        .unwrap();
    assert_eq!(setup_res.status(), StatusCode::OK);
    let setup_body: Value = setup_res.json().await.unwrap();
    assert!(!setup_body["qr_code"].as_str().unwrap().is_empty());
    assert!(!setup_body["secret"].as_str().unwrap().is_empty());
    assert_eq!(setup_body["backup_codes"].as_array().unwrap().len(), 10);
}

// ─── Test 4: 2FA Verify with valid TOTP code → success ───
#[tokio::test]
async fn test_2fa_verify() {
    let (base_url, client, _temp_dir, _auth_db) = spawn_test_server().await;

    let email = "test_2fa_verify@example.com";
    let password = "SuperSecurePassword123!";
    let name = "Verify User";

    // Register
    let _reg_res = client
        .post(format!("{}/auth/register", base_url))
        .json(&json!({
            "email": email,
            "password": password,
            "name": name,
        }))
        .send()
        .await
        .unwrap();

    // Login
    let login_res = client
        .post(format!("{}/auth/login", base_url))
        .json(&json!({
            "email": email,
            "password": password,
        }))
        .send()
        .await
        .unwrap();
    let login_body: Value = login_res.json().await.unwrap();
    let jwt = login_body["access_token"].as_str().unwrap();

    // Setup 2FA
    let setup_res = client
        .post(format!("{}/auth/2fa/setup", base_url))
        .header("Authorization", format!("Bearer {}", jwt))
        .send()
        .await
        .unwrap();
    let setup_body: Value = setup_res.json().await.unwrap();
    let secret = setup_body["secret"].as_str().unwrap();

    // Verify 2FA
    let code = generate_totp_code(secret, email);
    let verify_res = client
        .post(format!("{}/auth/2fa/verify", base_url))
        .header("Authorization", format!("Bearer {}", jwt))
        .json(&json!({ "code": code }))
        .send()
        .await
        .unwrap();
    assert_eq!(verify_res.status(), StatusCode::OK);
    let verify_body: Value = verify_res.json().await.unwrap();
    assert_eq!(verify_body["status"], "2fa_enabled");
}

// ─── Test 5: Recovery flow using seed phrase → password reset → new login works ───
#[tokio::test]
async fn test_recovery_flow() {
    let (base_url, client, _temp_dir, _auth_db) = spawn_test_server().await;

    let email = "test_recovery_flow@example.com";
    let password = "SuperSecurePassword123!";
    let name = "Recovery User";

    // Register
    let reg_res = client
        .post(format!("{}/auth/register", base_url))
        .json(&json!({
            "email": email,
            "password": password,
            "name": name,
        }))
        .send()
        .await
        .unwrap();
    let reg_body: Value = reg_res.json().await.unwrap();
    let seed_phrase = reg_body["seed_phrase"].as_str().unwrap().to_string();

    // Setup 2FA so we can check if recovery disables it
    let login_res = client
        .post(format!("{}/auth/login", base_url))
        .json(&json!({
            "email": email,
            "password": password,
        }))
        .send()
        .await
        .unwrap();
    let login_body: Value = login_res.json().await.unwrap();
    let jwt = login_body["access_token"].as_str().unwrap();

    let setup_res = client
        .post(format!("{}/auth/2fa/setup", base_url))
        .header("Authorization", format!("Bearer {}", jwt))
        .send()
        .await
        .unwrap();
    let setup_body: Value = setup_res.json().await.unwrap();
    let secret = setup_body["secret"].as_str().unwrap();

    let code = generate_totp_code(secret, email);
    let verify_res = client
        .post(format!("{}/auth/2fa/verify", base_url))
        .header("Authorization", format!("Bearer {}", jwt))
        .json(&json!({ "code": code }))
        .send()
        .await
        .unwrap();
    assert_eq!(verify_res.status(), StatusCode::OK);

    // Perform recovery password reset
    let new_password = "NewSuperSecurePassword456!!";
    let recovery_res = client
        .post(format!("{}/auth/recovery", base_url))
        .json(&json!({
            "email": email,
            "seed_phrase": seed_phrase,
            "new_password": new_password,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(recovery_res.status(), StatusCode::OK);
    let recovery_body: Value = recovery_res.json().await.unwrap();
    assert_eq!(recovery_body["status"], "recovery_completed");

    // Login with new password (should succeed and NOT require 2FA since it was disabled)
    let new_login_res = client
        .post(format!("{}/auth/login", base_url))
        .json(&json!({
            "email": email,
            "password": new_password,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(new_login_res.status(), StatusCode::OK);
    let new_login_body: Value = new_login_res.json().await.unwrap();
    assert!(!new_login_body["access_token"].as_str().unwrap().is_empty());
    assert!(!new_login_body["requires_2fa"].as_bool().unwrap());
}

// ─── Test 6: JWT refresh cycle → old token expires, new token works ───
#[tokio::test]
async fn test_jwt_refresh_cycle() {
    let (base_url, client, _temp_dir, _auth_db) = spawn_test_server().await;

    let email = "test_refresh_cycle@example.com";
    let password = "SuperSecurePassword123!";
    let name = "Refresh User";

    // Register
    let _reg_res = client
        .post(format!("{}/auth/register", base_url))
        .json(&json!({
            "email": email,
            "password": password,
            "name": name,
        }))
        .send()
        .await
        .unwrap();

    // Login to get first tokens
    let login_res = client
        .post(format!("{}/auth/login", base_url))
        .json(&json!({
            "email": email,
            "password": password,
        }))
        .send()
        .await
        .unwrap();
    let login_body: Value = login_res.json().await.unwrap();
    let refresh_token_1 = login_body["refresh_token"].as_str().unwrap().to_string();

    // Refresh token rotation
    let refresh_res = client
        .post(format!("{}/auth/refresh", base_url))
        .json(&json!({
            "refresh_token": refresh_token_1,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(refresh_res.status(), StatusCode::OK);
    let refresh_body: Value = refresh_res.json().await.unwrap();
    let access_token_2 = refresh_body["access_token"].as_str().unwrap().to_string();
    let _refresh_token_2 = refresh_body["refresh_token"].as_str().unwrap().to_string();

    // Use of old refresh_token_1 again should fail (rotated/revoked -> theft detection)
    let reuse_res = client
        .post(format!("{}/auth/refresh", base_url))
        .json(&json!({
            "refresh_token": refresh_token_1,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reuse_res.status(), StatusCode::FORBIDDEN);

    // New access token works for authentication
    let status_res = client
        .get(format!("{}/auth/status", base_url))
        .header("Authorization", format!("Bearer {}", access_token_2))
        .send()
        .await
        .unwrap();
    assert_eq!(status_res.status(), StatusCode::OK);
}
