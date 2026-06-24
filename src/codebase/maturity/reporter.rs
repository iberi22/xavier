use crate::codebase::maturity::scorer::ScoredFeature;
use serde_json::json;
use std::path::Path;
use anyhow::Result;

pub struct MaturityReporter {
    codebase_root: String,
}

impl MaturityReporter {
    pub fn new(root: &str) -> Self {
        Self {
            codebase_root: root.to_string(),
        }
    }

    pub fn generate_report(&self, results: &[ScoredFeature]) -> Result<()> {
        let report = json!({
            "scanned_at": chrono::Utc::now().to_rfc3339(),
            "features": results,
            "version": "1.0.0"
        });
        let path = Path::new(&self.codebase_root).join("feature-maturity.json");
        std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
        Ok(())
    }
}
