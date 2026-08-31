//! Supabase backend for Xavier memory store (REST API).

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use serde_json::json;
use std::any::Any;

use crate::checkpoint::Checkpoint;
use crate::domain::memory::belief::BeliefEdge;
use crate::memory::schema::MemoryQueryFilters;
use crate::memory::store::{
    filter_records, stable_key, DurableWorkspaceState, MemoryBackend, MemoryRecord, MemoryStore,
    SessionTokenRecord,
};
use crate::settings::XavierSettings;

pub fn shard_for_id(id: &str) -> u8 {
    // Consistent hash %2 for 2x sharding (ToS compliant: 2 active projects global)
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    id.hash(&mut h);
    (h.finish() % 2) as u8
}

pub fn shard_for_record_id(id: &str) -> u8 {
    shard_for_id(id)
}

#[derive(Clone)]
pub struct SupabaseMemoryStore {
    client: Client,
    url: String,
    key: String,
    /// Optional second Supabase project for 2x sharding (XAVIER_SUPABASE_URL_2)
    url_2: Option<String>,
    key_2: Option<String>,
    /// Secondary client if 2x configured
    client_2: Option<Client>,
}

impl SupabaseMemoryStore {
    /// From env.
    pub async fn from_env() -> Result<Self> {
        let settings = XavierSettings::current();
        let url = std::env::var("XAVIER_SUPABASE_URL")
            .ok()
            .or_else(|| settings.memory.supabase_url.clone())
            .context("XAVIER_SUPABASE_URL or settings.memory.supabase_url not set")?;
        let key = std::env::var("XAVIER_SUPABASE_KEY")
            .ok()
            .or_else(|| settings.memory.supabase_key.clone())
            .context("XAVIER_SUPABASE_KEY or settings.memory.supabase_key not set")?;

        Self::new(&url, &key).await
    }

    /// New.
    pub async fn new(url: &str, key: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        // Optional second project (2x) — ToS: max 2 active projects global per user
        let url_2 = std::env::var("XAVIER_SUPABASE_URL_2").ok();
        let key_2 = std::env::var("XAVIER_SUPABASE_KEY_2").ok();
        let client_2 = if url_2.is_some() && key_2.is_some() {
            Some(
                Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()?,
            )
        } else {
            None
        };

        Ok(Self {
            client,
            url: url.trim_end_matches('/').to_string(),
            key: key.to_string(),
            url_2: url_2.map(|u| u.trim_end_matches('/').to_string()),
            key_2,
            client_2,
        })
    }

    pub fn shard_for(&self, id: &str) -> u8 {
        shard_for_id(id)
    }

    pub fn is_sharded(&self) -> bool {
        self.client_2.is_some() && self.url_2.is_some()
    }

    fn client_for_shard(&self, shard: u8) -> (&Client, &str, &str) {
        if shard == 1 && self.client_2.is_some() && self.url_2.is_some() && self.key_2.is_some() {
            (
                self.client_2.as_ref().unwrap(),
                self.url_2.as_ref().unwrap(),
                self.key_2.as_ref().unwrap(),
            )
        } else {
            (&self.client, &self.url, &self.key)
        }
    }

    async fn postgrest_get<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
        query: &str,
    ) -> Result<Vec<T>> {
        let url = format!("{}/rest/v1/{}?{}", self.url, table, query);
        let resp = self
            .client
            .get(&url)
            .header("apikey", &self.key)
            .header("Authorization", format!("Bearer {}", self.key))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("Supabase GET failed: {}", resp.status());
        }

        Ok(resp.json().await?)
    }

    /// Health check.
    pub async fn health_check(&self) -> Result<()> {
        let url = format!("{}/rest/v1/", self.url);
        let resp = self
            .client
            .get(&url)
            .header("apikey", &self.key)
            .header("Authorization", format!("Bearer {}", self.key))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("Supabase health check failed: {}", resp.status());
        }

        Ok(())
    }

    async fn postgrest_upsert<T: serde::Serialize>(&self, table: &str, payload: &T) -> Result<()> {
        let url = format!("{}/rest/v1/{}", self.url, table);
        let resp = self
            .client
            .post(&url)
            .header("apikey", &self.key)
            .header("Authorization", format!("Bearer {}", self.key))
            .header("Content-Type", "application/json")
            .header("Prefer", "resolution=merge-duplicates")
            .json(payload)
            .send()
            .await?;

        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::CREATED {
            anyhow::bail!("Supabase UPSERT failed: {}", resp.status());
        }

        Ok(())
    }

    async fn postgrest_delete(&self, table: &str, query: &str) -> Result<()> {
        let url = format!("{}/rest/v1/{}?{}", self.url, table, query);
        let resp = self
            .client
            .delete(&url)
            .header("apikey", &self.key)
            .header("Authorization", format!("Bearer {}", self.key))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("Supabase DELETE failed: {}", resp.status());
        }

        Ok(())
    }
}

