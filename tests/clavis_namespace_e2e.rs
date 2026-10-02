//! E2E test verifying cross-process/cross-namespace key contract between CLI and HTTP handler.

use axum::extract::Path;
use axum::response::IntoResponse;
use xavier::adapters::inbound::http::handlers::clavis::{get_clavis_key_handler, set_clavis_vault};
use xavier::secrets::keygen::{self, KeyScope, KeyVault};
use xavier::secrets::vault::HardwareVault;

#[tokio::test]
#[serial_test::serial]
async fn test_cli_keys_generated_in_clavis_vault_are_readable_by_http_handler() {
    let dir = tempfile::tempdir().expect("tempdir");

    // Vault used by `xavier keys` CLI (service_name "xavier-clavis")
    let cli_vault = HardwareVault::new("xavier-clavis")
        .isolated(dir.path().to_path_buf(), [7u8; 32]);

    // Mint an API key via CLI keygen module
    let key_name = "e2e_cli_probe";
    let generated = keygen::generate_key(&cli_vault, key_name, KeyScope::Live, 3600, chrono::Utc::now())
        .expect("generate CLI key");

    // Install HTTP Clavis vault pointing to same isolated path
    let http_vault = std::sync::Arc::new(
        HardwareVault::new("xavier-clavis")
            .isolated(dir.path().to_path_buf(), [7u8; 32])
    );
    set_clavis_vault(http_vault);

    // 1. Read key by raw name 'e2e_cli_probe'
    let resp1 = get_clavis_key_handler(Path(key_name.to_string())).await.into_response();
    assert_eq!(resp1.status(), axum::http::StatusCode::OK);

    let body1 = axum::body::to_bytes(resp1.into_body(), 64 * 1024).await.expect("bytes");
    let json1: serde_json::Value = serde_json::from_slice(&body1).expect("json");
    assert_eq!(json1["value"], generated.value);

    // 2. Read key with 'api_key_e2e_cli_probe' prefix
    let pref_name = format!("api_key_{key_name}");
    let resp2 = get_clavis_key_handler(Path(pref_name)).await.into_response();
    assert_eq!(resp2.status(), axum::http::StatusCode::OK);

    let body2 = axum::body::to_bytes(resp2.into_body(), 64 * 1024).await.expect("bytes");
    let json2: serde_json::Value = serde_json::from_slice(&body2).expect("json");
    assert_eq!(json2["value"], generated.value);
}

#[tokio::test]
#[serial_test::serial]
async fn test_legacy_xavier_namespace_key_migrates_on_http_read() {
    let dir = tempfile::tempdir().expect("tempdir");

    // Store key in legacy "xavier" namespace
    let legacy_vault = HardwareVault::new("xavier")
        .isolated(dir.path().to_path_buf(), [7u8; 32]);
    legacy_vault.store_secret("api_key_legacy_probe", "sk_live_legacy_secret_999").expect("store legacy");

    // Set Clavis HTTP vault
    let http_vault = std::sync::Arc::new(
        HardwareVault::new("xavier-clavis")
            .isolated(dir.path().to_path_buf(), [7u8; 32])
    );
    set_clavis_vault(http_vault.clone());

    // HTTP read for 'legacy_probe' triggers migration
    let resp = get_clavis_key_handler(Path("legacy_probe".to_string())).await.into_response();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 64 * 1024).await.expect("bytes");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["value"], "sk_live_legacy_secret_999");

    // Verify key was migrated to the Clavis vault
    assert_eq!(http_vault.get_secret("legacy_probe").expect("migrated"), "sk_live_legacy_secret_999");
}
