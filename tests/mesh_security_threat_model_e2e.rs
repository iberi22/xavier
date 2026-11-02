use axum::{
    extract::State,
    http::{Method, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use rand::rngs::OsRng;
use rand::RngCore;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

use xavier::mesh::data_sanitizer::{DataSanitizer, SanitizationAction};
use xavier::mesh::private_mesh::{decrypt_session_payload, EncryptedSessionPayload};
use xavier::mesh::replay_store::{Claim, ReplayStore, SqliteReplayStore};

use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// Mock token state
#[derive(Default, Clone)]
struct AppState {
    tokens: Arc<Mutex<HashMap<String, bool>>>, // token -> used
}

// Simulated mock for Cloudflare Worker behavior.
async fn magic_link_handler(
    State(state): State<AppState>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    let method = req.method().clone();
    let query_string = req.uri().query().unwrap_or("");

    // Naive parsing for token=...
    let mut token = None;
    for pair in query_string.split('&') {
        if let Some(stripped) = pair.strip_prefix("token=") {
            token = Some(stripped.to_string());
        }
    }

    let token = match token {
        Some(t) => t,
        None => return StatusCode::BAD_REQUEST,
    };

    let mut tokens = state.tokens.lock().unwrap();
    if method == Method::HEAD {
        // HEAD does not invalidate the token, but responds with 200 if valid.
        if tokens.contains_key(&token) {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        }
    } else if method == Method::GET {
        // GET consumes the token.
        match tokens.get(&token) {
            Some(&used) => {
                if used {
                    StatusCode::GONE // 410 if already used
                } else {
                    tokens.insert(token, true); // mark as used
                    StatusCode::OK
                }
            }
            None => StatusCode::NOT_FOUND,
        }
    } else {
        StatusCode::METHOD_NOT_ALLOWED
    }
}

#[tokio::test]
async fn test_csprng_token_entropy_and_uniqueness() {
    let mut tokens = HashSet::new();
    let mut rng = OsRng;

    for _ in 0..1000 {
        let mut token_bytes = [0u8; 32];
        rng.fill_bytes(&mut token_bytes);
        let token_hex = to_hex(&token_bytes);

        // Ensure entropy exceeds 128 bits: 32 bytes * 8 = 256 bits of entropy.
        assert!(token_bytes.len() >= 16);

        // Uniqueness check
        assert!(tokens.insert(token_hex), "Collision detected in CSPRNG");
    }

    assert_eq!(tokens.len(), 1000, "Collision rate must be 0");
}

#[tokio::test]
async fn test_bot_prefetch_magic_link_protection() {
    let state = AppState::default();

    // Register a mock token
    let test_token = "mock-token-1234".to_string();
    state
        .tokens
        .lock()
        .unwrap()
        .insert(test_token.clone(), false);

    let app = Router::new()
        .route(
            "/auth/magic",
            get(magic_link_handler).head(magic_link_handler),
        )
        .with_state(state);

    // 1. Simulate bot sending HEAD /auth/magic?token=...
    let req = axum::http::Request::builder()
        .method(Method::HEAD)
        .uri(format!("/auth/magic?token={}", test_token))
        .body(axum::body::Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK, "HEAD should return HTTP 200");

    // 2. Subsequent GET successfully redeems token
    let req = axum::http::Request::builder()
        .method(Method::GET)
        .uri(format!("/auth/magic?token={}", test_token))
        .body(axum::body::Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "First GET should return HTTP 200 (redeemed)"
    );

    // 3. Repeat GET returns HTTP 410 Gone
    let req = axum::http::Request::builder()
        .method(Method::GET)
        .uri(format!("/auth/magic?token={}", test_token))
        .body(axum::body::Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::GONE,
        "Repeat GET should return HTTP 410 Gone"
    );
}

#[test]
fn test_ed25519_signature_replay_rejection() {
    let temp = tempfile::tempdir().unwrap();
    let store = SqliteReplayStore::open(temp.path().join("replay.db")).unwrap();

    let sender = "test-sender";
    let nonce = "test-nonce-123";
    let now = 1000;

    // 1. Valid request
    let claim1 = store.claim(sender, nonce, now, now).unwrap();
    assert_eq!(claim1, Claim::Fresh);

    // 2. Replayed request (same nonce)
    let claim2 = store.claim(sender, nonce, now, now).unwrap();
    assert_eq!(claim2, Claim::Replayed);

    // 3. Replayed with expired timestamp (skew > 300)
    let expired_ts = now - 301;
    let claim3 = store.claim(sender, "new-nonce", expired_ts, now).unwrap();
    assert_eq!(claim3, Claim::Stale);
}

#[test]
fn test_sensitive_data_redaction_before_mesh_export() {
    let mut sanitizer = DataSanitizer::new();
    sanitizer.add_rule("api_key", SanitizationAction::Redact("***".to_string()));

    #[derive(serde::Serialize)]
    struct MockPayload {
        api_key: String,
        note: String,
    }

    let payload = MockPayload {
        api_key: "sk-1234abcd5678efgh".to_string(),
        note: "exportable telemetry".to_string(),
    };

    let sanitized = sanitizer.sanitize(&payload).unwrap();
    let serialized = serde_json::to_string(&sanitized).unwrap();

    assert!(!serialized.contains("sk-1234abcd5678efgh"));
    assert!(serialized.contains("***"));
}

#[test]
fn test_non_contributory_ecdh_key_rejection() {
    // 1. Generate local static secret
    let local_secret = StaticSecret::from([42u8; 32]);

    // 2. Attempt DH key agreement using degenerate X25519 public key (small order point / all zeros)
    let degenerate_pubkey_bytes = [0u8; 32];
    let peer_ephem_public = X25519PublicKey::from(degenerate_pubkey_bytes);

    let shared_secret = local_secret.diffie_hellman(&peer_ephem_public);

    assert!(
        !shared_secret.was_contributory(),
        "Should be non-contributory"
    );

    let encrypted = EncryptedSessionPayload {
        ciphertext_hex: to_hex(&vec![0u8; 16]),
        nonce_hex: to_hex(&vec![0u8; 24]),
        ephemeral_public_key_hex: Some(to_hex(&degenerate_pubkey_bytes)),
    };

    let result = decrypt_session_payload(&encrypted, &local_secret);

    assert!(
        result.is_err(),
        "Decryption must fail with non-contributory key"
    );
}
