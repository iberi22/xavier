//! # Axum Middleware for Request Logging
//!
//! Captures every HTTP request and response, logs them to both:
//! - `tracing!` (stdout + file via tracing-subscriber)
//! - `ServiceLogStore` (persistent SQLite) when there's a server error
//!
//! ## Usage
//!
//! ```rust,ignore
//! use observability::middleware::{request_logger, ObservabilityState};
//!
//! let obs_state = Arc::new(ObservabilityState::new());
//!
//! Router::new()
//!     .route("/api/...", get(handler))
//!     .layer(axum::middleware::from_fn_with_state(obs_state, request_logger))
//! ```

use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use tokio::sync::OnceCell;

use super::service_log::{LogEntry, LogSource, ServiceLogStore};

/// Observability middleware state.
#[derive(Clone)]
pub struct ObservabilityState {
    /// Lazily-initialized log store. The `OnceCell` guarantees the store is
    /// created exactly once, on the async runtime, without `block_in_place`.
    store: Arc<OnceCell<Option<ServiceLogStore>>>,
    pub app_start_time: Instant,
}

impl ObservabilityState {
    /// Create a new state. The `ServiceLogStore` is NOT constructed here —
    /// that would require `block_in_place`/`block_on` which can deadlock the
    /// tokio runtime in release builds. It initializes lazily on first use.
    pub fn new() -> Self {
        Self {
            store: Arc::new(OnceCell::new()),
            app_start_time: Instant::now(),
        }
    }

    /// Lazily initialize (and cache) the store on the async runtime.
    pub async fn store(&self) -> Option<&ServiceLogStore> {
        let cell = self
            .store
            .get_or_init(|| async { ServiceLogStore::new().await.ok() })
            .await;
        cell.as_ref()
    }

    /// Get uptime in seconds.
    pub fn uptime_seconds(&self) -> u64 {
        self.app_start_time.elapsed().as_secs()
    }
}

impl Default for ObservabilityState {
    fn default() -> Self {
        Self::new()
    }
}

/// Axum middleware that logs each request and response.
///
/// Captures: method, URI, status code, latency.
/// Server errors (5xx) are also logged to the ServiceLogStore.
pub async fn request_logger(
    State(state): State<Arc<ObservabilityState>>,
    req: Request,
    next: Next,
) -> Response {
    let start = Instant::now();
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    tracing::info!(method = %method, path = %path, "â†’ incoming request");

    let response = next.run(req).await;
    let latency = start.elapsed();
    let status = response.status();

    tracing::info!(
        method = %method,
        path = %path,
        status = %status.as_u16(),
        latency_ms = latency.as_millis() as u64,
        "â† response sent"
    );

    // Log server errors to persistent store (lazily initialized on first 5xx).
    if status.is_server_error() {
        if let Some(store) = state.store().await {
            let entry = LogEntry::error(
                LogSource::HttpServer,
                &format!("http{}", path.replace('/', "::")),
                &format!("HTTP {} → {} ({}ms)", method, status, latency.as_millis()),
            )
            .with_metadata(serde_json::json!({
                "method": method.to_string(),
                "path": path,
                "status": status.as_u16(),
                "latency_ms": latency.as_millis(),
            }));

            let store_clone = store.clone();
            tokio::spawn(async move {
                if let Err(e) = store_clone.log(entry).await {
                    tracing::warn!("Failed to log server error: {}", e);
                }
            });
        }
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_observability_state_creation() {
        let state = ObservabilityState::new();
        // The store is lazily initialized — it only materializes on first use.
        // app_start_time should be set.
        assert!(state.uptime_seconds() < 100); // just started
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_observability_state_uptime() {
        let state = ObservabilityState::new();
        // Uptime should increase as time passes
        let u1 = state.uptime_seconds();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let u2 = state.uptime_seconds();
        assert!(u2 >= u1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_observability_state_default() {
        let state = ObservabilityState::default();
        assert!(state.uptime_seconds() < 100);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_observability_state_clone() {
        let state = ObservabilityState::new();
        let cloned = state.clone();
        assert_eq!(state.uptime_seconds(), cloned.uptime_seconds());
    }
}
