//! # CodeGraph Port — Intelligence & Maturity Interface
//!
//! Defines the core interface for code intelligence plugins and built-in
//! scanners. Used for codebase maturity tracking and design verification.

use std::path::Path;
use serde::{Deserialize, Serialize};
use async_trait::async_trait;

/// Core interface for code intelligence providers.
#[async_trait]
pub trait CodeGraphPort: Send + Sync {
    /// Scan maturity based on a feature manifest.
    async fn scan_maturity(&self, features_path: &Path, codebase_root: &Path) -> Result<ScanResult, String>;

    /// Verify design for a specific feature or the entire manifest.
    async fn verify_design(&self, features_path: &Path, codebase_root: &Path, feature_id: Option<&str>) -> Result<DesignVerification, String>;

    /// Identify missing design elements (gaps).
    async fn design_gaps(&self, features_path: &Path, codebase_root: &Path) -> Result<Vec<DesignGap>, String>;

    /// Check plugin health and capabilities.
    async fn health(&self) -> PluginStatus;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub feature_scans: Vec<FeatureScan>,
    pub total_checks: usize,
    pub total_found: usize,
    pub errors: Vec<String>,
    pub timing_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureScan {
    pub feature_id: String,
    pub found: Vec<String>,
    pub missing: Vec<String>,
    pub maturity_pct: f64,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignVerification {
    pub target: String,
    pub total_symbols: usize,
    pub verified: usize,
    pub gaps: Vec<String>,
    pub status: DesignStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DesignStatus {
    Verified,
    Partial,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignGap {
    pub symbol: String,
    pub context: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginStatus {
    pub available: bool,
    pub version: Option<String>,
    pub uptime_secs: Option<u64>,
    pub features: Vec<String>,
}
