//! Adversarial stress test suite for Challenger 2
//! Stress tests keygen boundary conditions, Clavis HTTP vault boundary conditions,
//! and concurrent test isolation.

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use chrono::Utc;
use http_body_util::BodyExt;
use tempfile::tempdir;
use tower::ServiceExt;

use xavier::adapters::inbound::http::routes::create_router;
use xavier::secrets::keygen::{
    export_key, generate_key, load_metadata, revoke_key, validate_name, KeyScope,
};
use xavier::secrets::vault::HardwareVault;
use xavier::secrets::SecretError;
use xavier::security::auth::{Claims, UserRole};

fn setup_isolated_vault(service_name: &str) -> (HardwareVault, tempfile::TempDir) {
    let dir = tempdir().expect("failed to create temp directory");
    std::env::set_var("HOME", dir.path());
    std::env::set_var("XAVIER_DATA_DIR", dir.path());
    let nonce = rand::random::<u64>();
    let vault = HardwareVault::new(&format!("{service_name}-{nonce:016x}"));
    (vault, dir)
}

fn test_now() -> chrono::DateTime<Utc> {
    chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("valid timestamp")
}

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

#[test]
fn test_adversarial_keygen_boundary_empty_and_invalid_names() {
    let (vault, _dir) = setup_isolated_vault("adv-keygen-empty");
    let now = test_now();

    // 1. Empty name
    assert!(generate_key(&vault, "", KeyScope::Live, 3600, now).is_err());
    assert!(export_key(&vault, "").is_err());
    assert!(revoke_key(&vault, "").is_err());

    // 2. Traversal and illegal characters
    let bad_names = [
        "..",
        ".",
        "../secrets",
        "key/slash",
        "key\\backslash",
        "key name with spaces",
        "key@special#chars$",
        "key\0null",
        "key\nnewline",
        "unicode_ñ_key",
    ];
    for bad in bad_names {
        assert!(
            generate_key(&vault, bad, KeyScope::Live, 3600, now).is_err(),
            "should reject bad name: {bad:?}"
        );
        assert!(export_key(&vault, bad).is_err());
        assert!(revoke_key(&vault, bad).is_err());
    }

    // 3. Length boundary: 64 is allowed, 65 is rejected
    let len_64 = "a".repeat(64);
    let len_65 = "a".repeat(65);
    assert!(validate_name(&len_64).is_ok());
    assert!(validate_name(&len_65).is_err());

    let key_64 = generate_key(&vault, &len_64, KeyScope::Live, 3600, now);
    assert!(key_64.is_ok(), "64-char key name must be accepted");
    let key_65 = generate_key(&vault, &len_65, KeyScope::Live, 3600, now);
    assert!(key_65.is_err(), "65-char key name must be rejected");
}

#[test]
fn test_adversarial_keygen_duplicate_names_and_rotation_lifecycle() {
    let (vault, _dir) = setup_isolated_vault("adv-keygen-duplicate");
    let now = test_now();
    let name = "dup_stress_key";

    // 1st generation
    let k1 = generate_key(&vault, name, KeyScope::Live, 3600, now).expect("k1 gen");
    assert_eq!(k1.metadata.rotation_count, 0);

    // 2nd generation (duplicate name) -> rotation count 1, new value
    let k2 = generate_key(&vault, name, KeyScope::Live, 3600, now).expect("k2 gen");
    assert_eq!(k2.metadata.rotation_count, 1);
    assert_ne!(k1.value, k2.value);

    // 3rd generation -> rotation count 2
    let k3 = generate_key(&vault, name, KeyScope::Live, 3600, now).expect("k3 gen");
    assert_eq!(k3.metadata.rotation_count, 2);
    assert_ne!(k2.value, k3.value);

    // Export returns latest
    let exported = export_key(&vault, name).expect("export latest");
    assert_eq!(exported, k3.value);

    // Revocation cleans up both value and metadata
    assert!(revoke_key(&vault, name).is_ok());
    assert!(export_key(&vault, name).is_err());
    assert!(load_metadata(&vault, name).is_none());

    // Revoke again on nonexistent returns error
    assert!(revoke_key(&vault, name).is_err());
}

#[test]
fn test_adversarial_keygen_nonexistent_keys() {
    let (vault, _dir) = setup_isolated_vault("adv-keygen-nonexistent");
    let name = "never_existed_key_12345";

    let res_export = export_key(&vault, name);
    assert!(res_export.is_err());
    assert!(matches!(res_export.unwrap_err(), SecretError::NotFound(_)));

    let res_revoke = revoke_key(&vault, name);
    assert!(res_revoke.is_err());
    assert!(matches!(res_revoke.unwrap_err(), SecretError::NotFound(_)));

    assert!(load_metadata(&vault, name).is_none());
}

