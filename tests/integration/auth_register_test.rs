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
            // auth2's JWT keypair / DB master key live in a HardwareVault keyed by service
            // name (default "xavier-auth"), which is NOT scoped by XAVIER_STATE_DIR — it
            // falls back to `$HOME/.xavier/secrets` when the OS keyring has no entry. Without
            // this override, this test process would read/write whatever machine it runs on's
            // real xavier-auth secrets. See auth2::auth_vault_service_name (src/auth2/mod.rs).
            .env(
                "XAVIER_AUTH_VAULT_SERVICE",
                format!("xavier-auth-test-{port}"),
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

/// Regression test for a real bug found while wiring up the panel's 2FA UI:
/// `setup_2fa_handler`/`verify_2fa_handler` used to operate on `list_users().next()`
/// (whichever account happens to be first in the DB — typically the very first one
/// ever registered) instead of the account identified by the caller's own JWT
/// (`claims.sub`). Any authenticated user could therefore re-enroll or overwrite
/// the FIRST user's TOTP secret/backup codes just by calling `/auth/2fa/setup` or
/// `/auth/2fa/verify` with their own valid token.
///
/// This registers two accounts (A first, B second), has B set up and verify its
/// OWN 2FA, and asserts that A is completely unaffected: A can still log in with
/// just its password (2FA was never toggled on for A), and B's 2FA — and only
/// B's — is enabled and enforced.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_2fa_scoped_to_jwt_user_not_first_user() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let (port, _server) = start_test_server(temp_dir.path()).await;
    let client = Client::new();
    let base = format!("http://127.0.0.1:{port}");

    async fn register(client: &Client, base: &str, email: &str, password: &str) {
        let resp = client
            .post(format!("{base}/auth/register"))
            .json(&json!({ "email": email, "name": "Regression Test", "password": password }))
            .send()
            .await
            .expect("register request failed");
        assert!(
            resp.status() == StatusCode::CREATED || resp.status() == StatusCode::OK,
            "register({email}) expected 200/201, got {}",
            resp.status()
        );
    }

    async fn login(
        client: &Client,
        base: &str,
        email: &str,
        password: &str,
        totp_code: Option<&str>,
    ) -> reqwest::Response {
        client
            .post(format!("{base}/auth/login"))
            .json(&json!({ "email": email, "password": password, "totp_code": totp_code }))
            .send()
            .await
            .expect("login request failed")
    }

    let email_a = "regression-user-a@example.com";
    let email_b = "regression-user-b@example.com";
    let password = "securepassword123"; // 12+ chars, satisfies validate_registration

    // A registers FIRST — under the old bug, A is exactly the account
    // `list_users().next()` would return regardless of who is authenticated.
    register(&client, &base, email_a, password).await;
    register(&client, &base, email_b, password).await;

    // B logs in and sets up + verifies 2FA using ONLY B's own JWT.
    let login_b = login(&client, &base, email_b, password, None).await;
    assert_eq!(
        login_b.status(),
        StatusCode::OK,
        "B's initial login should succeed"
    );
    let login_b_body: Value = login_b.json().await.expect("B login body");
    let access_token_b = login_b_body["access_token"]
        .as_str()
        .expect("B access_token")
        .to_string();

    let setup_b = client
        .post(format!("{base}/auth/2fa/setup"))
        .bearer_auth(&access_token_b)
        .send()
        .await
        .expect("2fa/setup as B failed");
    assert_eq!(
        setup_b.status(),
        StatusCode::OK,
        "2fa/setup as B should succeed"
    );
    let setup_b_body: Value = setup_b.json().await.expect("setup_b body");
    let secret_b = setup_b_body["secret"].as_str().expect("secret").to_string();

    let code_b = xavier::auth2::build_totp(&secret_b, email_b)
        .expect("build_totp for B")
        .generate_current()
        .to_string();

    let verify_b = client
        .post(format!("{base}/auth/2fa/verify"))
        .bearer_auth(&access_token_b)
        .json(&json!({ "code": code_b }))
        .send()
        .await
        .expect("2fa/verify as B failed");
    assert_eq!(
        verify_b.status(),
        StatusCode::OK,
        "2fa/verify as B should succeed with B's own code"
    );

    // A must be totally unaffected: plain password login still works, no TOTP required.
    let login_a_after = login(&client, &base, email_a, password, None).await;
    assert_eq!(
        login_a_after.status(),
        StatusCode::OK,
        "A must still log in with just a password — B's 2FA setup/verify must not have \
         enrolled A into 2FA (the exact regression this test guards against)"
    );
    let login_a_body: Value = login_a_after.json().await.expect("A login body");
    assert_eq!(
        login_a_body["requires_2fa"], false,
        "A's account must not report requires_2fa: true"
    );

    // B, on the other hand, must now be enforced: password alone is rejected.
    let login_b_no_code = login(&client, &base, email_b, password, None).await;
    assert_eq!(
        login_b_no_code.status(),
        StatusCode::UNAUTHORIZED,
        "B must now require a 2FA code"
    );
    let login_b_no_code_body: Value = login_b_no_code.json().await.expect("body");
    assert_eq!(login_b_no_code_body["error"], "mfa_required");

    // And B's own fresh TOTP code (own account, own secret) logs B in.
    let code_b_2 = xavier::auth2::build_totp(&secret_b, email_b)
        .expect("build_totp for B (2nd)")
        .generate_current()
        .to_string();
    let login_b_with_code = login(&client, &base, email_b, password, Some(&code_b_2)).await;
    assert_eq!(
        login_b_with_code.status(),
        StatusCode::OK,
        "B must be able to log in with B's own live TOTP code"
    );
}
