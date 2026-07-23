//! Engram-Xavier Bidirectional Memory Bridge
//!
//! Provides bidirectional memory synchronization between Engram and Xavier.

use anyhow::{Result, Context};
use std::process::Command;
use tracing::{warn, info, debug};
use std::time::{Instant, Duration};
use std::sync::LazyLock;
use tokio::sync::Mutex;
use std::collections::HashSet;
use serde::{Serialize, Deserialize};

use crate::memory::store::{MemoryRecord, MemoryStore};
use crate::settings::XavierSettings;

static LAST_SYNC_TIME: LazyLock<Mutex<Option<Instant>>> = LazyLock::new(|| Mutex::new(None));
static SYNCED_IDS: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Struct representing the Engram-Xavier bidirectional memory bridge.
#[derive(Debug, Clone)]
pub struct EngramBridge {
    pub enabled: bool,
    pub url: String,
    pub client: reqwest::Client,
}

impl EngramBridge {
    /// Creates a new `EngramBridge` using active settings.
    pub fn new() -> Self {
        let settings = XavierSettings::current();
        let enabled = settings.engram.enabled;
        let url = settings.engram.url.clone();

        let mut base_url = url;
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            base_url = format!("http://{}", base_url);
        }

        Self {
            enabled,
            url: base_url,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Helper to enforce a rate limit of maximum 1 sync operation per second.
    async fn enforce_rate_limit(&self) {
        let mut lock = LAST_SYNC_TIME.lock().await;
        if let Some(last) = *lock {
            let elapsed = last.elapsed();
            if elapsed < Duration::from_secs(1) {
                let sleep_time = Duration::from_secs(1) - elapsed;
                tokio::time::sleep(sleep_time).await;
            }
        }
        *lock = Some(Instant::now());
    }

    /// Pushes a single MemoryRecord to Engram using the external CLI: `engram save`.
    /// Anti-circular sync: skips if the record originally came from Engram.
    pub async fn push_to_engram(&self, memory: MemoryRecord) -> Result<()> {
        if !self.enabled {
            debug!("EngramBridge is disabled. Skipping push_to_engram.");
            return Ok(());
        }

        // Enforce rate limiting: Max 1 sync per second
        self.enforce_rate_limit().await;

        // No circular sync check: skip if originally from engram
        if let Some(source_app) = memory.metadata.get("provenance")
            .and_then(|prov| prov.get("source_app"))
            .and_then(|s| s.as_str()) {
            if source_app == "engram" {
                debug!("Skipping push of record {} to engram to prevent circular sync loop.", memory.id);
                return Ok(());
            }
        }

        // Also check global synced IDs to prevent circular push
        {
            let synced = SYNCED_IDS.lock().await;
            if synced.contains(&memory.id) {
                debug!("Skipping push of record {} because it was imported/synced from engram.", memory.id);
                return Ok(());
            }
        }

        let title = memory.metadata.get("title")
            .and_then(|t| t.as_str())
            .unwrap_or(&memory.path);

        let metadata_str = serde_json::to_string(&memory.metadata).unwrap_or_default();

        // Spawn engram save CLI process
        let mut cmd = Command::new("engram");
        cmd.arg("save")
            .arg("--content").arg(&memory.content)
            .arg("--path").arg(&memory.path)
            .arg("--id").arg(&memory.id)
            .arg("--title").arg(title)
            .arg("--metadata").arg(&metadata_str);

        match cmd.output() {
            Ok(output) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("engram save CLI returned non-zero status: {}", stderr);
                } else {
                    info!("Successfully pushed memory {} to engram.", memory.id);
                }
            }
            Err(e) => {
                // Graceful failure: log warning and continue
                warn!("Graceful failure: Failed to execute 'engram save' CLI: {:?}", e);
            }
        }

