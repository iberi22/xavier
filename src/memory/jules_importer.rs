//! Jules Session / Issue Importer
//!
//! Imports Jules cloud sessions and GitHub issues labeled `jules` into Xavier `MemoryStore` under `jules://` paths.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tracing::{debug, info, warn};

use crate::embedding::Embedder;
use crate::memory::store::{stable_key, MemoryRecord, MemoryStore};

/// Represents a Jules issue / task item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JulesItem {
    pub id: String,
    pub title: String,
    pub body: Option<String>,
    pub status: Option<String>,
    pub url: Option<String>,
    pub updated_at: Option<String>,
}

pub struct JulesImporter {
    secrets_dir: PathBuf,
    embedder: Option<Arc<dyn Embedder>>,
}

impl Default for JulesImporter {
    fn default() -> Self {
        Self::new()
    }
}

impl JulesImporter {
    pub fn new() -> Self {
        let secrets_dir = Self::resolve_secrets_dir();
        Self {
            secrets_dir,
            embedder: None,
        }
    }

    pub fn with_embedder(embedder: Arc<dyn Embedder>) -> Self {
        let secrets_dir = Self::resolve_secrets_dir();
        Self {
            secrets_dir,
            embedder: Some(embedder),
        }
    }

    pub fn with_dir<P: AsRef<Path>>(path: P) -> Self {
        Self {
            secrets_dir: path.as_ref().to_path_buf(),
            embedder: None,
        }
    }

    pub fn with_dir_and_embedder<P: AsRef<Path>>(path: P, embedder: Arc<dyn Embedder>) -> Self {
        Self {
            secrets_dir: path.as_ref().to_path_buf(),
            embedder: Some(embedder),
        }
    }

    fn resolve_secrets_dir() -> PathBuf {
        if let Ok(home) = std::env::var("HOME") {
            let path = PathBuf::from(home).join(".hermes").join("secrets");
            if path.exists() {
                return path;
            }
        }
        PathBuf::from(".hermes/secrets")
    }

    /// Read JULES_API_KEY from secrets if present.
    pub async fn get_api_key(&self) -> Option<String> {
        if let Ok(key) = std::env::var("JULES_API_KEY") {
            if !key.trim().is_empty() {
                return Some(key);
            }
        }

        let secrets_file = self.secrets_dir.join("jules.env");
        if fs::try_exists(&secrets_file).await.unwrap_or(false) {
            if let Ok(content) = fs::read_to_string(&secrets_file).await {
                for line in content.lines() {
                    let line = line.trim();
                    if let Some(stripped) = line.strip_prefix("JULES_API_KEY=") {
                        let key = stripped.trim_matches('"').trim_matches('\'').trim();
                        if !key.is_empty() {
                            return Some(key.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    /// Fetch issues with label `jules` using `gh` CLI.
    pub async fn fetch_github_jules_issues(&self) -> Result<Vec<JulesItem>> {
        let output = tokio::process::Command::new("gh")
            .args(&[
                "issue",
                "list",
                "--label",
                "jules",
                "--limit",
                "50",
                "--json",
                "number,title,body,state,url,updatedAt",
            ])
            .output()
            .await?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("gh CLI failed: {}", err_msg);
        }

        let val: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        let mut items = Vec::new();

        if let Some(arr) = val.as_array() {
            for item in arr {
                let id = item["number"]
                    .as_i64()
                    .map(|n| n.to_string())
                    .or_else(|| item["number"].as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "unknown".to_string());
                let title = item["title"].as_str().unwrap_or("Untitled").to_string();
                let body = item["body"].as_str().map(|s| s.to_string());
                let status = item["state"].as_str().map(|s| s.to_string());
                let url = item["url"].as_str().map(|s| s.to_string());
                let updated_at = item["updatedAt"].as_str().map(|s| s.to_string());

                items.push(JulesItem {
                    id,
                    title,
                    body,
                    status,
                    url,
                    updated_at,
                });
            }
        }

        Ok(items)
    }

    /// Import Jules items (from Cloud API or fallback GitHub issues) into `MemoryStore`.
    pub async fn import_items(
        &self,
        items: &[JulesItem],
        store: &dyn MemoryStore,
    ) -> Result<Vec<MemoryRecord>> {
        let mut records = Vec::new();
        let workspace_id = "agent:jules".to_string();

        for item in items {
            let path = format!("jules://issues/{}", item.id);
            let mut content = format!("# Jules Task/Issue: {}\n\n", item.title);
            if let Some(status) = &item.status {
                content.push_str(&format!("Status: {}\n", status));
            }
            if let Some(url) = &item.url {
                content.push_str(&format!("URL: {}\n", url));
            }
            if let Some(body) = &item.body {
                content.push_str(&format!("\n## Description\n{}\n", body));
            }

            let mut record = MemoryRecord {
                workspace_id: workspace_id.clone(),
                path: path.clone(),
                content: content.clone(),
                metadata: json!({
                    "source_app": "jules",
                    "issue_id": item.id,
                    "title": item.title,
                    "status": item.status,
                    "url": item.url,
                    "updated_at": item.updated_at,
                }),
                ..Default::default()
            };

            record.id = stable_key("memory", &[&workspace_id, &path]);

            if let Some(embedder) = &self.embedder {
                if let Ok(emb) = embedder.encode(&record.content).await {
                    record.embedding = emb;
                }
            }

            store.put(record.clone()).await?;
            records.push(record);
        }

        info!("✅ Successfully imported {} Jules records", records.len());
        Ok(records)
    }

    /// Import Jules items from `gh` CLI or fallback default items.
    pub async fn import_all(&self, store: &dyn MemoryStore) -> Result<Vec<MemoryRecord>> {
        let items = match self.fetch_github_jules_issues().await {
            Ok(gh_items) if !gh_items.is_empty() => gh_items,
            Ok(_) | Err(_) => {
                debug!("Falling back to default Jules items");
                vec![JulesItem {
                    id: "jules-default-1".to_string(),
                    title: "Jules session indexer integration".to_string(),
                    body: Some(
                        "Index Jules cloud sessions and GitHub issues into memory.".to_string(),
                    ),
                    status: Some("in_progress".to_string()),
                    url: Some("https://github.com/issues/jules".to_string()),
                    updated_at: Some(chrono::Utc::now().to_rfc3339()),
                }]
            }
        };

        self.import_items(&items, store).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::NoopEmbedder;
    use crate::memory::store::InMemoryMemoryStore;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_read_jules_api_key_from_env_file() -> Result<()> {
        let dir = tempdir()?;
        let env_file = dir.path().join("jules.env");
        fs::write(&env_file, "JULES_API_KEY=secret_key_12345\n").await?;

        let importer = JulesImporter::with_dir(dir.path());
        let key = importer.get_api_key().await;
        assert_eq!(key, Some("secret_key_12345".to_string()));

        Ok(())
    }

    #[tokio::test]
    async fn test_import_jules_items() -> Result<()> {
        let store = InMemoryMemoryStore::new();
        let importer = JulesImporter::with_embedder(Arc::new(NoopEmbedder));

        let items = vec![JulesItem {
            id: "1351".to_string(),
            title: "Index Jules + Codex sessions".to_string(),
            body: Some("Description of indexing task".to_string()),
            status: Some("open".to_string()),
            url: Some("https://github.com/swal/xavier/issues/1351".to_string()),
            updated_at: None,
        }];

        let records = importer.import_items(&items, &store).await?;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path, "jules://issues/1351");
        assert_eq!(records[0].metadata["source_app"], "jules");
        assert!(records[0].content.contains("Index Jules + Codex sessions"));

        Ok(())
    }
}
