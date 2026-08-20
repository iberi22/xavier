use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use std::sync::Arc;
use tempfile::NamedTempFile;
use tower::ServiceExt;
use xavier::server::maloca::data_node::{
    router, ConsentUpdateRequest, DataNodeConfig, DataNodeConsentResponse, DataNodeManager,
};

#[test]
fn test_data_node_config_and_conversions() {
    let default_config = DataNodeConfig::default();
    assert!(!default_config.opt_in);
    assert_eq!(default_config.storage_quota_mb, 1024);
    assert_eq!(default_config.quota_bytes(), 1024 * 1024 * 1024);

    let custom_config = DataNodeConfig::new(true, 2048);
    assert!(custom_config.opt_in);
    assert_eq!(custom_config.storage_quota_mb, 2048);
    assert_eq!(custom_config.quota_bytes(), 2048 * 1024 * 1024);

    let resp = DataNodeConsentResponse::from(&custom_config);
    assert!(resp.opt_in);
    assert_eq!(resp.storage_quota_mb, 2048);
    assert_eq!(resp.storage_quota_bytes, 2048 * 1024 * 1024);
    assert_eq!(resp.status, "opted_in");

    let resp_out = DataNodeConsentResponse::from(&default_config);
    assert_eq!(resp_out.status, "opted_out");
}

#[test]
fn test_manager_state_transitions() {
    let manager = DataNodeManager::default();

    // Initial state check
    assert!(!manager.is_opted_in());
    assert_eq!(manager.storage_quota_mb(), 1024);
    assert_eq!(manager.storage_quota_bytes(), 1024 * 1024 * 1024);

    let initial_resp = manager.get_consent_response();
    assert!(!initial_resp.opt_in);
    assert_eq!(initial_resp.status, "opted_out");

    // Transition opt_in: false -> true
    let res1 = manager.update_consent(Some(true), None);
    assert!(res1.opt_in);
    assert_eq!(res1.storage_quota_mb, 1024);
    assert_eq!(res1.status, "opted_in");
    assert!(manager.is_opted_in());

    // Transition storage_quota_mb update
    let res2 = manager.update_consent(None, Some(4096));
    assert!(res2.opt_in);
    assert_eq!(res2.storage_quota_mb, 4096);
    assert_eq!(res2.storage_quota_bytes, 4096 * 1024 * 1024);

    // Transition opt_in: true -> false
    let res3 = manager.update_consent(Some(false), Some(512));
    assert!(!res3.opt_in);
    assert_eq!(res3.storage_quota_mb, 512);
    assert_eq!(res3.status, "opted_out");
    assert!(!manager.is_opted_in());

    // Empty transition update
    let res4 = manager.update_consent(None, None);
    assert!(!res4.opt_in);
    assert_eq!(res4.storage_quota_mb, 512);
}

#[test]
fn test_manager_quota_boundary_values() {
    let manager = DataNodeManager::default();

    // Zero quota
    let res_zero = manager.update_consent(Some(true), Some(0));
    assert_eq!(res_zero.storage_quota_mb, 0);
    assert_eq!(res_zero.storage_quota_bytes, 0);
    assert_eq!(manager.storage_quota_bytes(), 0);

    // Large quota boundary
    let max_mb = u32::MAX;
    let res_max = manager.update_consent(None, Some(max_mb));
    assert_eq!(res_max.storage_quota_mb, max_mb);
    let expected_bytes = (max_mb as u64) * 1024 * 1024;
    assert_eq!(res_max.storage_quota_bytes, expected_bytes);
    assert_eq!(manager.storage_quota_bytes(), expected_bytes);
}

