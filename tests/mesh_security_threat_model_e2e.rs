use axum::http::{Method, StatusCode};
use rand::rngs::OsRng;
use rand::RngCore;
use std::collections::HashSet;
use tower::ServiceExt;

use xavier::mesh::private_mesh::{
    decrypt_session_payload, derive_ecdh_session_key, EncryptedSessionPayload,
};
use xavier::mesh::replay_store::{Claim, ReplayStore, SqliteReplayStore};
use xavier::mesh::DataSanitizer;

use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

#[path = "router_harness/mod.rs"]
mod router_harness;
use router_harness::test_app;

#[tokio::test]
async fn test_csprng_token_entropy_and_uniqueness() {
    let mut tokens = HashSet::new();
    let mut rng = OsRng;

    for _ in 0..1000 {
        let mut token_bytes = [0u8; 32];
        rng.fill_bytes(&mut token_bytes);
        let token_hex = hex::encode(token_bytes);

        assert!(token_bytes.len() >= 16);
        assert!(tokens.insert(token_hex), "Collision detected in CSPRNG");
    }

    assert_eq!(tokens.len(), 1000, "Collision rate must be 0");
}

#[tokio::test]
#[ignore = "Cloudflare edge-hosted route"]
async fn test_bot_prefetch_magic_link_protection() {
    let t = test_app().await;
    let test_token = "mock-token-1234".to_string();

    let req = axum::http::Request::builder()
        .method(Method::HEAD)
        .uri(format!("/auth/magic?token={}", test_token))
        .body(axum::body::Body::empty())
        .unwrap();
    let res = t.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK, "HEAD should return HTTP 200");

    let req = axum::http::Request::builder()
        .method(Method::GET)
        .uri(format!("/auth/magic?token={}", test_token))
        .body(axum::body::Body::empty())
        .unwrap();
    let res = t.app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "First GET should return HTTP 200 (redeemed)"
    );

    let req = axum::http::Request::builder()
        .method(Method::GET)
        .uri(format!("/auth/magic?token={}", test_token))
        .body(axum::body::Body::empty())
        .unwrap();
    let res = t.app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::GONE,
        "Repeat GET should return HTTP 410 Gone"
    );
}

#[test]
fn test_ed25519_signature_replay_rejection() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let store = SqliteReplayStore::new(std::sync::Arc::new(std::sync::Mutex::new(conn))).unwrap();

    let sender = "test-sender";
    let nonce = "test-nonce-123";
    let now = 1000;

    let claim1 = store.claim(sender, nonce, now, now).unwrap();
    assert_eq!(claim1, Claim::Fresh);

    let claim2 = store.claim(sender, nonce, now, now).unwrap();
    assert_eq!(claim2, Claim::Replayed);

    let expired_ts = now - 301;
    let claim3 = store.claim(sender, "new-nonce", expired_ts, now).unwrap();
    assert_eq!(claim3, Claim::Stale);
}

#[test]
fn test_sensitive_data_redaction_before_mesh_export() {
    let sanitizer = DataSanitizer::new();

    let mock_payload = "Here is my private API key: sk-1234abcd5678efgh, keep it safe.";
    let redacted = sanitizer.sanitize_string(mock_payload);

    assert!(!redacted.contains("sk-1234abcd5678efgh"));
}

#[test]
fn test_non_contributory_ecdh_key_rejection() {
    let mut rng = OsRng;
    let local_secret = StaticSecret::random_from_rng(&mut rng);

    let degenerate_pubkey_bytes = [0u8; 32];
    let peer_ephem_public = X25519PublicKey::from(degenerate_pubkey_bytes);

    let shared_secret = local_secret.diffie_hellman(&peer_ephem_public);
    assert!(
        !shared_secret.was_contributory(),
        "Should be non-contributory"
    );

    let encrypted = EncryptedSessionPayload {
        ephemeral_public_key_hex: hex::encode(degenerate_pubkey_bytes),
        ciphertext_hex: hex::encode(vec![0; 16]),
        nonce_hex: hex::encode(vec![0; 24]),
        recipient_node_id: "test".to_string(),
    };

    let result = decrypt_session_payload(&encrypted, &local_secret);
    match result {
        Err(e) => {
            assert!(
                e.to_string()
                    .contains("Non-contributory ECDH shared secret"),
                "Decryption failed but with unexpected error: {}",
                e
            );
        }
        Ok(_) => panic!("Decryption should have failed fast with non-contributory key"),
    }
}