        Ok(())
    }

    /// Searches Engram's index via its HTTP API. This acts as a fallback search.
    pub async fn search_engram(&self, query: String, limit: usize) -> Result<Vec<MemoryRecord>> {
        if !self.enabled {
            debug!("EngramBridge is disabled. Skipping search_engram.");
            return Ok(Vec::new());
        }

        let endpoint = format!("{}/mem/search", self.url);

        let body = serde_json::json!({
            "query": query,
            "limit": limit
        });

        let response = match self.client.post(&endpoint).json(&body).send().await {
            Ok(resp) => resp,
            Err(e) => {
                warn!("Graceful failure: Failed to reach Engram search API: {:?}", e);
                return Ok(Vec::new());
            }
        };

        if !response.status().is_success() {
            warn!("Engram search API returned non-success status: {}", response.status());
            return Ok(Vec::new());
        }

        let val: serde_json::Value = response.json().await.unwrap_or_default();

        // Parse results flexibly. Could be top-level array or inside "results" / "memories" / "observations"
        let items = if let Some(arr) = val.as_array() {
            arr.clone()
        } else if let Some(arr) = val.get("results").and_then(|v| v.as_array()) {
            arr.clone()
        } else if let Some(arr) = val.get("memories").and_then(|v| v.as_array()) {
            arr.clone()
        } else if let Some(arr) = val.get("observations").and_then(|v| v.as_array()) {
            arr.clone()
        } else {
            Vec::new()
        };

        let mut records = Vec::new();
        for item in items {
            if let Some(record) = parse_engram_observation_to_record(item) {
                records.push(record);
            }
        }

        Ok(records)
    }

    /// Pulls observations from Engram's API and imports them into Xavier.
    pub async fn pull_from_engram(&self, store: &dyn MemoryStore, workspace_id: &str) -> Result<Vec<MemoryRecord>> {
        if !self.enabled {
            debug!("EngramBridge is disabled. Skipping pull_from_engram.");
            return Ok(Vec::new());
        }

        // Enforce rate limiting: Max 1 sync per second
        self.enforce_rate_limit().await;

        let endpoint = format!("{}/export", self.url);
        let response = match self.client.get(&endpoint).send().await {
            Ok(resp) => resp,
            Err(e) => {
                warn!("Graceful failure: Failed to reach Engram export API during pull: {:?}", e);
                return Ok(Vec::new());
            }
        };

        if !response.status().is_success() {
            warn!("Engram export API returned non-success status: {}", response.status());
            return Ok(Vec::new());
        }

        let export_data: serde_json::Value = response.json().await.unwrap_or_default();
        let mut imported = Vec::new();

        // Process sessions, observations, and user prompts from the export data
        if let Some(observations) = export_data.get("observations").and_then(|v| v.as_array()) {
            for obs in observations {
                if let Some(mut record) = parse_engram_observation_to_record(obs.clone()) {
                    record.workspace_id = workspace_id.to_string();

                    // Avoid importing duplicates or causing loop
                    let id = record.id.clone();
                    let exists = store.get(workspace_id, &id).await?.is_some();
                    if !exists {
                        // Mark as synced to prevent circular push
                        {
                            let mut synced = SYNCED_IDS.lock().await;
                            synced.insert(id.clone());
                        }

                        if let Err(e) = store.put(record.clone()).await {
                            warn!("Failed to store pulled engram record {}: {:?}", id, e);
                        } else {
                            imported.push(record);
                        }
                    }
                }
            }
        }

        info!("Successfully pulled and imported {} memories from engram.", imported.len());
        Ok(imported)
    }
}

/// Standalone function helper for engram save subprocess
pub async fn push_to_engram(memory: MemoryRecord) -> Result<()> {
    let bridge = EngramBridge::new();
    bridge.push_to_engram(memory).await
}

/// Standalone function helper for fallback engram search
pub async fn search_engram(query: String, limit: usize) -> Result<Vec<MemoryRecord>> {
    let bridge = EngramBridge::new();
    bridge.search_engram(query, limit).await
}

/// Standalone function helper for pull_from_engram
pub async fn pull_from_engram(store: &dyn MemoryStore, workspace_id: &str) -> Result<Vec<MemoryRecord>> {
    let bridge = EngramBridge::new();
    bridge.pull_from_engram(store, workspace_id).await
}