#[async_trait]
impl MemoryStore for SupabaseMemoryStore {
    fn backend(&self) -> MemoryBackend {
        MemoryBackend::Supabase
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn health(&self) -> Result<String> {
        // Just try to fetch an empty query to check connectivity
        let _: Vec<serde_json::Value> = self
            .postgrest_get("memory_records", "limit=1")
            .await
            .unwrap_or_default();
        Ok(format!("supabase connected to {}", self.url))
    }

    async fn put(&self, record: MemoryRecord) -> Result<()> {
        // Sharded write: hash(id)%2 → shard 0/1 (ToS 2 active max)
        let shard = shard_for_id(&record.id);
        let mut payload = serde_json::to_value(&record)?;
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("shard_id".into(), serde_json::json!(shard));
            if self.is_sharded() {
                obj.insert(
                    "project_id".into(),
                    serde_json::json!(format!("shard-{}", shard)),
                );
            }
        }
        let (client, url, key) = self.client_for_shard(shard);
        let upsert_url = format!("{}/rest/v1/memory_records", url);
        let resp = client
            .post(&upsert_url)
            .header("apikey", key)
            .header("Authorization", format!("Bearer {}", key))
            .header("Content-Type", "application/json")
            .header("Prefer", "resolution=merge-duplicates")
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::CREATED {
            anyhow::bail!("Supabase shard {} UPSERT failed: {}", shard, resp.status());
        }
        Ok(())
    }

    async fn get(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let key = stable_key("sqlite_mem", &[workspace_id, id_or_path]);
        let shard = shard_for_id(&key);

        // Try primary shard first, then fallback if sharded
        for try_shard in if self.is_sharded() {
            vec![shard, 1 - shard]
        } else {
            vec![shard]
        } {
            let (client, url, key_val) = self.client_for_shard(try_shard);
            let fetch = |q: &str| {
                let url = format!("{}/rest/v1/memory_records?{}", url, q);
                let client = client.clone();
                let key = key_val.to_string();
                async move {
                    let resp = client
                        .get(&url)
                        .header("apikey", &key)
                        .header("Authorization", format!("Bearer {}", key))
                        .send()
                        .await?;
                    if !resp.status().is_success() {
                        anyhow::bail!("Supabase shard {} GET failed: {}", try_shard, resp.status());
                    }
                    Ok::<Vec<MemoryRecord>, anyhow::Error>(resp.json().await?)
                }
            };
            // Try by ID
            if let Ok(records) = fetch(&format!("id=eq.{}", key)).await {
                if let Some(record) = records.into_iter().next() {
                    return Ok(Some(record));
                }
            }
            // Try by path
            if let Ok(records) = fetch(&format!(
                "workspace_id=eq.{}&path=eq.{}",
                workspace_id, id_or_path
            ))
            .await
            {
                if let Some(r) = records.into_iter().next() {
                    return Ok(Some(r));
                }
            }
            if !self.is_sharded() {
                break;
            }
        }
        // Fallback to original unified get for backward compat
        let records: Vec<MemoryRecord> = self
            .postgrest_get("memory_records", &format!("id=eq.{}", key))
            .await
            .unwrap_or_default();
        if let Some(record) = records.into_iter().next() {
            return Ok(Some(record));
        }
        let records: Vec<MemoryRecord> = self
            .postgrest_get(
                "memory_records",
                &format!("workspace_id=eq.{}&path=eq.{}", workspace_id, id_or_path),
            )
            .await
            .unwrap_or_default();
        Ok(records.into_iter().next())
    }

    async fn update(&self, record: MemoryRecord) -> Result<()> {
        self.put(record).await
    }

    async fn delete(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let existing = self.get(workspace_id, id_or_path).await?;
        if let Some(ref record) = existing {
            self.postgrest_delete("memory_records", &format!("id=eq.{}", record.id))
                .await?;
            self.postgrest_delete(
                "memory_records",
                &format!(
                    "workspace_id=eq.{}&parent_id=eq.{}",
                    workspace_id, record.id
                ),
            )
            .await?;
        }
        Ok(existing)
    }

    async fn list(&self, workspace_id: &str) -> Result<Vec<MemoryRecord>> {
        self.postgrest_get(
            "memory_records",
            &format!("workspace_id=eq.{}", workspace_id),
        )
        .await
    }

    async fn list_workspaces(&self) -> Result<Vec<String>> {
        #[derive(serde::Deserialize)]
        struct WsRow {
            workspace_id: String,
        }
        let rows: Vec<WsRow> = self
            .postgrest_get("memory_records", "select=workspace_id&order=workspace_id")
            .await?;
        let mut seen = std::collections::HashSet::new();
        let mut ids = Vec::new();
        for row in rows {
            if seen.insert(row.workspace_id.clone()) {
                ids.push(row.workspace_id);
            }
        }
        Ok(ids)
    }

