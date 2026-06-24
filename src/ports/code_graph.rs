use async_trait::async_trait;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DesignGap {
    pub name: String,
    pub description: String,
    pub severity: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DesignStatus {
    pub name: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DriftReport {
    pub name: String,
    pub drift: f64,
}

#[async_trait]
pub trait CodeGraphPort: Send + Sync {
    async fn find_design_gaps(&self) -> anyhow::Result<Vec<DesignGap>>;
    async fn check_design_status(&self) -> anyhow::Result<Vec<DesignStatus>>;
    async fn analyze_config_drift(&self) -> anyhow::Result<Vec<DriftReport>>;
}
