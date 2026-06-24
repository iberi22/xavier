//! # HTTP CodeGraph Adapter — Plugin Connection via REST API
//!
//! Connects Xavier to the external `codegraph-plugin` binary.
//! The plugin is a fork of `codegraph-ai/CodeGraph` v0.18.6 that
//! exposes a REST API for code intelligence operations.
//!
//! ## Protocol
//!
//! All requests go to `http://{host}:{port}` as JSON POST bodies.
//! Authorization via `Bearer` token header.
//!
//! ## Graceful Degradation
//!
//! If the plugin is not available (connection refused, timeout, etc.),
//! this adapter returns errors that Xavier uses to fall back to the
//! `FallbackCodeGraphAdapter`.

use async_trait::async_trait;
use std::path::Path;
use std::time::Duration;

use crate::ports::code_graph::{CodeGraphPort, DesignGap, DesignStatus, DesignVerification, FeatureScan, PluginStatus, ScanResult};

/// HTTP adapter connecting Xavier to the codegraph-plugin binary.
pub struct HttpCodeGraphAdapter {
    /// Plugin HTTP server URL (e.g. "http://127.0.0.1:9091")
    base_url: String,
    /// Bearer token for authentication
    token: String,
    /// HTTP client
    client: reqwest::Client,
    /// Whether the plugin is considered available
    available: bool,
}

impl HttpCodeGraphAdapter {
    /// Create a new adapter connecting to the plugin.
    pub fn new(host: &str, port: u16, token: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            base_url: format!("http://{}:{}", host, port),
            token: token.to_string(),
            client,
            available: true,
        }
    }

    /// Create a disabled adapter (plugin not configured).
    pub fn disabled() -> Self {
        Self {
            base_url: String::new(),
            token: String::new(),
            client: reqwest::Client::new(),
            available: false,
        }
    }

    /// Send a JSON request to the plugin.
    async fn post<T: serde::Serialize + Send, R: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<R, String> {
        if !self.available {
            return Err("Plugin not configured".to_string());
        }

        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(body)
            .send()
            .await
            .map_err(|e| format!("Plugin connection failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Plugin returned {}: {}", status, text));
        }

        resp.json().await.map_err(|e| format!("Plugin response parse error: {}", e))
    }
}

// ── Plugin API request/response types ────────────────────

#[derive(serde::Serialize)]
struct ScanMaturityRequest {
    features_path: String,
    codebase_root: String,
}

#[derive(serde::Deserialize)]
struct ScanMaturityResponse {
    feature_scans: Vec<PluginFeatureScan>,
    total_checks: usize,
    total_found: usize,
    errors: Vec<String>,
    timing_ms: u64,
}

#[derive(serde::Deserialize)]
struct PluginFeatureScan {
    feature_id: String,
    found: Vec<String>,
    missing: Vec<String>,
    maturity_pct: f64,
    detail: String,
}

#[derive(serde::Serialize)]
struct VerifyDesignRequest {
    features_path: String,
    codebase_root: String,
    feature_id: Option<String>,
}

#[derive(serde::Deserialize)]
struct VerifyDesignResponse {
    target: String,
    total_symbols: usize,
    verified: usize,
    gaps: Vec<String>,
    status: String,
}

#[derive(serde::Serialize)]
struct DesignGapsRequest {
    features_path: String,
    codebase_root: String,
}

#[derive(serde::Deserialize)]
struct DesignGapsEntry {
    symbol: String,
    context: String,
    suggestion: Option<String>,
}

// ── Implementation ────────────────────────────────────────

#[async_trait]
impl CodeGraphPort for HttpCodeGraphAdapter {
    async fn scan_maturity(
        &self,
        features_path: &Path,
        codebase_root: &Path,
    ) -> Result<ScanResult, String> {
        let resp: ScanMaturityResponse = self
            .post(
                "/api/v1/code/feature-maturity",
                &ScanMaturityRequest {
                    features_path: features_path.to_string_lossy().to_string(),
                    codebase_root: codebase_root.to_string_lossy().to_string(),
                },
            )
            .await?;

        Ok(ScanResult {
            feature_scans: resp
                .feature_scans
                .into_iter()
                .map(|f| FeatureScan {
                    feature_id: f.feature_id,
                    found: f.found,
                    missing: f.missing,
                    maturity_pct: f.maturity_pct,
                    detail: f.detail,
                })
                .collect(),
            total_checks: resp.total_checks,
            total_found: resp.total_found,
            errors: resp.errors,
            timing_ms: resp.timing_ms,
        })
    }

    async fn verify_design(
        &self,
        features_path: &Path,
        codebase_root: &Path,
        feature_id: Option<&str>,
    ) -> Result<DesignVerification, String> {
        let resp: VerifyDesignResponse = self
            .post(
                "/api/v1/code/verify-design",
                &VerifyDesignRequest {
                    features_path: features_path.to_string_lossy().to_string(),
                    codebase_root: codebase_root.to_string_lossy().to_string(),
                    feature_id: feature_id.map(|s| s.to_string()),
                },
            )
            .await?;

        let status = match resp.status.as_str() {
            "verified" => DesignStatus::Verified,
            "partial" => DesignStatus::Partial,
            "failed" => DesignStatus::Failed,
            _ => DesignStatus::Unknown,
        };

        Ok(DesignVerification {
            target: resp.target,
            total_symbols: resp.total_symbols,
            verified: resp.verified,
            gaps: resp.gaps,
            status,
        })
    }

    async fn design_gaps(
        &self,
        features_path: &Path,
        codebase_root: &Path,
    ) -> Result<Vec<DesignGap>, String> {
        let gaps: Vec<DesignGapsEntry> = self
            .post(
                "/api/v1/code/design-gaps",
                &DesignGapsRequest {
                    features_path: features_path.to_string_lossy().to_string(),
                    codebase_root: codebase_root.to_string_lossy().to_string(),
                },
            )
            .await?;

        Ok(gaps
            .into_iter()
            .map(|g| DesignGap {
                symbol: g.symbol,
                context: g.context,
                suggestion: g.suggestion,
            })
            .collect())
    }

    async fn health(&self) -> PluginStatus {
        if !self.available {
            return PluginStatus {
                available: false,
                version: None,
                uptime_secs: None,
                features: vec![],
            };
        }

        match self.post::<serde_json::Value, serde_json::Value>("/health", &serde_json::json!({})).await {
            Ok(resp) => PluginStatus {
                available: true,
                version: resp.get("version").and_then(|v| v.as_str()).map(|s| s.to_string()),
                uptime_secs: resp.get("uptime_secs").and_then(|v| v.as_u64()),
                features: resp
                    .get("features")
                    .and_then(|f| f.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            Err(e) => PluginStatus {
                available: false,
                version: Some(format!("error: {}", e)),
                uptime_secs: None,
                features: vec![],
            },
        }
    }
}

