// SPDX-License-Identifier: MIT OR LICENSE-MESH
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

#[derive(Clone)]
pub struct SupabaseMemoryStore {
    client: Client,
    url: String,
    key: String,
}

impl SupabaseMemoryStore {
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

    pub async fn new(url: &str, key: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self {
            client,
            url: url.trim_end_matches('/').to_string(),
            key: key.to_string(),
        })
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
        self.postgrest_upsert("memory_records", &record).await
    }

    async fn get(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let key = stable_key("sqlite_mem", &[workspace_id, id_or_path]);

        // Try by ID
        let records: Vec<MemoryRecord> = self
            .postgrest_get("memory_records", &format!("id=eq.{}", key))
            .await?;
        if let Some(record) = records.into_iter().next() {
            return Ok(Some(record));
        }

        // Try by path
        let records: Vec<MemoryRecord> = self
            .postgrest_get(
                "memory_records",
                &format!("workspace_id=eq.{}&path=eq.{}", workspace_id, id_or_path),
            )
            .await?;
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
