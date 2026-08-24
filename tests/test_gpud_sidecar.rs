//! Integration and unit tests for GPU Sidecar `gpud` service, fallback policy, and REST API.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use std::time::Duration;
use tower::ServiceExt; // for oneshot
use xavier::server::gpud_sidecar::{
    router, ExecutionBackend, GpuDeviceInfo, GpudFallbackPolicy, GpudHealthStatus, GpudService,
    ServeRequest, ServeResponse, StatusResponse,
};

#[tokio::test]
async fn test_policy_gpu_selected_when_healthy_and_vram_sufficient() {
    let policy = GpudFallbackPolicy::new(1024, 95.0, true);
    let dev = GpuDeviceInfo {
        detected: true,
        vendor: Some("NVIDIA".to_string()),
        model: Some("NVIDIA RTX 4090".to_string()),
        vram_total_mb: 24576,
        vram_free_mb: 16384,
        driver_version: Some("535.104.05".to_string()),
        cuda_available: true,
    };
    let health = GpudHealthStatus::Healthy;

    let backend = policy.evaluate(Some(&dev), &health);
    match backend {
        ExecutionBackend::Gpu {
            vendor,
            model,
            vram_free_mb,
            vram_total_mb,
        } => {
            assert_eq!(vendor, "NVIDIA");
            assert_eq!(model, "NVIDIA RTX 4090");
            assert_eq!(vram_free_mb, 16384);
            assert_eq!(vram_total_mb, 24576);
        }
        _ => panic!("Expected ExecutionBackend::Gpu, got {:?}", backend),
    }
}

#[tokio::test]
async fn test_policy_fallback_cpu_when_vram_insufficient() {
    let policy = GpudFallbackPolicy::new(2048, 95.0, true);
    let dev = GpuDeviceInfo {
        detected: true,
        vendor: Some("NVIDIA".to_string()),
        model: Some("NVIDIA GTX 1050".to_string()),
        vram_total_mb: 4096,
        vram_free_mb: 512, // Less than 2048MB required
        driver_version: Some("470.100".to_string()),
        cuda_available: true,
    };
    let health = GpudHealthStatus::Healthy;

    let backend = policy.evaluate(Some(&dev), &health);
    match backend {
        ExecutionBackend::Cpu { reason } => {
            assert!(
                reason.contains("Insufficient free VRAM"),
                "Expected VRAM error, got: {}",
                reason
            );
        }
        _ => panic!("Expected ExecutionBackend::Cpu, got {:?}", backend),
    }
}

#[tokio::test]
async fn test_policy_fallback_cpu_when_vram_utilization_exceeded() {
    let policy = GpudFallbackPolicy::new(512, 90.0, true);
    let dev = GpuDeviceInfo {
        detected: true,
        vendor: Some("NVIDIA".to_string()),
        model: Some("NVIDIA T4".to_string()),
        vram_total_mb: 10000,
        vram_free_mb: 800, // 92% used > 90% threshold
        driver_version: Some("525.60".to_string()),
        cuda_available: true,
    };
    let health = GpudHealthStatus::Healthy;

    let backend = policy.evaluate(Some(&dev), &health);
    match backend {
        ExecutionBackend::Cpu { reason } => {
            assert!(
                reason.contains("utilization threshold exceeded"),
                "Expected utilization error, got: {}",
                reason
            );
        }
        _ => panic!("Expected ExecutionBackend::Cpu, got {:?}", backend),
    }
}

#[tokio::test]
async fn test_policy_fallback_cpu_when_health_failed() {
    let policy = GpudFallbackPolicy::default();
    let dev = GpuDeviceInfo {
        detected: true,
        vendor: Some("NVIDIA".to_string()),
        model: Some("NVIDIA A100".to_string()),
        vram_total_mb: 81920,
        vram_free_mb: 80000,
        driver_version: Some("530.30".to_string()),
        cuda_available: true,
    };
    let health = GpudHealthStatus::Failed {
        reason: "Overheating warning triggered".to_string(),
    };

    let backend = policy.evaluate(Some(&dev), &health);
    match backend {
        ExecutionBackend::Cpu { reason } => {
            assert!(
                reason.contains("GPU health check failed"),
                "Expected health failure reason, got: {}",
                reason
            );
        }
        _ => panic!("Expected ExecutionBackend::Cpu, got {:?}", backend),
    }
}

#[tokio::test]
async fn test_policy_fallback_cpu_when_no_gpu_detected() {
    let policy = GpudFallbackPolicy::default();
    let dev = GpuDeviceInfo {
        detected: false,
        ..Default::default()
    };
    let health = GpudHealthStatus::Healthy;

    let backend = policy.evaluate(Some(&dev), &health);
    match backend {
        ExecutionBackend::Cpu { reason } => {
            assert!(reason.contains("No compatible GPU hardware detected"));
        }
        _ => panic!("Expected ExecutionBackend::Cpu, got {:?}", backend),
    }

    let backend_none = policy.evaluate(None, &health);
    assert!(matches!(backend_none, ExecutionBackend::Cpu { .. }));
}

#[tokio::test]
async fn test_axum_health_endpoint() {
    let service = GpudService::default();
    let app = router(service);

    let request = Request::builder()
        .uri("/v1/gpud/health")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(json.get("status").is_some());
    assert!(json.get("active_backend").is_some());
    assert!(json.get("last_check").is_some());
}

#[tokio::test]
async fn test_axum_detect_endpoint() {
    let service = GpudService::default();
    let app = router(service);

    let request = Request::builder()
        .uri("/v1/gpud/detect")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let dev_info: GpuDeviceInfo = serde_json::from_slice(&body_bytes).unwrap();
    // In CI or test container, detected might be true or false depending on hardware,
    // but the payload must parse cleanly as GpuDeviceInfo.
    let _ = dev_info.detected;
}

#[tokio::test]
async fn test_axum_serve_endpoint_fallback() {
    let service = GpudService::default();
    let app = router(service);

    let serve_req = ServeRequest {
        prompt: "Synthesize quantum state".to_string(),
        model: Some("llama-3-8b".to_string()),
    };
    let req_body = serde_json::to_vec(&serve_req).unwrap();

    let request = Request::builder()
        .uri("/v1/gpud/serve")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let serve_res: ServeResponse = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(serve_res.model_used, "llama-3-8b");
    assert!(!serve_res.output.is_empty());
}

#[tokio::test]
async fn test_axum_status_endpoint() {
    let service = GpudService::default();
    let app = router(service);

    let request = Request::builder()
        .uri("/v1/gpud/status")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let status_res: StatusResponse = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(status_res.policy.min_vram_mb, 1024);
    assert_eq!(status_res.policy.max_vram_utilization_pct, 95.0);
}

#[tokio::test]
async fn test_service_health_status_transition() {
    let service = GpudService::default();

    // Set healthy state
    service.set_health_status(GpudHealthStatus::Healthy).await;

    // Trigger failure
    service
        .set_health_status(GpudHealthStatus::Failed {
            reason: "GPU power disconnect".to_string(),
        })
        .await;

    let active_backend = service.state.active_backend.read().await.clone();
    match active_backend {
        ExecutionBackend::Cpu { reason } => {
            assert!(reason.contains("GPU power disconnect"));
        }
        _ => panic!("Expected transition to ExecutionBackend::Cpu upon health failure"),
    }
}

#[tokio::test]
async fn test_background_monitor_spawn() {
    let service = GpudService::default();
    let handle = service.start_background_monitor(Duration::from_millis(50));
    tokio::time::sleep(Duration::from_millis(120)).await;
    handle.abort();
}
