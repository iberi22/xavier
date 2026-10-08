//! Mesh Authentication & Node ID Cryptographic Binding Tests (#2795 / CWE-347)
//!
//! Asserts that any node attempting to impersonate another node by providing its
//! own public key and signing with its own private key while declaring a different
//! `node_id` is strictly rejected with HTTP 401 Unauthorized.

xavier::isolate_test_process!();

use axum::{routing::post, Extension, Router};
use reqwest::StatusCode;
use serial_test::serial;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::net::TcpListener;
use ulid::Ulid;
use xavier::agents::RuntimeConfig;
use xavier::memory::store::MemoryBackend;
use xavier::mesh::protocol::{MeshHandshake, MeshHandshakeResponse};
use xavier::mesh::NodeIdentity;
use xavier::workspace::{WorkspaceConfig, WorkspaceContext, WorkspaceState};

async fn start_test_server() -> (String, String) {
    let config_dir = tempdir().unwrap();
    let config_path = config_dir.path().join("xavier-config.json");
    let config_json = serde_json::json!({
        "license": { "mesh_accepted": true, "license_type": "AGPL-3.0" }
    });
    std::fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();
    unsafe {
        std::env::set_var("XAVIER_CONFIG_PATH", config_path.as_os_str());
        std::env::set_var("XAVIER_CONFIG_DIR", config_dir.path());
    }

    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    };

    let token = format!("test-token-{}", Ulid::new());
    let workspace_id = format!("test-ws-{}", Ulid::new());
    let temp_dir = tempdir().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = WorkspaceConfig {
        id: workspace_id.clone(),
        token: token.clone(),
        plan: xavier::workspace::PlanTier::Personal,
        memory_backend: MemoryBackend::Memory,
        storage_limit_bytes: None,
        request_limit: None,
        request_unit_limit: None,
        embedding_provider_mode: xavier::workspace::EmbeddingProviderMode::BringYourOwn,
        managed_google_embeddings: false,
        sync_policy: xavier::workspace::SyncPolicy::CloudMirror,
        protocol: Default::default(),
        dedup: Default::default(),
    };

    let workspace = Arc::new(
        WorkspaceState::new(config, RuntimeConfig::default(), workspace_dir)
            .await
            .unwrap(),
    );

    let workspace_ctx = WorkspaceContext {
        workspace_id: workspace_id.clone(),
        workspace: workspace.clone(),
    };

    let app = Router::new()
        .route(
            "/v1/mesh/handshake",
            post(xavier::server::v1_api::v1_mesh_handshake),
        )
        .layer(Extension(workspace_ctx));

    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await.unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("http://{}", addr), token)
}

#[tokio::test]
#[serial]
async fn test_handshake_rejects_impersonated_node_id() {
    let (url, token) = start_test_server().await;

    let victim = NodeIdentity::generate();
    let attacker = NodeIdentity::generate();

    let nonce = uuid::Uuid::new_v4().to_string();
    let signature = attacker.sign(nonce.as_bytes());

    // Attacker crafts handshake claiming victim's node_id, but carrying attacker's public key
    let forged_handshake = MeshHandshake {
        node_id: victim.node_id.clone(),
        public_key_hex: xavier::crypto::hex_encode(&attacker.public_key),
        xavier_version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: vec!["sync-v1".to_string()],
        timestamp: chrono::Utc::now().timestamp(),
        nonce,
        signature_hex: xavier::crypto::hex_encode(signature),
        pairing_secret: None,
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/mesh/handshake", url))
        .header("X-Xavier-Token", &token)
        .json(&forged_handshake)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["accepted"], false);
    assert_eq!(body["reason"], "node_id does not match public key");
}

#[tokio::test]
#[serial]
async fn test_handshake_accepts_valid_node_id() {
    let (url, token) = start_test_server().await;

    let legitimate_node = NodeIdentity::generate();

    let nonce = uuid::Uuid::new_v4().to_string();
    let signature = legitimate_node.sign(nonce.as_bytes());

    let handshake = MeshHandshake {
        node_id: legitimate_node.node_id.clone(),
        public_key_hex: xavier::crypto::hex_encode(&legitimate_node.public_key),
        xavier_version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: vec!["sync-v1".to_string()],
        timestamp: chrono::Utc::now().timestamp(),
        nonce,
        signature_hex: xavier::crypto::hex_encode(signature),
        pairing_secret: None,
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/mesh/handshake", url))
        .header("X-Xavier-Token", &token)
        .json(&handshake)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(resp.status(), StatusCode::OK);

    let body: MeshHandshakeResponse = resp.json().await.unwrap();
    assert!(body.accepted);
}
