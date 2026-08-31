//! SnippetWriteThrough — CodeGraph → Memory auto-sync (WAVE-3.06)
//!
//! Activates unified writethrough: when code-graph indexes a file, its snippets
//! are automatically persisted as MemoryRecords with code-graph provenance.
//! Cascade delete removes memory entries when source file is deleted.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// CodeGraph snippet writethrough config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnippetWriteThroughConfig {
    pub enabled: bool,
    pub workspace: String,
    pub snippet_zone: String,
    pub max_snippet_chars: usize,
    pub cascade_delete: bool,
}

impl Default for SnippetWriteThroughConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            workspace: "default".to_string(),
            snippet_zone: "code-graph".to_string(),
            max_snippet_chars: 4000,
            cascade_delete: true,
        }
    }
}

/// Provenance tracking for code-graph sourced memories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnippetProvenance {
    pub file_path: String,
    pub symbol: Option<String>,
    pub language: String,
    pub snippet_id: String,
    pub code_graph_hash: String,
}

/// SnippetWriteThrough — bridges code-graph indexer and MemoryStore
pub struct SnippetWriteThrough {
    config: SnippetWriteThroughConfig,
    // In-memory index of file → snippet ids (for cascade delete)
    file_index: RwLock<HashMap<String, Vec<String>>>,
    // In-memory store of snippet provenance (mirrors what's in MemoryStore metadata)
    provenance: RwLock<HashMap<String, SnippetProvenance>>,
}

impl SnippetWriteThrough {
    pub fn new(config: SnippetWriteThroughConfig) -> Self {
        Self {
            config,
            file_index: RwLock::new(HashMap::new()),
            provenance: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_defaults(workspace: &str) -> Self {
        let cfg = SnippetWriteThroughConfig {
            workspace: workspace.to_string(),
            ..Default::default()
        };
        Self::new(cfg)
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn config(&self) -> &SnippetWriteThroughConfig {
        &self.config
    }

    /// Called when code-graph indexes a file — produces memory records
    pub async fn on_file_indexed(
        &self,
        file_path: &str,
        language: &str,
        snippets: Vec<(String, String, Option<String>)>,
        // (snippet_id, content, symbol)
    ) -> Result<Vec<CodeGraphSnippetRecord>> {
        let mut records = Vec::new();
        let mut file_index = self.file_index.write().await;
        let mut provenance = self.provenance.write().await;

        let ids: Vec<String> = snippets.iter().map(|(id, _, _)| id.clone()).collect();
        file_index.insert(file_path.to_string(), ids.clone());

        for (snippet_id, content, symbol) in snippets {
            let clipped = if content.len() > self.config.max_snippet_chars {
                content[..self.config.max_snippet_chars].to_string()
            } else {
                content
            };
            let prov = SnippetProvenance {
                file_path: file_path.to_string(),
                symbol: symbol.clone(),
                language: language.to_string(),
                snippet_id: snippet_id.clone(),
                code_graph_hash: format!("{:x}", snippet_id.len() * 31 + file_path.len()),
            };
            provenance.insert(snippet_id.clone(), prov.clone());

            records.push(CodeGraphSnippetRecord {
                id: snippet_id,
                content: clipped,
                workspace: self.config.workspace.clone(),
                zone: self.config.snippet_zone.clone(),
                file_path: file_path.to_string(),
                language: language.to_string(),
                symbol,
                provenance: prov,
            });
        }
        Ok(records)
    }

    /// Called when a file is deleted — returns snippet ids to cascade delete
    pub async fn on_file_deleted(&self, file_path: &str) -> Vec<String> {
        if !self.config.cascade_delete {
            return Vec::new();
        }
        let mut file_index = self.file_index.write().await;
        let mut provenance = self.provenance.write().await;
        let ids = file_index.remove(file_path).unwrap_or_default();
        for id in &ids {
            provenance.remove(id);
        }
        ids
    }

    /// Lookup provenance for a snippet
    pub async fn provenance_for(&self, snippet_id: &str) -> Option<SnippetProvenance> {
        self.provenance.read().await.get(snippet_id).cloned()
    }

    /// List all tracked files
    pub async fn tracked_files(&self) -> Vec<String> {
        self.file_index.read().await.keys().cloned().collect()
    }

    /// Count tracked snippets
    pub async fn snippet_count(&self) -> usize {
        self.provenance.read().await.len()
    }
}

/// Record produced by SnippetWriteThrough for MemoryStore insertion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphSnippetRecord {
    pub id: String,
    pub content: String,
    pub workspace: String,
    pub zone: String,
    pub file_path: String,
    pub language: String,
    pub symbol: Option<String>,
    pub provenance: SnippetProvenance,
}

impl CodeGraphSnippetRecord {
    pub fn as_memory_metadata(&self) -> serde_json::Value {
        serde_json::json!({
            "code_graph": true,
            "file_path": self.file_path,
            "language": self.language,
            "symbol": self.symbol,
            "snippet_id": self.id,
            "code_graph_hash": self.provenance.code_graph_hash,
            "zone": self.zone,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_snippet_writethrough_index() {
        let swt = SnippetWriteThrough::with_defaults("ws1");
        assert!(swt.is_enabled());
        let recs = swt
            .on_file_indexed(
                "src/main.rs",
                "rust",
                vec![
                    ("s1".into(), "fn main() {}".into(), Some("main".into())),
                    ("s2".into(), "struct Foo {}".into(), Some("Foo".into())),
                ],
            )
            .await
            .unwrap();
        assert_eq!(recs.len(), 2);
        assert_eq!(swt.snippet_count().await, 2);
        assert_eq!(swt.tracked_files().await, vec!["src/main.rs"]);
        let prov = swt.provenance_for("s1").await.unwrap();
        assert_eq!(prov.language, "rust");
    }

    #[tokio::test]
    async fn test_snippet_cascade_delete() {
        let swt = SnippetWriteThrough::with_defaults("ws1");
        let _ = swt
            .on_file_indexed(
                "src/lib.rs",
                "rust",
                vec![("s1".into(), "code".into(), None)],
            )
            .await
            .unwrap();
        let deleted = swt.on_file_deleted("src/lib.rs").await;
        assert_eq!(deleted, vec!["s1"]);
        assert_eq!(swt.snippet_count().await, 0);
        assert!(swt.tracked_files().await.is_empty());
    }

    #[tokio::test]
    async fn test_snippet_clipping() {
        let cfg = SnippetWriteThroughConfig {
            max_snippet_chars: 10,
            ..Default::default()
        };
        let swt = SnippetWriteThrough::new(cfg);
        let recs = swt
            .on_file_indexed(
                "f.rs",
                "rust",
                vec![("s1".into(), "0123456789ABCDEF".into(), None)],
            )
            .await
            .unwrap();
        assert_eq!(recs[0].content.len(), 10);
    }

    #[test]
    fn test_codegraph_record_metadata() {
        let rec = CodeGraphSnippetRecord {
            id: "s1".into(),
            content: "code".into(),
            workspace: "ws".into(),
            zone: "code-graph".into(),
            file_path: "src/main.rs".into(),
            language: "rust".into(),
            symbol: Some("main".into()),
            provenance: SnippetProvenance {
                file_path: "src/main.rs".into(),
                symbol: Some("main".into()),
                language: "rust".into(),
                snippet_id: "s1".into(),
                code_graph_hash: "abc".into(),
            },
        };
        let meta = rec.as_memory_metadata();
        assert_eq!(meta["code_graph"], true);
        assert_eq!(meta["symbol"], "main");
    }
}