    async fn search(
        &self,
        workspace_id: &str,
        query: &str,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryRecord>> {
        let records = self.list(workspace_id).await?;
        filter_records(records, workspace_id, query, filters)
    }

    async fn load_workspace_state(&self, workspace_id: &str) -> Result<DurableWorkspaceState> {
        let memories = self.list(workspace_id).await?;

        let belief_key = stable_key("belief_row", &[workspace_id]);
        let beliefs_rows: Vec<serde_json::Value> = self
            .postgrest_get("belief_states", &format!("id=eq.{}", belief_key))
            .await?;
        let beliefs = beliefs_rows
            .into_iter()
            .next()
            .and_then(|r| r.get("beliefs").cloned())
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        let now = Utc::now().to_rfc3339();
        let session_tokens = self
            .postgrest_get(
                "session_tokens",
                &format!("workspace_id=eq.{}&expires_at=gt.{}", workspace_id, now),
            )
            .await?;

        let checkpoints = self
            .postgrest_get(
                "checkpoint_records",
                &format!("workspace_id=eq.{}", workspace_id),
            )
            .await?;

        Ok(DurableWorkspaceState {
            memories,
            beliefs,
            session_tokens,
            checkpoints,
            entity_graph_snapshot: None,
        })
    }

    async fn save_beliefs(&self, workspace_id: &str, beliefs: Vec<BeliefEdge>) -> Result<()> {
        let belief_key = stable_key("belief_row", &[workspace_id]);
        let payload = json!({
            "id": belief_key,
            "workspace_id": workspace_id,
            "beliefs": beliefs,
            "updated_at": Utc::now().to_rfc3339()
        });
        self.postgrest_upsert("belief_states", &payload).await
    }

    async fn save_session_token(
        &self,
        workspace_id: &str,
        token: SessionTokenRecord,
    ) -> Result<()> {
        let token_key = stable_key("session_token_row", &[workspace_id, &token.token]);
        let payload = json!({
            "id": token_key,
            "workspace_id": workspace_id,
            "token": token.token,
            "created_at": token.created_at.to_rfc3339(),
            "expires_at": token.expires_at.to_rfc3339()
        });
        self.postgrest_upsert("session_tokens", &payload).await
    }

    async fn is_session_token_valid(&self, workspace_id: &str, token: &str) -> Result<bool> {
        let token_key = stable_key("session_token_row", &[workspace_id, token]);
        let now = Utc::now().to_rfc3339();
        let rows: Vec<serde_json::Value> = self
            .postgrest_get(
                "session_tokens",
                &format!("id=eq.{}&expires_at=gt.{}", token_key, now),
            )
            .await?;
        Ok(!rows.is_empty())
    }

    async fn save_checkpoint(&self, workspace_id: &str, checkpoint: Checkpoint) -> Result<()> {
        let checkpoint_key = stable_key(
            "checkpoint_row",
            &[workspace_id, &checkpoint.task_id, &checkpoint.name],
        );
        let payload = json!({
            "id": checkpoint_key,
            "workspace_id": workspace_id,
            "task_id": checkpoint.task_id,
            "name": checkpoint.name,
            "data": checkpoint.data
        });
        self.postgrest_upsert("checkpoint_records", &payload).await
    }

    async fn load_checkpoint(
        &self,
        workspace_id: &str,
        task_id: &str,
        name: &str,
    ) -> Result<Option<Checkpoint>> {
        let checkpoints: Vec<Checkpoint> = self
            .postgrest_get(
                "checkpoint_records",
                &format!(
                    "workspace_id=eq.{}&task_id=eq.{}&name=eq.{}",
                    workspace_id, task_id, name
                ),
            )
            .await?;
        Ok(checkpoints.into_iter().next())
    }

    async fn list_checkpoints(&self, workspace_id: &str, task_id: &str) -> Result<Vec<Checkpoint>> {
        self.postgrest_get(
            "checkpoint_records",
            &format!("workspace_id=eq.{}&task_id=eq.{}", workspace_id, task_id),
        )
        .await
    }

    async fn delete_checkpoint(&self, workspace_id: &str, task_id: &str, name: &str) -> Result<()> {
        self.postgrest_delete(
            "checkpoint_records",
            &format!(
                "workspace_id=eq.{}&task_id=eq.{}&name=eq.{}",
                workspace_id, task_id, name
            ),
        )
        .await
    }
}

#[cfg(test)]
mod shard_tests {
    use super::*;
    #[test]
    fn test_shard_for_id_deterministic() {
        let a = shard_for_id("test-id-1");
        let b = shard_for_id("test-id-1");
        assert_eq!(a, b);
        assert!(a == 0 || a == 1);
    }
    #[test]
    fn test_shard_distribution() {
        let mut counts = [0, 0];
        for i in 0..100 {
            let s = shard_for_id(&format!("id-{}", i));
            counts[s as usize] += 1;
        }
        assert!(
            counts[0] > 20 && counts[1] > 20,
            "should distribute roughly even"
        );
    }
}
