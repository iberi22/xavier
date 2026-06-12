//! Cloud Peer Adapter — Supabase REST transport for Xavier Mesh
//!
//! Enables synchronization for nodes that are not directly reachable via P2P.
//! It uses Supabase REST API (via PgHeart) as a mailbox/relay.

use crate::mesh::node::{NodeId, NodeIdentity};
use crate::mesh::peer::PeerInfo;
use crate::mesh::protocol::{MeshHandshakeResponse, MeshManifest};
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

pub struct CloudPeer {
    client: reqwest::Client,
    local_identity: Arc<NodeIdentity>,
    supabase_url: String,
    supabase_key: String,
    namespace: String,
}

impl CloudPeer {
    /// Create a new CloudPeer adapter using PgHeart settings.
    pub fn new(identity: Arc<NodeIdentity>) -> Result<Self> {
        let settings = crate::settings::XavierSettings::current();
        let url = settings.pgheart.url.clone().context("XAVIER_PGHEART_URL not set")?;
        let key = settings.pgheart.token.clone().context("XAVIER_PGHEART_TOKEN not set")?;
        let namespace = settings.pgheart.instance_id.clone().unwrap_or_else(|| "default".to_string());

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Ok(Self {
            client,
            local_identity: identity,
            supabase_url: url,
            supabase_key: key,
            namespace,
        })
    }

    /// "Handshake" with a cloud peer: Verify they have a manifest in the cloud.
    pub async fn handshake(&self, peer_url: &str, _token: &str) -> Result<MeshHandshakeResponse> {
        let node_id = if peer_url.starts_with("xv1-") {
            peer_url.to_string()
        } else {
            anyhow::bail!("Invalid cloud peer NodeID: {}", peer_url);
        };

        // For handshake, we just return ok if we can connect to supabase.
        // Or we could check if there are any chunks for this namespace.
        let url = format!("{}/rest/v1/encrypted_sync_chunks?namespace=eq.{}&limit=1", self.supabase_url, self.namespace);
        let resp = self.client.get(&url)
            .header("apikey", &self.supabase_key)
            .header("Authorization", format!("Bearer {}", self.supabase_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("Cloud handshake failed: {}", resp.status());
        }

        Ok(MeshHandshakeResponse {
            accepted: true,
            node_id: NodeId(node_id),
            public_key_hex: String::new(),
            reason: None,
        })
    }

    /// Fetch manifest from Supabase. We synthesize one from available chunks.
    pub async fn fetch_manifest(&self, peer: &PeerInfo, _token: &str) -> Result<MeshManifest> {
        let url = format!("{}/rest/v1/encrypted_sync_chunks?namespace=eq.{}&select=chunk_hash,created_at", self.supabase_url, self.namespace);
        let resp = self.client.get(&url)
            .header("apikey", &self.supabase_key)
            .header("Authorization", format!("Bearer {}", self.supabase_key))
            .send()
            .await?;

        let rows: Vec<serde_json::Value> = resp.json().await.context("Failed to parse cloud chunks for manifest")?;
        
        let mut chunks = Vec::new();
        for row in rows {
            if let (Some(hash), Some(created_at)) = (row["chunk_hash"].as_str(), row["created_at"].as_str()) {
                let ts = chrono::DateTime::parse_from_rfc3339(created_at).unwrap_or_default().timestamp();
                chunks.push(crate::mesh::protocol::ChunkRef {
                    hash: hash.to_string(),
                    document_count: 0, // Not strictly known without payload
                    created_at: ts,
                });
            }
        }

        Ok(MeshManifest {
            node_id: peer.node_id.clone(),
            chunks,
            generated_at: chrono::Utc::now().timestamp(),
        })
    }

    /// Fetch chunks from Supabase `encrypted_sync_chunks` table.
    pub async fn fetch_chunks(
        &self,
        _peer: &PeerInfo,
        _token: &str,
        hashes: &[String],
    ) -> Result<HashMap<String, Vec<u8>>> {
        let mut results = HashMap::new();

        for hash in hashes {
            let url = format!("{}/rest/v1/encrypted_sync_chunks?namespace=eq.{}&chunk_hash=eq.{}",
                self.supabase_url, self.namespace, hash);

            let resp = self.client.get(&url)
                .header("apikey", &self.supabase_key)
                .header("Authorization", format!("Bearer {}", self.supabase_key))
                .send()
                .await?;

            if resp.status().is_success() {
                let chunks: Vec<serde_json::Value> = resp.json().await?;
                if let Some(chunk) = chunks.into_iter().next() {
                    if let Some(payload_str) = chunk["payload"].as_str() {
                        use base64::{Engine as _, engine::general_purpose};
                        let data = general_purpose::STANDARD.decode(payload_str)?;
                        results.insert(hash.clone(), data);
                    }
                }
            }
        }

        Ok(results)
    }

    /// Push chunks to Supabase.
    pub async fn push_chunks(
        &self,
        _peer: &PeerInfo,
        _token: &str,
        chunks: &[(String, Vec<u8>)],
    ) -> Result<Vec<String>> {
        let mut pushed = Vec::new();
        use base64::{Engine as _, engine::general_purpose};

        for (hash, data) in chunks {
            let payload = json!({
                "namespace": self.namespace,
                "chunk_hash": hash,
                "payload": general_purpose::STANDARD.encode(data),
                "created_at": chrono::Utc::now().to_rfc3339()
            });

            let url = format!("{}/rest/v1/encrypted_sync_chunks", self.supabase_url);
            let resp = self.client.post(&url)
                .header("apikey", &self.supabase_key)
                .header("Authorization", format!("Bearer {}", self.supabase_key))
                .header("Content-Type", "application/json")
                .header("Prefer", "resolution=merge-duplicates")
                .json(&payload)
                .send()
                .await?;

            if resp.status().is_success() || resp.status() == reqwest::StatusCode::CREATED {
                pushed.push(hash.clone());
            } else {
                let status = resp.status();
                let err_text = resp.text().await.unwrap_or_default();
                eprintln!("Failed to push chunk to supabase: status={}, error={}", status, err_text);
            }
        }

        Ok(pushed)
    }

    /// Publish the local manifest to cloud storage. (No-op since we build it from chunks dynamically)
    pub async fn publish_manifest(&self, _manifest: &MeshManifest) -> Result<()> {
        // With our simplified schema, we don't store the manifest separately.
        // It's derived dynamically in `fetch_manifest` from the chunks table.
        Ok(())
    }
}
