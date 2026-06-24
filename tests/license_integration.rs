//! License Integration Tests
//!
//! Tests license enforcement at the API level.

use axum::{
    routing::get,
    Extension, Router,
};
use std::sync::Arc;
use tempfile::tempdir;
use tokio::net::TcpListener;
use ulid::Ulid;
use xavier::agents::RuntimeConfig;
use xavier::memory::store::MemoryBackend;
use xavier::workspace::{WorkspaceConfig, WorkspaceContext, WorkspaceState};
use xavier::settings::{XavierSettings, tests::ENV_LOCK};

async fn start_test_server(mesh_accepted: bool, commercial_key: Option<&str>) -> (String, String, Arc<WorkspaceState>, tempfile::TempDir) {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    };

    let token = format!("test-token-{}", Ulid::new());
    let workspace_id = format!("test-ws-{}", Ulid::new());
    let temp_dir = tempdir().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    // Create a test-specific config file to avoid global state pollution and ensure thread safety
    let config_path = temp_dir.path().join("xavier.config.json");
    let mut settings = XavierSettings::default();
    settings.license.mesh_accepted = mesh_accepted;
    if let Some(key) = commercial_key {
        settings.license.commercial_key = Some(key.to_string());
        settings.license.license_type = "Xavier-Commercial-1.0".to_string();
    }
    settings.server.port = port;

    let config_json = serde_json::to_string(&settings).unwrap();
    std::fs::write(&config_path, config_json).unwrap();

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
            "/v1/mesh/identity",
            get(xavier::server::v1_api::v1_mesh_identity),
        )
        .layer(Extension(workspace_ctx));

    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await.unwrap();

    // We pass the config path via an environment variable that is read by the server in its own thread/context if needed.
    // However, XavierSettings::current() might still be problematic if it doesn't look at the env var per-thread.
    // In many of our handlers, we use XavierSettings::current().
    // For these tests to be truly isolated, we need a way to pass settings to the router or handlers.
    // Since v1_mesh_identity calls XavierSettings::current(), we must set XAVIER_CONFIG_PATH.
    // WARNING: std::env::set_var is still global. But by using different ports and workspaces,
    // and unique config files, we minimize collisions if we run with --test-threads=1.
    // To support parallel tests, the app itself should probably take settings as an Extension.

    tokio::spawn(async move {
        // This is still slightly risky but better than before.
        std::env::set_var("XAVIER_CONFIG_PATH", config_path);
        axum::serve(listener, app).await.unwrap();
    });

    (format!("http://{}", addr), token, workspace, temp_dir)
}

#[tokio::test]
async fn test_mesh_features_blocked_without_mesh_license() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (url, token, _ws, _temp) = start_test_server(false, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/v1/mesh/identity", url))
        .header("X-Xavier-Token", &token)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), axum::http::StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("require the Xavier Mesh License"));
}

#[tokio::test]
async fn test_mesh_features_allowed_after_license_accept() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (url, token, _ws, _temp) = start_test_server(true, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/v1/mesh/identity", url))
        .header("X-Xavier-Token", &token)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), axum::http::StatusCode::OK);
}

#[tokio::test]
async fn test_commercial_features_blocked_without_commercial_license() {
    // Only relevant if enterprise feature is enabled
    if !cfg!(feature = "enterprise") {
        return;
    }

    let _guard = ENV_LOCK.lock().unwrap();
    let (url, token, _ws, _temp) = start_test_server(true, None).await;
    let client = reqwest::Client::new();

    // We need an enterprise-only endpoint. /plugins/health is gated by require_commercial_license indirectly or directly.
    // Looking at src/cli/server.rs, /plugins/health is added in enterprise feature.
    let resp = client
        .get(format!("{}/plugins/health", url))
        .header("X-Xavier-Token", &token)
        .send()
        .await
        .unwrap();

    // If it's gated correctly, it should be 403 or 401 depending on middleware.
    // Current requirement is that it is BLOCKED.
    assert!(resp.status() == axum::http::StatusCode::FORBIDDEN || resp.status() == axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_license_state_persists_across_restart() {
    let _guard = ENV_LOCK.lock().unwrap();
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("xavier.config.json");

    // Set env var for this test
    std::env::set_var("XAVIER_CONFIG_PATH", &config_path);

    let mut settings = XavierSettings::default();
    settings.license.mesh_accepted = true;
    settings.license.license_type = "Xavier-Mesh-1.0".to_string();
    settings.save().await.unwrap();

    // Reload
    let loaded = XavierSettings::load().unwrap().expect("should load settings");
    assert!(loaded.license.mesh_accepted);
    assert_eq!(loaded.license.license_type, "Xavier-Mesh-1.0");
}

#[tokio::test]
async fn test_license_status_endpoint() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (url, token, _ws, _temp) = start_test_server(true, Some("test-commercial-key")).await;
    let client = reqwest::Client::new();

    // We don't have a direct /v1/license/status endpoint yet,
    // but v1_mesh_identity returns some info or we can check xavier license status command.
    // For now, let's just verify identity works when licensed.
    let resp = client
        .get(format!("{}/v1/mesh/identity", url))
        .header("X-Xavier-Token", &token)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), axum::http::StatusCode::OK);
}
