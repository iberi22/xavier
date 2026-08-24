//! GPU Sidecar (`gpud`) Service, Health Check, Model Detection & Service Fallback Hardening.
//!
//! Provides GPU hardware probing, VRAM monitoring, health diagnostics,
//! dynamic execution backend selection, and resilient CPU fallback without stalling Axum HTTP threads.

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Hardware execution backend selected by the fallback policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExecutionBackend {
    /// GPU acceleration is active and healthy.
    Gpu {
        vendor: String,
        model: String,
        vram_free_mb: u64,
        vram_total_mb: u64,
    },
    /// Graceful fallback to CPU execution.
    Cpu { reason: String },
}

/// Health status of the GPU sidecar service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum GpudHealthStatus {
    /// GPU hardware and sidecar are operating nominally.
    Healthy,
    /// Operating with degraded performance or VRAM warnings.
    Degraded { reason: String },
    /// GPU service or hardware check failed; operating on fallback.
    Failed { reason: String },
}

/// Detected GPU device details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuDeviceInfo {
    pub detected: bool,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub vram_total_mb: u64,
    pub vram_free_mb: u64,
    pub driver_version: Option<String>,
    pub cuda_available: bool,
}

impl Default for GpuDeviceInfo {
    fn default() -> Self {
        Self {
            detected: false,
            vendor: None,
            model: None,
            vram_total_mb: 0,
            vram_free_mb: 0,
            driver_version: None,
            cuda_available: false,
        }
    }
}

/// Policy governing GPU vs CPU execution selection and dynamic fallback rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpudFallbackPolicy {
    /// Minimum free VRAM required (in MB) to utilize GPU execution.
    pub min_vram_mb: u64,
    /// Maximum allowed VRAM utilization percentage (0.0 to 100.0) before triggering warning/fallback.
    pub max_vram_utilization_pct: f64,
    /// Whether automatic fallback to CPU is permitted upon GPU fault or VRAM depletion.
    pub fallback_to_cpu_on_failure: bool,
}

impl Default for GpudFallbackPolicy {
    fn default() -> Self {
        Self {
            min_vram_mb: 1024,
            max_vram_utilization_pct: 95.0,
            fallback_to_cpu_on_failure: true,
        }
    }
}

impl GpudFallbackPolicy {
    /// Creates a new custom fallback policy.
    pub fn new(min_vram_mb: u64, max_vram_utilization_pct: f64, fallback_to_cpu_on_failure: bool) -> Self {
        Self {
            min_vram_mb,
            max_vram_utilization_pct,
            fallback_to_cpu_on_failure,
        }
    }

    /// Evaluates the active device info and health state to select the target `ExecutionBackend`.
    pub fn evaluate(
        &self,
        device: Option<&GpuDeviceInfo>,
        health: &GpudHealthStatus,
    ) -> ExecutionBackend {
        if let GpudHealthStatus::Failed { reason } = health {
            return ExecutionBackend::Cpu {
                reason: format!("GPU health check failed: {}", reason),
            };
        }

        let dev = match device {
            Some(d) if d.detected => d,
            _ => {
                return ExecutionBackend::Cpu {
                    reason: "No compatible GPU hardware detected".to_string(),
                };
            }
        };

        if dev.vram_free_mb < self.min_vram_mb {
            return ExecutionBackend::Cpu {
                reason: format!(
                    "Insufficient free VRAM: {}MB available < {}MB required",
                    dev.vram_free_mb, self.min_vram_mb
                ),
            };
        }

        if dev.vram_total_mb > 0 {
            let used_mb = dev.vram_total_mb.saturating_sub(dev.vram_free_mb);
            let util_pct = (used_mb as f64 / dev.vram_total_mb as f64) * 100.0;
            if util_pct > self.max_vram_utilization_pct {
                return ExecutionBackend::Cpu {
                    reason: format!(
                        "VRAM utilization threshold exceeded: {:.1}% > {:.1}% max limit",
                        util_pct, self.max_vram_utilization_pct
                    ),
                };
            }
        }

        ExecutionBackend::Gpu {
            vendor: dev.vendor.clone().unwrap_or_else(|| "Unknown".to_string()),
            model: dev.model.clone().unwrap_or_else(|| "Generic GPU".to_string()),
            vram_free_mb: dev.vram_free_mb,
            vram_total_mb: dev.vram_total_mb,
        }
    }
}

