//! Code graph port interface.

use async_trait::async_trait;
use std::path::Path;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct FeatureScan {
    pub feature_id: String,
    pub found: Vec<String>,
    pub missing: Vec<String>,
    pub maturity_pct: f64,
    pub detail: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct ScanResult {
    pub feature_scans: Vec<FeatureScan>,
    pub total_checks: usize,
    pub total_found: usize,
    pub errors: Vec<String>,
    pub timing_ms: u64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq)]
pub enum DesignStatus {
    Verified,
    Partial,
    Failed,
    Unknown,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct DesignVerification {
    pub target: String,
    pub total_symbols: usize,
    pub verified: usize,
    pub gaps: Vec<String>,
    pub status: DesignStatus,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct DesignGap {
    pub symbol: String,
    pub context: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct PluginStatus {
    pub available: bool,
    pub version: Option<String>,
    pub uptime_secs: Option<u64>,
    pub features: Vec<String>,
}

#[async_trait]
pub trait CodeGraphPort: Send + Sync {
    async fn scan_maturity(
        &self,
        features_path: &Path,
        codebase_root: &Path,
    ) -> Result<ScanResult, String>;

    async fn verify_design(
        &self,
        features_path: &Path,
        codebase_root: &Path,
        feature_id: Option<&str>,
    ) -> Result<DesignVerification, String>;

    async fn design_gaps(
        &self,
        features_path: &Path,
        codebase_root: &Path,
    ) -> Result<Vec<DesignGap>, String>;

    async fn health(&self) -> PluginStatus;
}
