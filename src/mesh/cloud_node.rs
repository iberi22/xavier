//! Cloud Peer Adapter — Supabase REST transport for Xavier Mesh
//!
//! Enables synchronization for nodes that are not directly reachable via P2P.
//! It uses Supabase REST API (via PgHeart) as a mailbox/relay.

use crate::mesh::node::{NodeId, NodeIdentity};
use crate::mesh::peer::PeerInfo;
use crate::mesh::protocol::{MeshHandshakeResponse, MeshManifest};
pub use crate::node_identity::challenge::{sign_sync_request, SignatureHeader};
use crate::session::sharing::SessionBundle;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;

type ChunkFetchResult = Result<Option<(String, Vec<u8>)>>;

fn compute_payload_hash<T: ?Sized + serde::Serialize>(payload: &T) -> String {
    let bytes = serde_json::to_vec(payload).unwrap_or_default();
    crate::crypto::hex_encode(Sha256::digest(&bytes))
}

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
        let url = settings
            .pgheart
            .url
            .clone()
            .context("PGHEART_URL (or XAVIER_PGHEART_URL) not set")?;
        let key = settings
            .pgheart
            .token
            .clone()
            .context("PGHEART_TOKEN (or XAVIER_PGHEART_TOKEN) not set")?;

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

        let url = format!(
            "{}/rest/v1/mesh_manifests?node_id=eq.{}",
            self.supabase_url, node_id
        );
        let resp = self
            .client
            .get(&url)
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
        let url = format!(
            "{}/rest/v1/mesh_manifests?node_id=eq.{}",
            self.supabase_url, peer.node_id
        );
        let resp = self
            .client
            .get(&url)
            .header("apikey", &self.supabase_key)
            .header("Authorization", format!("Bearer {}", self.supabase_key))
            .send()
            .await?;

        let manifests: Vec<MeshManifest> = resp
            .json()
            .await
            .context("Failed to parse cloud manifest")?;
        manifests
            .into_iter()
            .next()
            .context("Manifest not found for cloud peer")
    }

    /// Fetch chunks from Supabase `mesh_chunks` table concurrently.
    pub async fn fetch_chunks(
        &self,
        peer: &PeerInfo,
        _token: &str,
        hashes: &[String],
    ) -> Result<HashMap<String, Vec<u8>>> {
        let mut set: tokio::task::JoinSet<ChunkFetchResult> = tokio::task::JoinSet::new();

        for hash in hashes {
            let client = self.client.clone();
            let supabase_url = self.supabase_url.clone();
            let supabase_key = self.supabase_key.clone();
            let node_id = peer.node_id.clone();
            let hash = hash.clone();

            set.spawn(async move {
                let url = format!(
                    "{}/rest/v1/mesh_chunks?node_id=eq.{}&hash=eq.{}",
                    supabase_url, node_id, hash
                );

                let resp = client
                    .get(&url)
                    .header("apikey", &supabase_key)
                    .header("Authorization", format!("Bearer {}", supabase_key))
                    .send()
                    .await?;

                if resp.status().is_success() {
                    let chunks: Vec<serde_json::Value> = resp.json().await?;
                    if let Some(chunk) = chunks.into_iter().next() {
                        if let Some(data_b64) = chunk["data"].as_str() {
                            let data = crate::crypto::base64_decode(data_b64)
                                .context("Failed to decode base64 chunk data")?;
                            return Ok(Some((hash, data)));
                        }
                    }
                }
                Ok(None)
            });
        }

        let mut results = HashMap::new();
        while let Some(res) = set.join_next().await {
            match res {
                Ok(Ok(Some((hash, data)))) => {
                    results.insert(hash, data);
                }
                Ok(Ok(None)) => {}
                Ok(Err(err)) => return Err(err),
                Err(err) => return Err(err.into()),
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

        for (hash, data) in chunks {
            let payload = json!({
                "node_id": self.local_identity.node_id.as_str(),
                "hash": hash,
                "data": crate::crypto::base64_encode(data),
                "created_at": chrono::Utc::now().to_rfc3339()
            });

            let payload_hash = compute_payload_hash(&payload);
            let sig_header = sign_sync_request(&self.local_identity, &payload_hash);

            let url = format!("{}/rest/v1/mesh_chunks", self.supabase_url);
            let mut req = self
                .client
                .post(&url)
                .header("apikey", &self.supabase_key)
                .header("Authorization", format!("Bearer {}", self.supabase_key))
                .header("Content-Type", "application/json")
                .header("Prefer", "resolution=merge-duplicates");

            for (header_name, header_val) in sig_header.to_headers() {
                req = req.header(header_name, header_val);
            }

            let resp = req.json(&payload).send().await?;

            if resp.status().is_success() || resp.status() == reqwest::StatusCode::CREATED {
                pushed.push(hash.clone());
            }
        }

        Ok(pushed)
    }

    /// Publish the local manifest to cloud storage.
    pub async fn publish_manifest(&self, manifest: &MeshManifest) -> Result<()> {
        let payload_hash = compute_payload_hash(manifest);
        let sig_header = sign_sync_request(&self.local_identity, &payload_hash);

        let url = format!("{}/rest/v1/mesh_manifests", self.supabase_url);
        let mut req = self
            .client
            .post(&url)
            .header("apikey", &self.supabase_key)
            .header("Authorization", format!("Bearer {}", self.supabase_key))
            .header("Content-Type", "application/json")
            .header("Prefer", "resolution=merge-duplicates");

        for (header_name, header_val) in sig_header.to_headers() {
            req = req.header(header_name, header_val);
        }

        let resp = req.json(manifest).send().await?;

        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::CREATED {
            anyhow::bail!("Failed to publish manifest to cloud: {}", resp.status());
        }

        Ok(())
    }

    /// Share a session bundle with a cloud peer.
    pub async fn share_session(
        &self,
        peer: &PeerInfo,
        _token: &str,
        bundle: SessionBundle,
    ) -> Result<()> {
        let payload = json!({
            "node_id": peer.node_id.as_str(),
            "sender_node_id": self.local_identity.node_id.as_str(),
            "bundle": bundle,
            "created_at": chrono::Utc::now().to_rfc3339()
        });

        let payload_hash = compute_payload_hash(&payload);
        let sig_header = sign_sync_request(&self.local_identity, &payload_hash);

        let url = format!("{}/rest/v1/mesh_sessions", self.supabase_url);
        let mut req = self
            .client
            .post(&url)
            .header("apikey", &self.supabase_key)
            .header("Authorization", format!("Bearer {}", self.supabase_key))
            .header("Content-Type", "application/json")
            .header("Prefer", "resolution=merge-duplicates");

        for (header_name, header_val) in sig_header.to_headers() {
            req = req.header(header_name, header_val);
        }

        let resp = req.json(&payload).send().await?;

        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::CREATED {
            anyhow::bail!("Failed to share session to cloud: {}", resp.status());
        }

        Ok(())
    }
}

/// Cloudflare adapters R2/KV/D1 shards (WAVE-2.03) — OpenAI-compat proxy via Workers
pub fn shard_for_cloud(id: &str) -> u8 {
    crate::crypto::envelope::shard_for_id(id)
}
pub struct CloudflareAdapters {
    pub r2_shard_0: String,
    pub r2_shard_1: String,
    pub kv_namespace: String,
    pub d1_db: String,
}
impl CloudflareAdapters {
    pub fn new() -> Self {
        Self {
            r2_shard_0: "r2-shard-0".into(),
            r2_shard_1: "r2-shard-1".into(),
            kv_namespace: "kv-xavier".into(),
            d1_db: "d1-xavier".into(),
        }
    }
    pub fn shard_bucket(&self, shard: u8) -> &str {
        if shard == 0 {
            &self.r2_shard_0
        } else {
            &self.r2_shard_1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_identity::challenge::verify_sync_request;

    #[test]
    fn test_compute_payload_hash_and_sign() {
        let identity = Arc::new(NodeIdentity::generate());
        let payload = json!({
            "test_key": "test_value",
            "number": 42
        });

        let hash = compute_payload_hash(&payload);
        assert!(!hash.is_empty());

        let sig_header = sign_sync_request(&identity, &hash);
        assert_eq!(sig_header.node_id, identity.node_id.as_str());

        let verified = verify_sync_request(&sig_header, &hash, None).unwrap();
        assert_eq!(verified, identity.node_id);

        let headers = sig_header.to_headers();
        let map: HashMap<&str, String> = headers.into_iter().collect();
        assert_eq!(map.get("X-Node-Id").unwrap(), identity.node_id.as_str());
        assert_eq!(
            map.get("X-Node-Public-Key").unwrap(),
            &crate::crypto::hex_encode(&identity.public_key)
        );
        assert!(map.contains_key("X-Node-Signature"));
        assert!(map.contains_key("X-Timestamp"));
        assert!(map.contains_key("X-Nonce"));
    }
}

// ---------------------------------------------------------------------------
// Zero-Handoff Context Capsule Storage (Cloudflare R2)
// ---------------------------------------------------------------------------

/// Magic prefix for encrypted Zero-Handoff context capsules: `ZHCP` (Zero-Handoff Capsule)
pub const CAPSULE_MAGIC: &[u8; 4] = b"ZHCP";
pub const CAPSULE_VERSION: u8 = 1;
pub const CAPSULE_SCHEME: &str = "swal-capsule://";

/// Metadata envelope stored in the capsule container
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapsuleEnvelope {
    pub version: u8,
    pub capsule_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub original_size: usize,
    pub compressed_size: usize,
    pub nonce_hex: String,
    pub salt_hex: String,
    pub ciphertext_hex: String,
    pub content_hash: String,
}

/// Compress capsule data using gzip.
pub fn compress_capsule(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(data)
        .context("Failed to gzip capsule data")?;
    encoder
        .finish()
        .context("Failed to finish gzip compression")
}

/// Decompress capsule data using gzip.
pub fn decompress_capsule(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = flate2::read::GzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .context("Failed to decompress gzip capsule data")?;
    Ok(decompressed)
}

/// Derive a 256-bit encryption key from secret/token and salt.
fn derive_capsule_key(secret: &str, salt: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"swal-zero-handoff-capsule-v1:");
    hasher.update(salt);
    hasher.update(secret.as_bytes());
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

/// Encrypt and compress raw capsule data.
/// Returns the serialized JSON envelope ready to be stored as `.json.gz`.
pub fn seal_capsule(capsule_id: &str, data: &[u8], secret: &str) -> Result<Vec<u8>> {
    let compressed = compress_capsule(data)?;

    let mut salt = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut salt);

    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);

    let key = derive_capsule_key(secret, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("AES-GCM key init error: {e}"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    let aad = format!("capsule:{capsule_id}");
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: &compressed,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|e| anyhow::anyhow!("AES-GCM encryption error: {e}"))?;

    let content_hash = crate::crypto::hex_encode(Sha256::digest(data));

    let envelope = CapsuleEnvelope {
        version: CAPSULE_VERSION,
        capsule_id: capsule_id.to_string(),
        created_at: chrono::Utc::now(),
        original_size: data.len(),
        compressed_size: compressed.len(),
        nonce_hex: crate::crypto::hex_encode(nonce_bytes),
        salt_hex: crate::crypto::hex_encode(salt),
        ciphertext_hex: crate::crypto::hex_encode(ciphertext),
        content_hash,
    };

    let envelope_json =
        serde_json::to_vec(&envelope).context("Failed to serialize capsule envelope")?;
    // Final storage format is .json.gz
    compress_capsule(&envelope_json)
}

/// Decompress, authenticate, and decrypt a capsule package.
pub fn open_capsule(capsule_id: &str, sealed_gz: &[u8], secret: &str) -> Result<Vec<u8>> {
    let envelope_json =
        decompress_capsule(sealed_gz).context("Failed to decompress capsule container")?;
    let envelope: CapsuleEnvelope =
        serde_json::from_slice(&envelope_json).context("Invalid capsule envelope JSON")?;

    if envelope.capsule_id != capsule_id {
        anyhow::bail!(
            "Capsule ID mismatch: expected {}, got {}",
            capsule_id,
            envelope.capsule_id
        );
    }

    let salt = crate::crypto::hex_decode(&envelope.salt_hex).context("Invalid salt hex")?;
    let nonce_bytes =
        crate::crypto::hex_decode(&envelope.nonce_hex).context("Invalid nonce hex")?;
    let ciphertext =
        crate::crypto::hex_decode(&envelope.ciphertext_hex).context("Invalid ciphertext hex")?;

    if nonce_bytes.len() != 12 {
        anyhow::bail!("Invalid nonce length in capsule envelope");
    }

    let key = derive_capsule_key(secret, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("AES-GCM key init error: {e}"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    let aad = format!("capsule:{capsule_id}");
    let compressed = cipher
        .decrypt(
            nonce,
            Payload {
                msg: &ciphertext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| {
            anyhow::anyhow!("Capsule decryption failed: incorrect secret or tampered data")
        })?;

    let plaintext =
        decompress_capsule(&compressed).context("Failed to decompress decrypted payload")?;

    let actual_hash = crate::crypto::hex_encode(Sha256::digest(&plaintext));
    if actual_hash != envelope.content_hash {
        anyhow::bail!("Capsule integrity check failed: content hash mismatch");
    }

    Ok(plaintext)
}

/// Resolve the active tenant identifier from environment or settings.
pub fn resolve_capsule_tenant() -> String {
    std::env::var("XAVIER_TENANT")
        .or_else(|_| std::env::var("PGHEART_TENANT"))
        .or_else(|_| {
            let settings = crate::settings::XavierSettings::current();
            settings
                .pgheart
                .instance_id
                .filter(|s| !s.is_empty())
                .ok_or(std::env::VarError::NotPresent)
        })
        .unwrap_or_else(|_| "default".to_string())
}

/// Resolve the capsule encryption secret.
pub fn resolve_capsule_secret() -> String {
    std::env::var("XAVIER_CAPSULE_SECRET")
        .or_else(|_| std::env::var("XAVIER_CLOUD_BACKUP_PASSPHRASE"))
        .or_else(|_| std::env::var("PGHEART_TOKEN"))
        .or_else(|_| std::env::var("XAVIER_PGHEART_TOKEN"))
        .or_else(|_| {
            let settings = crate::settings::XavierSettings::current();
            settings
                .pgheart
                .token
                .filter(|s| !s.is_empty())
                .ok_or(std::env::VarError::NotPresent)
        })
        .unwrap_or_else(|_| "xavier-zero-handoff-default-secret".to_string())
}

/// Resolve the cloud base URL for capsules.
pub fn resolve_capsule_base_url() -> Result<String> {
    let settings = crate::settings::XavierSettings::current();
    std::env::var("XAVIER_R2_URL")
        .or_else(|_| std::env::var("PGHEART_URL"))
        .or_else(|_| std::env::var("XAVIER_PGHEART_URL"))
        .or_else(|_| {
            settings
                .pgheart
                .url
                .filter(|s| !s.is_empty())
                .ok_or(std::env::VarError::NotPresent)
        })
        .context("Neither XAVIER_R2_URL nor PGHEART_URL/XAVIER_PGHEART_URL is configured")
}

/// Resolve optional bearer token for cloud storage access.
pub fn resolve_capsule_auth_token() -> Option<String> {
    std::env::var("XAVIER_R2_TOKEN")
        .or_else(|_| std::env::var("PGHEART_TOKEN"))
        .or_else(|_| std::env::var("XAVIER_PGHEART_TOKEN"))
        .or_else(|_| {
            let settings = crate::settings::XavierSettings::current();
            settings
                .pgheart
                .token
                .filter(|s| !s.is_empty())
                .ok_or(std::env::VarError::NotPresent)
        })
        .ok()
}

/// Format the R2 path for a context capsule:
/// `t/<tenant>/capsules/<capsule_id>.json.gz`
pub fn capsule_r2_path(tenant: &str, capsule_id: &str) -> String {
    format!("t/{}/capsules/{}.json.gz", tenant, capsule_id)
}

/// Upload an encrypted, compressed Zero-Handoff context capsule to Cloudflare R2.
/// Returns the reference token: `swal-capsule://<capsule_id>`.
pub async fn upload_capsule(capsule_id: &str, data: &[u8]) -> Result<String> {
    let tenant = resolve_capsule_tenant();
    let secret = resolve_capsule_secret();
    let base_url = resolve_capsule_base_url()?;
    let auth_token = resolve_capsule_auth_token();

    let sealed_gz = seal_capsule(capsule_id, data, &secret)?;
    let relative_path = capsule_r2_path(&tenant, capsule_id);
    let target_url = format!("{}/{}", base_url.trim_end_matches('/'), relative_path);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut request = client
        .put(&target_url)
        .header("Content-Type", "application/gzip")
        .header("X-Capsule-ID", capsule_id)
        .header("X-Tenant-ID", &tenant);

    if let Some(token) = auth_token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }

    let resp = request
        .body(sealed_gz)
        .send()
        .await
        .with_context(|| format!("Failed to send PUT request for capsule {capsule_id}"))?;

    if !resp.status().is_success() && resp.status() != reqwest::StatusCode::CREATED {
        let status = resp.status();
        let err_text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Capsule upload failed (status {status}): {err_text}");
    }

    Ok(format!("{}{}", CAPSULE_SCHEME, capsule_id))
}

/// Download, authenticate, and decrypt a Zero-Handoff context capsule from Cloudflare R2.
pub async fn download_capsule(capsule_id: &str) -> Result<Vec<u8>> {
    // Strip swal-capsule:// scheme if provided
    let clean_id = capsule_id
        .strip_prefix(CAPSULE_SCHEME)
        .unwrap_or(capsule_id);
    let tenant = resolve_capsule_tenant();
    let secret = resolve_capsule_secret();
    let base_url = resolve_capsule_base_url()?;
    let auth_token = resolve_capsule_auth_token();

    let relative_path = capsule_r2_path(&tenant, clean_id);
    let target_url = format!("{}/{}", base_url.trim_end_matches('/'), relative_path);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut request = client.get(&target_url);
    if let Some(token) = auth_token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }

    let resp = request
        .send()
        .await
        .with_context(|| format!("Failed to send GET request for capsule {clean_id}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let err_text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Capsule download failed (status {status}): {err_text}");
    }

    let sealed_gz = resp
        .bytes()
        .await
        .context("Failed to read capsule response bytes")?;

    open_capsule(clean_id, &sealed_gz, &secret)
}

#[cfg(test)]
mod capsule_tests {
    use super::*;

    #[test]
    fn test_capsule_compression_roundtrip() {
        let sample = b"Zero-Handoff continuation context test data payload with high compression ratio repeated text text text text";
        let compressed = compress_capsule(sample).expect("compression");
        assert!(!compressed.is_empty());
        let decompressed = decompress_capsule(&compressed).expect("decompression");
        assert_eq!(decompressed, sample);
    }

    #[test]
    fn test_capsule_seal_and_open_roundtrip() {
        let capsule_id = "test-capsule-uuid-12345";
        let secret = "super-secure-capsule-key";
        let data = json!({
            "task_id": "task-99",
            "messages": [
                {"role": "system", "content": "You are Xavier Zero-Handoff agent."},
                {"role": "user", "content": "Context saturated. Migrate state."}
            ],
            "working_memory": {"token_count": 285000}
        });
        let raw_bytes = serde_json::to_vec(&data).unwrap();

        let sealed_gz = seal_capsule(capsule_id, &raw_bytes, secret).expect("seal");
        assert!(!sealed_gz.is_empty());

        let opened = open_capsule(capsule_id, &sealed_gz, secret).expect("open");
        assert_eq!(opened, raw_bytes);
    }

    #[test]
    fn test_capsule_wrong_secret_fails() {
        let capsule_id = "test-capsule-uuid-wrong-secret";
        let raw_bytes = b"Secret agent context data";
        let sealed_gz = seal_capsule(capsule_id, raw_bytes, "secret-one").unwrap();

        let err = open_capsule(capsule_id, &sealed_gz, "secret-two");
        assert!(err.is_err());
        assert!(err
            .unwrap_err()
            .to_string()
            .contains("Capsule decryption failed"));
    }

    #[test]
    fn test_capsule_tampered_data_fails() {
        let capsule_id = "test-capsule-uuid-tamper";
        let raw_bytes = b"Tamper test data";
        let mut sealed_gz = seal_capsule(capsule_id, raw_bytes, "secret").unwrap();

        // Corrupt byte in the gzip container
        let len = sealed_gz.len();
        sealed_gz[len - 2] ^= 0xFF;

        let err = open_capsule(capsule_id, &sealed_gz, "secret");
        assert!(err.is_err());
    }

    #[test]
    fn test_capsule_r2_path_formatting() {
        let path = capsule_r2_path("acme-corp", "cap-001");
        assert_eq!(path, "t/acme-corp/capsules/cap-001.json.gz");
    }

    #[tokio::test]
    async fn test_upload_and_download_capsule_mock() {
        let mut server = mockito::Server::new_async().await;
        let tenant = "test-tenant";
        let capsule_id = "cap-xyz-987";
        let secret = "mock-secret";
        let payload = b"Mocked zero handoff agent context data";

        let path = format!("/t/{}/capsules/{}.json.gz", tenant, capsule_id);

        let put_mock = server
            .mock("PUT", path.as_str())
            .match_header("X-Capsule-ID", capsule_id)
            .match_header("X-Tenant-ID", tenant)
            .with_status(200)
            .create_async()
            .await;

        let sealed = seal_capsule(capsule_id, payload, secret).unwrap();

        let get_mock = server
            .mock("GET", path.as_str())
            .with_status(200)
            .with_body(sealed)
            .create_async()
            .await;

        std::env::set_var("XAVIER_R2_URL", server.url());
        std::env::set_var("XAVIER_TENANT", tenant);
        std::env::set_var("XAVIER_CAPSULE_SECRET", secret);

        let ref_token = upload_capsule(capsule_id, payload).await.expect("upload");
        assert_eq!(ref_token, format!("swal-capsule://{}", capsule_id));

        let downloaded = download_capsule(&ref_token).await.expect("download");
        assert_eq!(downloaded, payload);

        put_mock.assert_async().await;
        get_mock.assert_async().await;

        std::env::remove_var("XAVIER_R2_URL");
        std::env::remove_var("XAVIER_TENANT");
        std::env::remove_var("XAVIER_CAPSULE_SECRET");
    }
}