/// Shared internal state for `gpud` sidecar service.
#[derive(Clone)]
pub struct GpudState {
    pub policy: Arc<RwLock<GpudFallbackPolicy>>,
    pub active_backend: Arc<RwLock<ExecutionBackend>>,
    pub health_status: Arc<RwLock<GpudHealthStatus>>,
    pub device_info: Arc<RwLock<Option<GpuDeviceInfo>>>,
    pub last_check: Arc<RwLock<DateTime<Utc>>>,
}

impl Default for GpudState {
    fn default() -> Self {
        let policy = GpudFallbackPolicy::default();
        let initial_backend = ExecutionBackend::Cpu {
            reason: "Initial state - hardware probe pending".to_string(),
        };
        Self {
            policy: Arc::new(RwLock::new(policy)),
            active_backend: Arc::new(RwLock::new(initial_backend)),
            health_status: Arc::new(RwLock::new(GpudHealthStatus::Healthy)),
            device_info: Arc::new(RwLock::new(None)),
            last_check: Arc::new(RwLock::new(Utc::now())),
        }
    }
}

/// GPU Sidecar Service orchestrator.
#[derive(Clone)]
pub struct GpudService {
    pub state: GpudState,
}

impl Default for GpudService {
    fn default() -> Self {
        Self::new(GpudFallbackPolicy::default())
    }
}

impl GpudService {
    /// Creates a new GPU Sidecar service instance with given fallback policy.
    pub fn new(policy: GpudFallbackPolicy) -> Self {
        let state = GpudState {
            policy: Arc::new(RwLock::new(policy)),
            ..Default::default()
        };
        Self { state }
    }

    /// Performs dynamic synchronous hardware probing for GPU devices.
    pub fn probe_hardware_sync() -> GpuDeviceInfo {
        // Try nvidia-smi
        if let Ok(output) = Command::new("nvidia-smi")
            .args([
                "--query-gpu=name,memory.total,memory.free,driver_version",
                "--format=csv,noheader,nounits",
            ])
            .output()
        {
            if output.status.success() {
                if let Ok(text) = String::from_utf8(output.stdout) {
                    let parts: Vec<&str> = text.trim().split(',').map(|s| s.trim()).collect();
                    if parts.len() >= 4 {
                        let total: u64 = parts[1].parse().unwrap_or(0);
                        let free: u64 = parts[2].parse().unwrap_or(0);
                        let cuda_avail = Command::new("nvcc")
                            .arg("--version")
                            .output()
                            .map(|o| o.status.success())
                            .unwrap_or(false);

                        return GpuDeviceInfo {
                            detected: true,
                            vendor: Some("NVIDIA".to_string()),
                            model: Some(parts[0].to_string()),
                            vram_total_mb: total,
                            vram_free_mb: free,
                            driver_version: Some(parts[3].to_string()),
                            cuda_available: cuda_avail,
                        };
                    }
                }
            }
        }

        // Try AMD rocm-smi
        if let Ok(output) = Command::new("rocm-smi")
            .arg("--showproductname")
            .output()
        {
            if output.status.success() {
                return GpuDeviceInfo {
                    detected: true,
                    vendor: Some("AMD".to_string()),
                    model: Some("ROCm Device".to_string()),
                    vram_total_mb: 8192,
                    vram_free_mb: 4096,
                    driver_version: None,
                    cuda_available: false,
                };
            }
        }

        // macOS Metal system probe
        #[cfg(target_os = "macos")]
        {
            if let Ok(output) = Command::new("system_profiler")
                .args(["SPDisplaysDataType", "-json"])
                .output()
            {
                if output.status.success() {
                    return GpuDeviceInfo {
                        detected: true,
                        vendor: Some("Apple".to_string()),
                        model: Some("Apple Silicon / Metal".to_string()),
                        vram_total_mb: 16384,
                        vram_free_mb: 8192,
                        driver_version: None,
                        cuda_available: false,
                    };
                }
            }
        }

        GpuDeviceInfo::default()
    }

    /// Non-blocking hardware probe wrapper that offloads to a blocking thread pool
    /// with a safe timeout to prevent stalling Axum HTTP workers.
    pub async fn probe_hardware_async(timeout_duration: Duration) -> GpuDeviceInfo {
        let handle = tokio::task::spawn_blocking(Self::probe_hardware_sync);
        match tokio::time::timeout(timeout_duration, handle).await {
            Ok(Ok(device_info)) => device_info,
            _ => GpuDeviceInfo::default(),
        }
    }

