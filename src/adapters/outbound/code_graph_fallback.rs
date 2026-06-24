use crate::ports::outbound::code_graph::{CodeGraphPort, SymbolInfo};
use anyhow::Result;
use async_trait::async_trait;
use std::fs;
use std::path::Path;

pub struct FallbackCodeGraphAdapter {
    codebase_root: String,
}

impl FallbackCodeGraphAdapter {
    pub fn new(root: &str) -> Self {
        Self {
            codebase_root: root.to_string(),
        }
    }

    fn grep_codebase(&self, pattern: &str) -> bool {
        let walker = walkdir::WalkDir::new(&self.codebase_root)
            .max_depth(8)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "rs"))
            .take(300);

        for entry in walker {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                if content.contains(pattern) {
                    return true;
                }
            }
        }
        false
    }
}

#[async_trait]
impl CodeGraphPort for FallbackCodeGraphAdapter {
    async fn find_symbol(&self, symbol: &str) -> Result<Option<SymbolInfo>> {
        if self.grep_codebase(symbol) {
            Ok(Some(SymbolInfo {
                name: symbol.to_string(),
                kind: "unknown".to_string(),
                location: "fallback".to_string(),
            }))
        } else {
            Ok(None)
        }
    }

    async fn get_gaps(&self, _features_path: &str) -> Result<Vec<String>> {
        // Simple gap detection: if symbols are mentioned in comments but not defined
        // For fallback, we'll just return some placeholders for now but could be improved.
        Ok(vec![])
    }

    async fn verify_design(&self, _feature_id: &str) -> Result<bool> {
        Ok(true)
    }

    async fn check_feature_gate(&self, gate: &str) -> Result<bool> {
        let cargo_path = Path::new(&self.codebase_root).join("Cargo.toml");
        if let Ok(content) = fs::read_to_string(&cargo_path) {
            if content.contains(&format!("{} = [", gate)) || content.contains(&format!("\"{}\"", gate)) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
