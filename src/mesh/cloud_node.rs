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
}

impl CloudPeer {
    /// Create a new CloudPeer adapter using PgHeart settings.
    pub fn new(identity: Arc<NodeIdentity>) -> Result<Self> {
        let settings = crate::settings::XavierSettings::current();
        let url = settings.pgheart.url.clone().context("XAVIER_PGHEART_URL not set")?;
        let key = settings.pgheart.token.clone().context("XAVIER_PGHEART_TOKEN not set")?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Ok(Self {
            client,
            local_identity: identity,
            supabase_url: url,
            supabase_key: key,
        })
    }

    /// "Handshake" with a cloud peer: Verify they have a manifest in the cloud.
    pub async fn handshake(&self, peer_url: &str, _token: &str) -> Result<MeshHandshakeResponse> {
        // In cloud mode, the "peer_url" is used as the NodeID if it starts with xv1-
        let node_id = if peer_url.starts_with("xv1-") {
            peer_url.to_string()
        } else {
            anyhow::bail!("Invalid cloud peer NodeID: {}", peer_url);
        };

        let url = format!("{}/rest/v1/mesh_manifests?node_id=eq.{}", self.supabase_url, node_id);
        let resp = self.client.get(&url)
            .header("apikey", &self.supabase_key)
            .header("Authorization", format!("Bearer {}", self.supabase_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("Cloud handshake failed: {}", resp.status());
        }

        let manifests: Vec<serde_json::Value> = resp.json().await?;
        if manifests.is_empty() {
            anyhow::bail!("Peer {} has no manifest in cloud storage", node_id);
        }

        Ok(MeshHandshakeResponse {
            accepted: true,
            node_id: NodeId(node_id),
            public_key_hex: String::new(), // Cloud peers might not expose PK directly in manifest table
            reason: None,
        })
    }

    /// Fetch manifest from Supabase.
    pub async fn fetch_manifest(&self, peer: &PeerInfo, _token: &str) -> Result<MeshManifest> {
        let url = format!("{}/rest/v1/mesh_manifests?node_id=eq.{}", self.supabase_url, peer.node_id);
        let resp = self.client.get(&url)
            .header("apikey", &self.supabase_key)
            .header("Authorization", format!("Bearer {}", self.supabase_key))
            .send()
            .await?;

        let manifests: Vec<MeshManifest> = resp.json().await.context("Failed to parse cloud manifest")?;
        manifests.into_iter().next().context("Manifest not found for cloud peer")
    }

    /// Fetch chunks from Supabase `mesh_chunks` table.
    pub async fn fetch_chunks(
        &self,
        peer: &PeerInfo,
        _token: &str,
        hashes: &[String],
    ) -> Result<HashMap<String, Vec<u8>>> {
        let mut results = HashMap::new();

        // Fetch in batches or one by one for simplicity in Phase 1
        for hash in hashes {
            let url = format!("{}/rest/v1/mesh_chunks?node_id=eq.{}&hash=eq.{}",
                self.supabase_url, peer.node_id, hash);

            let resp = self.client.get(&url)
                .header("apikey", &self.supabase_key)
                .header("Authorization", format!("Bearer {}", self.supabase_key))
                .send()
                .await?;

            if resp.status().is_success() {
                let chunks: Vec<serde_json::Value> = resp.json().await?;
                if let Some(chunk) = chunks.into_iter().next() {
                    if let Some(data_b64) = chunk["data"].as_str() {
                        use base64::{Engine as _, engine::general_purpose};
                        let data = general_purpose::STANDARD.decode(data_b64)?;
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
                "node_id": self.local_identity.node_id.as_str(),
                "hash": hash,
                "data": general_purpose::STANDARD.encode(data),
                "created_at": chrono::Utc::now().to_rfc3339()
            });

            let url = format!("{}/rest/v1/mesh_chunks", self.supabase_url);
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
            }
        }

        Ok(pushed)
    }

    /// Publish the local manifest to cloud storage.
    pub async fn publish_manifest(&self, manifest: &MeshManifest) -> Result<()> {
        let url = format!("{}/rest/v1/mesh_manifests", self.supabase_url);
        let resp = self.client.post(&url)
            .header("apikey", &self.supabase_key)
            .header("Authorization", format!("Bearer {}", self.supabase_key))
            .header("Content-Type", "application/json")
            .header("Prefer", "resolution=merge-duplicates")
            .json(manifest)
            .send()
            .await?;

        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::CREATED {
            anyhow::bail!("Failed to publish manifest to cloud: {}", resp.status());
        }

        Ok(())
    }
}