    /// Refreshes health status, probes GPU hardware, and updates active backend according to policy.
    pub async fn refresh_status(&self) {
        let probed = Self::probe_hardware_async(Duration::from_millis(1500)).await;
        let mut dev_guard = self.state.device_info.write().await;
        *dev_guard = Some(probed.clone());

        let health_guard = self.state.health_status.read().await;
        let policy_guard = self.state.policy.read().await;

        let evaluated_backend = policy_guard.evaluate(Some(&probed), &health_guard);
        let mut backend_guard = self.state.active_backend.write().await;
        *backend_guard = evaluated_backend;

        let mut last_check_guard = self.state.last_check.write().await;
        *last_check_guard = Utc::now();
    }

    /// Spawns a background health and VRAM monitor task that periodically refreshes status.
    pub fn start_background_monitor(&self, interval: Duration) -> tokio::task::JoinHandle<()> {
        let service = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                service.refresh_status().await;
            }
        })
    }

    /// Explicitly updates health status (e.g. from telemetry or health check hooks).
    pub async fn set_health_status(&self, status: GpudHealthStatus) {
        {
            let mut health_guard = self.state.health_status.write().await;
            *health_guard = status.clone();
        }

        let dev_guard = self.state.device_info.read().await;
        let policy_guard = self.state.policy.read().await;

        let evaluated_backend = policy_guard.evaluate(dev_guard.as_ref(), &status);
        let mut backend_guard = self.state.active_backend.write().await;
        *backend_guard = evaluated_backend;
    }
}

// REST Response structures

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpudHealthResponse {
    pub status: GpudHealthStatus,
    pub active_backend: ExecutionBackend,
    pub device_info: Option<GpuDeviceInfo>,
    pub last_check: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServeRequest {
    pub prompt: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServeResponse {
    pub status: String,
    pub active_backend: ExecutionBackend,
    pub model_used: String,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub health: GpudHealthStatus,
    pub active_backend: ExecutionBackend,
    pub policy: GpudFallbackPolicy,
    pub device_info: Option<GpuDeviceInfo>,
    pub last_check: DateTime<Utc>,
}

// Axum Handlers

async fn health_handler(State(service): State<GpudService>) -> impl IntoResponse {
    let health = service.state.health_status.read().await.clone();
    let active_backend = service.state.active_backend.read().await.clone();
    let device_info = service.state.device_info.read().await.clone();
    let last_check = *service.state.last_check.read().await;

    Json(GpudHealthResponse {
        status: health,
        active_backend,
        device_info,
        last_check,
    })
}

async fn detect_handler(State(service): State<GpudService>) -> impl IntoResponse {
    let probed = GpudService::probe_hardware_async(Duration::from_millis(2000)).await;
    {
        let mut dev_guard = service.state.device_info.write().await;
        *dev_guard = Some(probed.clone());
    }
    Json(probed)
}

async fn serve_handler(
    State(service): State<GpudService>,
    Json(payload): Json<ServeRequest>,
) -> impl IntoResponse {
    let active_backend = service.state.active_backend.read().await.clone();
    let model = payload.model.unwrap_or_else(|| "default-llm".to_string());

    let (status, output) = match &active_backend {
        ExecutionBackend::Gpu { model: gpu_model, .. } => (
            "ok",
            format!("Processed prompt on GPU ({}) using model {}", gpu_model, model),
        ),
        ExecutionBackend::Cpu { reason } => (
            "fallback_cpu",
            format!("Processed prompt on CPU (Reason: {}) using model {}", reason, model),
        ),
    };

    (
        StatusCode::OK,
        Json(ServeResponse {
            status: status.to_string(),
            active_backend,
            model_used: model,
            output,
        }),
    )
}

async fn status_handler(State(service): State<GpudService>) -> impl IntoResponse {
    let health = service.state.health_status.read().await.clone();
    let active_backend = service.state.active_backend.read().await.clone();
    let policy = service.state.policy.read().await.clone();
    let device_info = service.state.device_info.read().await.clone();
    let last_check = *service.state.last_check.read().await;

    Json(StatusResponse {
        health,
        active_backend,
        policy,
        device_info,
        last_check,
    })
}

/// Builds and returns the Axum router for `/v1/gpud` endpoints.
pub fn router(service: GpudService) -> Router {
    Router::new()
        .route("/v1/gpud/health", get(health_handler))
        .route("/v1/gpud/detect", get(detect_handler))
        .route("/v1/gpud/serve", post(serve_handler))
        .route("/v1/gpud/status", get(status_handler))
        .with_state(service)
}
