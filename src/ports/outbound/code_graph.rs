use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: String,
    pub location: String,
}

#[async_trait]
pub trait CodeGraphPort: Send + Sync {
    async fn find_symbol(&self, symbol: &str) -> Result<Option<SymbolInfo>>;
    async fn get_gaps(&self, features_path: &str) -> Result<Vec<String>>;
    async fn verify_design(&self, feature_id: &str) -> Result<bool>;
    async fn check_feature_gate(&self, gate: &str) -> Result<bool>;
}
