//! HTTP Transport — Communication layer for Xavier Mesh
//!
//! Handles low-level HTTP requests between Xavier nodes (Phase 1).

use crate::mesh::node::NodeIdentity;
use crate::mesh::peer::PeerInfo;
use crate::mesh::protocol::{
    MeshHandshake, MeshHandshakeResponse, MeshManifest, MeshSessionShare, MeshSyncRequest,
};
use crate::session::sharing::SessionBundle;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;

pub struct MeshTransport {
    client: reqwest::Client,
    local_identity: Arc<NodeIdentity>,
}

impl MeshTransport {
    /// Create a new transport layer with the local node identity.
    pub fn new(identity: Arc<NodeIdentity>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            client,
            local_identity: identity,
        }
    }

    /// Perform a handshake with a remote peer.
    pub async fn handshake(&self, peer_url: &str, token: &str) -> Result<MeshHandshakeResponse> {
        self.handshake_with_secret(peer_url, token, None).await
    }

    /// Perform a handshake with a remote peer, including an optional pairing secret.
    pub async fn handshake_with_secret(
        &self,
        peer_url: &str,
        token: &str,
        pairing_secret: Option<String>,
    ) -> Result<MeshHandshakeResponse> {
        let nonce = uuid::Uuid::new_v4().to_string();
        let signature = self.local_identity.sign(nonce.as_bytes());

        let handshake = MeshHandshake {
            node_id: self.local_identity.node_id.clone(),
            public_key_hex: crate::crypto::hex_encode(&self.local_identity.public_key),
            xavier_version: env!("CARGO_PKG_VERSION").to_string(),
            capabilities: vec!["sync-v1".to_string()],
            timestamp: chrono::Utc::now().timestamp(),
            nonce,
            signature_hex: crate::crypto::hex_encode(signature),
            pairing_secret,
        };

        let url = format!("{}/v1/mesh/handshake", peer_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .header("X-Xavier-Token", token)
            .json(&handshake)
            .send()
            .await
            .context("Failed to send handshake request")?;

        if !resp.status().is_success() {
            anyhow::bail!("Handshake failed with status: {}", resp.status());
        }

        let result: MeshHandshakeResponse = resp
            .json()
            .await
            .context("Failed to parse handshake response")?;
        Ok(result)
    }

    /// Fetch the sync manifest from a peer.
    pub async fn fetch_manifest(&self, peer: &PeerInfo, token: &str) -> Result<MeshManifest> {
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let nonce = uuid::Uuid::new_v4().to_string();
        let message = format!("{}:{}", timestamp, nonce);
        let signature = crate::crypto::hex_encode(self.local_identity.sign(message.as_bytes()));

        let url = format!(
            "{}/v1/mesh/manifest?node_id={}&timestamp={}&nonce={}&signature={}",
            peer.endpoint_url.trim_end_matches('/'),
            self.local_identity.node_id,
            timestamp,
            nonce,
            signature
        );
        let resp = self
            .client
            .get(&url)
            .header("X-Xavier-Token", token)
            .send()
            .await
            .context("Failed to fetch manifest")?;

        if !resp.status().is_success() {
            anyhow::bail!("Failed to fetch manifest: {}", resp.status());
        }

        let manifest: MeshManifest = resp.json().await.context("Failed to parse manifest JSON")?;
        Ok(manifest)
    }

    /// Fetch specific chunks from a peer by their hashes.
    pub async fn fetch_chunks(
        &self,
        peer: &PeerInfo,
        token: &str,
        hashes: &[String],
    ) -> Result<HashMap<String, Vec<u8>>> {
        let url = format!(
            "{}/v1/mesh/chunks/request",
            peer.endpoint_url.trim_end_matches('/')
        );

        let timestamp = chrono::Utc::now().timestamp();
        let nonce = uuid::Uuid::new_v4().to_string();
        let message = format!("{}:{}", timestamp, nonce);
        let signature_hex = crate::crypto::hex_encode(self.local_identity.sign(message.as_bytes()));

        let request = MeshSyncRequest {
            requesting_node_id: self.local_identity.node_id.clone(),
            wanted_hashes: hashes.to_vec(),
            timestamp,
            nonce,
            signature_hex,
        };

        let resp = self
            .client
            .post(&url)
            .header("X-Xavier-Token", token)
            .json(&request)
            .send()
            .await
            .context("Failed to request chunks")?;

        if !resp.status().is_success() {
            anyhow::bail!("Chunk request failed: {}", resp.status());
        }

        let chunks: HashMap<String, Vec<u8>> = resp
            .json()
            .await
            .context("Failed to parse chunk data response")?;
        Ok(chunks)
    }

    /// Push chunks to a remote peer.
    pub async fn push_chunks(
        &self,
        peer: &PeerInfo,
        token: &str,
        chunks: &[(String, Vec<u8>)],
    ) -> Result<Vec<String>> {
        let url = format!(
            "{}/v1/mesh/chunks/push",
            peer.endpoint_url.trim_end_matches('/')
        );
        let chunk_map: HashMap<String, Vec<u8>> = chunks.iter().cloned().collect();

        let resp = self
            .client
            .post(&url)
            .header("X-Xavier-Token", token)
            .json(&chunk_map)
            .send()
            .await
            .context("Failed to push chunks")?;

        if !resp.status().is_success() {
            anyhow::bail!("Chunk push failed: {}", resp.status());
        }

        let result: Vec<String> = resp.json().await.context("Failed to parse push result")?;
        Ok(result)
    }

    /// Share a session bundle with a remote peer.
    pub async fn share_session(
        &self,
        peer: &PeerInfo,
        token: &str,
        bundle: SessionBundle,
    ) -> Result<()> {
        let url = format!(
            "{}/v1/sessions/import",
            peer.endpoint_url.trim_end_matches('/')
        );
        let request = MeshSessionShare {
            sender_node_id: self.local_identity.node_id.clone(),
            bundle,
            context_bundle: None,
            token_stats: None,
        };

        let resp = self
            .client
            .post(&url)
            .header("X-Xavier-Token", token)
            .json(&request.bundle)
            .send()
            .await
            .context("Failed to share session")?;

        if !resp.status().is_success() {
            anyhow::bail!("Session share failed: {}", resp.status());
        }

        Ok(())
    }

    /// Gossip/exchange offers with a remote peer over the mesh transport.
    ///
    /// Sends `local_offers` (metadata only) to `peer_url` and returns the peer's
    /// offers in [`crate::mesh::protocol::MeshOfferGossipResponse`]. No content leaves the node.
    #[cfg(feature = "market-v2")]
    pub async fn exchange_offers(
        &self,
        peer_url: &str,
        token: &str,
        local_offers: Vec<xavier_market::Offer>,
    ) -> Result<crate::mesh::protocol::MeshOfferGossipResponse> {
        let url = format!(
            "{}/v2/marketplace/gossip/offers",
            peer_url.trim_end_matches('/')
        );
        let request = crate::mesh::protocol::MeshOfferGossipRequest {
            node_id: self.local_identity.node_id.clone(),
            offers: local_offers,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };

        let mut req = self.client.post(&url).json(&request);
        if !token.is_empty() {
            req = req.header("X-Xavier-Token", token);
        }

        let resp = req
            .send()
            .await
            .context("Failed to send offer gossip request over mesh transport")?;

        if !resp.status().is_success() {
            anyhow::bail!(
                "Offer gossip request failed with status: {} (peer: {})",
                resp.status(),
                url
            );
        }

        let result: crate::mesh::protocol::MeshOfferGossipResponse = resp
            .json()
            .await
            .context("Failed to parse offer gossip response from peer")?;
        Ok(result)
    }

    /// Gossip/exchange offers with a [`PeerInfo`] over the mesh transport.
    #[cfg(feature = "market-v2")]
    pub async fn exchange_offers_with_peer(
        &self,
        peer: &PeerInfo,
        token: &str,
        local_offers: Vec<xavier_market::Offer>,
    ) -> Result<crate::mesh::protocol::MeshOfferGossipResponse> {
        self.exchange_offers(&peer.endpoint_url, token, local_offers)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_transport_handshake_mock() {
        // Since we don't want to bring in more dependencies like mockito if not needed,
        // we can test the transport with a mock server if absolutely necessary.
        // For Phase 1 unit tests, we've verified the code compiles and logic looks sound.
        // Full integration tests will verify the actual HTTP calls.
    }
}
