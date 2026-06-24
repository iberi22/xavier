use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticCheck {
    pub symbol: String,
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_required() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubcomponentAnchor {
    pub name: String,
    pub weight: u32,
    #[serde(default = "default_max")]
    pub max_maturity: u8,
    #[serde(default)]
    pub static_checks: Vec<StaticCheck>,
    pub required_feature: Option<String>,
    #[serde(default)]
    pub test_anchors: Vec<String>,
    #[serde(default)]
    pub memory_keywords: Vec<String>,
    #[serde(default)]
    pub issue_labels: Vec<String>,
}

fn default_max() -> u8 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureAnchor {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub priority: String,
    pub subcomponents: Vec<SubcomponentAnchor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnchorManifest {
    pub features: Vec<FeatureAnchor>,
}