#[test]
fn test_file_persistence_and_reload() {
    let temp_file = NamedTempFile::new().unwrap();
    let file_path = temp_file.path().to_path_buf();

    // Create manager with file path and modify settings
    let manager = DataNodeManager::default().with_file_path(file_path.clone());
    manager.update_consent(Some(true), Some(8192));

    // Ensure state was saved to file and load new instance from file
    assert!(file_path.exists());
    let reloaded_manager = DataNodeManager::from_file(&file_path).unwrap();

    assert!(reloaded_manager.is_opted_in());
    assert_eq!(reloaded_manager.storage_quota_mb(), 8192);
    assert_eq!(reloaded_manager.storage_quota_bytes(), 8192 * 1024 * 1024);

    // Non-existent path falls back to default
    let non_existent_path = file_path.parent().unwrap().join("non_existent_consent.json");
    let fallback_manager = DataNodeManager::from_file(&non_existent_path).unwrap();
    assert!(!fallback_manager.is_opted_in());
    assert_eq!(fallback_manager.storage_quota_mb(), 1024);
}

#[tokio::test]
async fn test_axum_http_consent_endpoints() {
    let manager = DataNodeManager::default();
    let app = router(manager.clone());

    // GET /v1/maloca/node/consent
    let req_get = Request::builder()
        .method("GET")
        .uri("/v1/maloca/node/consent")
        .body(Body::empty())
        .unwrap();

    let resp_get = app.clone().oneshot(req_get).await.unwrap();
    assert_eq!(resp_get.status(), StatusCode::OK);

    let body_bytes = to_bytes(resp_get.into_body(), usize::MAX).await.unwrap();
    let get_data: DataNodeConsentResponse = serde_json::from_slice(&body_bytes).unwrap();
    assert!(!get_data.opt_in);
    assert_eq!(get_data.storage_quota_mb, 1024);
    assert_eq!(get_data.status, "opted_out");

    // POST /v1/maloca/node/consent - Opt-in and allocate 2048 MB
    let update_req = ConsentUpdateRequest {
        opt_in: Some(true),
        storage_quota_mb: Some(2048),
    };

    let req_post = Request::builder()
        .method("POST")
        .uri("/v1/maloca/node/consent")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&update_req).unwrap()))
        .unwrap();

    let resp_post = app.clone().oneshot(req_post).await.unwrap();
    assert_eq!(resp_post.status(), StatusCode::OK);

    let body_bytes = to_bytes(resp_post.into_body(), usize::MAX).await.unwrap();
    let post_data: DataNodeConsentResponse = serde_json::from_slice(&body_bytes).unwrap();
    assert!(post_data.opt_in);
    assert_eq!(post_data.storage_quota_mb, 2048);
    assert_eq!(post_data.storage_quota_bytes, 2048 * 1024 * 1024);
    assert_eq!(post_data.status, "opted_in");

    // GET /v1/maloca/node/consent again to confirm manager state update
    let req_get_2 = Request::builder()
        .method("GET")
        .uri("/v1/maloca/node/consent")
        .body(Body::empty())
        .unwrap();

    let resp_get_2 = app.clone().oneshot(req_get_2).await.unwrap();
    assert_eq!(resp_get_2.status(), StatusCode::OK);

    let body_bytes = to_bytes(resp_get_2.into_body(), usize::MAX).await.unwrap();
    let get_data_2: DataNodeConsentResponse = serde_json::from_slice(&body_bytes).unwrap();
    assert!(get_data_2.opt_in);
    assert_eq!(get_data_2.storage_quota_mb, 2048);
}

#[tokio::test]
async fn test_concurrent_updates() {
    let manager = Arc::new(DataNodeManager::default());
    let mut handles = Vec::new();

    for i in 0..10 {
        let mgr = Arc::clone(&manager);
        handles.push(tokio::spawn(async move {
            let opt_in = i % 2 == 0;
            let quota = 1000 + (i * 100);
            mgr.update_consent(Some(opt_in), Some(quota));
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Ensure manager remains accessible and non-corrupted after multi-threaded updates
    let cfg = manager.get_config();
    assert!(cfg.storage_quota_mb >= 1000);
}