#[tokio::test]
#[serial_test::serial]
async fn test_adversarial_http_vault_boundary_conditions() {
    let (_temp_dir, _vault) = {
        let temp_dir = tempdir().expect("tempdir");
        std::env::set_var("HOME", temp_dir.path());
        std::env::set_var("XAVIER_DATA_DIR", temp_dir.path());
        (temp_dir, xavier::adapters::inbound::http::handlers::clavis::clavis_vault())
    };

    // 1. Missing auth token (unauthenticated GET) -> 403
    let unauth_req = build_router_request(Method::GET, "/v1/clavis/keys/probe_key", None, None);
    let resp = create_router().oneshot(unauth_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 2. Nonexistent key GET -> 404 with code clavis_key_not_found
    let ghost_req = build_router_request(
        Method::GET,
        "/v1/clavis/keys/ghost_key_never_exists",
        None,
        Some(UserRole::Admin),
    );
    let resp = create_router().oneshot(ghost_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["code"], "clavis_key_not_found");

    // 3. Invalid key name with leading dot -> 400
    let dot_req = build_router_request(
        Method::GET,
        "/v1/clavis/keys/.hidden",
        None,
        Some(UserRole::Admin),
    );
    let resp = create_router().oneshot(dot_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 4. PUT empty key value -> correctly rejected with 400 BAD_REQUEST ("value must not be empty")
    let put_empty_req = build_router_request(
        Method::PUT,
        "/v1/clavis/keys/empty_val_key",
        Some(r#"{"value":""}"#),
        Some(UserRole::Admin),
    );
    let resp = create_router().oneshot(put_empty_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 5. PUT valid initial key value -> 201 CREATED
    let put_req = build_router_request(
        Method::PUT,
        "/v1/clavis/keys/valid_val_key",
        Some(r#"{"value":"initial_secret_123"}"#),
        Some(UserRole::Admin),
    );
    let resp = create_router().oneshot(put_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Verify stored value
    let get_req = build_router_request(
        Method::GET,
        "/v1/clavis/keys/valid_val_key",
        None,
        Some(UserRole::Admin),
    );
    let resp = create_router().oneshot(get_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["value"], "initial_secret_123");

    // 6. PUT duplicate overwrite -> returns 200 OK
    let overwrite_req = build_router_request(
        Method::PUT,
        "/v1/clavis/keys/valid_val_key",
        Some(r#"{"value":"new_updated_val"}"#),
        Some(UserRole::Admin),
    );
    let resp = create_router().oneshot(overwrite_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[test]
fn test_adversarial_cross_vault_collision_same_key_name() {
    let (v1, _dir1) = setup_isolated_vault("vault-isolated-1");
    let (v2, _dir2) = setup_isolated_vault("vault-isolated-2");
    let now = test_now();
    let key_name = "collision_test_key";

    // Vault 1 generates a key
    let _k1 = generate_key(&v1, key_name, KeyScope::Live, 3600, now).expect("v1 gen");

    // Vault 2 attempts to export key before generating in v2:
    // If vaults are properly isolated, v2 MUST NOT see v1's key!
    let v2_export = export_key(&v2, key_name);

    println!("v2 export result: {:?}", v2_export);

    // Clean up
    let _ = revoke_key(&v1, key_name);
    let _ = revoke_key(&v2, key_name);

    assert!(
        v2_export.is_err(),
        "CRITICAL ISOLATION BUG: Vault 2 saw Vault 1's secret! Got: {:?}",
        v2_export
    );
}


#[test]
fn test_adversarial_concurrency_isolation_multi_vaults() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;

    let success_count = Arc::new(AtomicUsize::new(0));
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let sc = Arc::clone(&success_count);
            thread::spawn(move || {
                let dir = tempdir().expect("tempdir");
                let nonce = rand::random::<u64>();
                let vault = HardwareVault::new(&format!("concurrent-worker-{i}-{nonce:016x}"));
                let now = test_now();
                let key_name = "concurrent_key_shared_name";

                // Generate key
                let gen = generate_key(&vault, key_name, KeyScope::Test, 3600, now)
                    .expect("generate concurrent");
                let exported = export_key(&vault, key_name).expect("export concurrent");
                assert_eq!(gen.value, exported);

                // Revoke
                revoke_key(&vault, key_name).expect("revoke concurrent");
                assert!(export_key(&vault, key_name).is_err());

                sc.fetch_add(1, Ordering::SeqCst);
                // Keep dir alive until end of thread
                let _ = dir;
            })
        })
        .collect();

    for t in threads {
        t.join().expect("thread join");
    }

    assert_eq!(success_count.load(Ordering::SeqCst), 8);
}
