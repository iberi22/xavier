use serde::{Deserialize, Serialize};
use crate::graph::Node;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRequest {
    pub version: String,
    pub files: Vec<String>,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginResponse {
    pub version: String,
    pub results: Vec<Node>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub languages: Vec<String>,
    pub capabilities: Vec<String>,
}