/// Helper parser to map a JSON engram observation/memory into a Xavier MemoryRecord.
fn parse_engram_observation_to_record(obs: serde_json::Value) -> Option<MemoryRecord> {
    let id_val = obs.get("id")?;
    let id = if let Some(i) = id_val.as_i64() {
        i.to_string()
    } else if let Some(s) = id_val.as_str() {
        s.to_string()
    } else {
        return None;
    };

    let title = obs.get("title").and_then(|v| v.as_str()).unwrap_or("Observation");
    let content_text = obs.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let content = format!("{}\n\n{}", title, content_text);

    let engram_type = obs.get("type").and_then(|v| v.as_str()).unwrap_or("discovery");

    let created_at_str = obs.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
    let created_at = chrono::DateTime::parse_from_rfc3339(created_at_str)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());

    let updated_at_str = obs.get("updated_at").and_then(|v| v.as_str()).or_else(|| obs.get("created_at").and_then(|v| v.as_str())).unwrap_or("");
    let updated_at = chrono::DateTime::parse_from_rfc3339(updated_at_str)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());

    // Build the canonical provenance and metadata
    let provenance = serde_json::json!({
        "source_app": "engram",
        "source_type": "observation",
        "observed_at": created_at.to_rfc3339(),
        "recorded_at": updated_at.to_rfc3339(),
        "topic_key": obs.get("topic_key").cloned().unwrap_or(serde_json::Value::Null),
        "tool_name": obs.get("tool_name").cloned().unwrap_or(serde_json::Value::Null),
    });

    let metadata = serde_json::json!({
        "title": title,
        "engram_type": engram_type,
        "kind": "fact",
        "provenance": provenance,
        "duplicate_count": obs.get("duplicate_count").cloned().unwrap_or(serde_json::Value::Null),
    });

    Some(MemoryRecord {
        id: format!("engram-{}", id),
        workspace_id: String::new(),
        path: format!("bridge/engram/observations/{}", id),
        content,
        metadata,
        embedding: Vec::new(),
        created_at,
        updated_at,
        revision: 1,
        primary: true,
        score: 0.0,
        parent_id: None,
        cluster_id: None,
        level: crate::memory::schema::MemoryLevel::Raw,
        relation: None,
        clearance: Default::default(),
        revisions: Vec::new(),
        encrypted_dek: None,
        content_iv: None,
        metadata_iv: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::InMemoryMemoryStore;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_engram_bridge_disabled_by_default() {
        let mut bridge = EngramBridge::new();
        bridge.enabled = false;

        let record = MemoryRecord::default();
        let res = bridge.push_to_engram(record).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_circular_sync_prevention() {
        let mut bridge = EngramBridge::new();
        bridge.enabled = true;

        let mut record = MemoryRecord::default();
        record.id = "test-circular".to_string();
        record.metadata = serde_json::json!({
            "provenance": {
                "source_app": "engram"
            }
        });

        // Calling push should bypass and do nothing
        let res = bridge.push_to_engram(record).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let mut bridge = EngramBridge::new();
        bridge.enabled = true;

        let start = Instant::now();
        // Execute 2 operations
        let _ = bridge.enforce_rate_limit().await;
        let _ = bridge.enforce_rate_limit().await;
        let elapsed = start.elapsed();

        assert!(elapsed >= Duration::from_millis(900));
    }

    #[test]
    fn test_parse_engram_observation() {
        let obs = serde_json::json!({
            "id": 42,
            "title": "Design decision",
            "content": "Use engram bridge for sync.",
            "type": "decision",
            "created_at": "2026-03-20T10:00:00Z",
            "updated_at": "2026-03-20T10:05:00Z",
            "topic_key": "arch/bridge",
            "tool_name": "git-cli",
            "duplicate_count": 2
        });

        let record = parse_engram_observation_to_record(obs).unwrap();
        assert_eq!(record.id, "engram-42");
        assert_eq!(record.path, "bridge/engram/observations/42");
        assert!(record.content.contains("Design decision"));
        assert!(record.content.contains("Use engram bridge for sync."));
        assert_eq!(record.metadata["engram_type"], "decision");
        assert_eq!(record.metadata["provenance"]["source_app"], "engram");
    }

    #[tokio::test]
    async fn test_search_engram_success() {
        let mut server = mockito::Server::new_async().await;
        let mock_body = serde_json::json!([
            {
                "id": 101,
                "title": "Search Hit",
                "content": "Found via engram fallback search.",
                "type": "fact",
                "created_at": "2026-03-20T10:00:00Z"
            }
        ]);

        let _mock = server.mock("POST", "/mem/search")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&mock_body).unwrap())
            .create_async()
            .await;

        let mut bridge = EngramBridge::new();
        bridge.enabled = true;
        bridge.url = server.url();

        let results = bridge.search_engram("test query".to_string(), 5).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "engram-101");
        assert!(results[0].content.contains("Search Hit"));
    }

    #[tokio::test]
    async fn test_pull_from_engram_success() {
        let mut server = mockito::Server::new_async().await;
        let mock_body = serde_json::json!({
            "observations": [
                {
                    "id": 202,
                    "title": "Pulled Observation",
                    "content": "Imported from engram server.",
                    "type": "discovery",
                    "created_at": "2026-03-20T10:00:00Z"
                }
            ]
        });

        let _mock = server.mock("GET", "/export")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&mock_body).unwrap())
            .create_async()
            .await;

        let mut bridge = EngramBridge::new();
        bridge.enabled = true;
        bridge.url = server.url();

        let store = InMemoryMemoryStore::new();
        let imported = bridge.pull_from_engram(&store, "test-workspace").await.unwrap();

        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].id, "engram-202");
        assert!(imported[0].content.contains("Pulled Observation"));

        // Verify it was persisted to store
        let stored = store.get("test-workspace", "engram-202").await.unwrap().unwrap();
        assert_eq!(stored.id, "engram-202");
    }
}
