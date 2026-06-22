//! # Anchor Definitions
//!
//! Defines the data structures for feature anchors — the mapping between
//! features, their subcomponents, required symbols, and validating tests.

use serde::{Deserialize, Serialize};

/// Static check: a symbol that must exist in the codebase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticCheck {
    /// Fully qualified symbol name, e.g. `DaoGovernanceSystem`
    pub symbol: String,
    /// Whether this symbol is strictly required
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_required() -> bool {
    true
}

/// One test anchor: a test that validates a subcomponent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestAnchor {
    /// Fully qualified test name, e.g. `mesh::governance::tests::test_dao_governance_consensus`
    pub name: String,
    /// Description of what this test validates
    #[serde(default)]
    pub description: String,
}

/// A subcomponent anchor for a single feature piece.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubcomponentAnchor {
    /// Human-readable name
    pub name: String,
    /// Weight (0-100) — contribution to the parent feature's overall score.
    /// Sum of all subcomponent weights in a feature should be 100.
    pub weight: u32,
    /// Maximum possible maturity for this subcomponent (0-100).
    #[serde(default = "default_max")]
    pub max_maturity: u8,
    /// Static code symbols that must exist
    #[serde(default)]
    pub static_checks: Vec<StaticCheck>,
    /// Required cfg(feature = "...") for this subcomponent, if any
    pub required_feature: Option<String>,
    /// Tests that validate this subcomponent
    #[serde(default)]
    pub test_anchors: Vec<String>,
    /// Keywords used to search agent memory / sessions for evidence of usage.
    /// Falls back to the feature id + name when empty (v2 deep-scan, Layer 3).
    #[serde(default)]
    pub memory_keywords: Vec<String>,
    /// GitHub issue labels (or .gitcore feature tags) associated with this
    /// subcomponent, used to gauge issue health (v2 deep-scan, Layer 4).
    #[serde(default)]
    pub issue_labels: Vec<String>,
}

fn default_max() -> u8 {
    100
}

/// A feature anchor: one major feature of Xavier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureAnchor {
    /// Feature identifier, e.g. "memory-rag"
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Priority for the sprint
    #[serde(default)]
    pub priority: String,
    /// Subcomponents that make up this feature
    pub subcomponents: Vec<SubcomponentAnchor>,
}

/// Top-level manifest file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnchorManifest {
    /// Schema version
    pub version: String,
    /// When this manifest was generated
    pub generated: String,
    /// All feature anchors
    pub features: Vec<FeatureAnchor>,
}
