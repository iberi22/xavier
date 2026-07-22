//! SWAL Mesh Node Authentication
//!
//! Provides strict authentication mechanism for P2P Mesh nodes using SWAL tokens.
//! Authenticated connections are validated against Ed25519 signatures, expiration times,
//! and valid token payload structure. Unauthenticated or forged attempts are rejected/dropped.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use crate::mesh::node::NodeIdentity;

/// A SWAL Token wrapper used for node authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwalToken {
    /// Base64 encoded outer token representation.
    pub token: String,
}

/// The inner structure of a SWAL Token after base64 and JSON decoding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwalTokenContainer {
    /// Stringified JSON payload containing token claims.
    pub payload: String,
    /// Hex-encoded signature of the payload by the node's private key.
    pub signature: String,
}

/// The inner payload claims for node authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwalTokenPayload {
    /// Node ID of the issuing/authenticated node.
    pub node_id: String,
    /// Hex-encoded Ed25519 public key of the node.
    pub public_key_hex: String,
    /// Expiration timestamp of the token (Unix timestamp).
    pub expires_at: u64,
    /// Optional workspace ID limits.
    pub workspace_id: Option<String>,
}

impl SwalToken {
    /// Create a new SWAL token by signing a payload with the node identity.
    pub fn create(identity: &NodeIdentity, expires_at: u64, workspace_id: Option<String>) -> anyhow::Result<Self> {
        let payload = SwalTokenPayload {
            node_id: identity.node_id.as_str().to_string(),
            public_key_hex: crate::crypto::hex_encode(&identity.public_key),
            expires_at,
            workspace_id,
        };

        let payload_str = serde_json::to_string(&payload)?;
        let signature_bytes = identity.sign(payload_str.as_bytes());
        let signature_hex = crate::crypto::hex_encode(signature_bytes);

        let container = SwalTokenContainer {
            payload: payload_str,
            signature: signature_hex,
        };

        let container_bytes = serde_json::to_vec(&container)?;
        let base64_token = crate::crypto::base64_encode(container_bytes);

        Ok(Self { token: base64_token })
    }

    /// Verifies and authenticates the SWAL token.
    /// Returns the authenticated Node ID and public key if successful, or an error.
    pub fn verify(&self) -> anyhow::Result<SwalTokenPayload> {
        let decoded_bytes = crate::crypto::base64_decode(&self.token)
            .ok_or_else(|| anyhow::anyhow!("Invalid base64 token format"))?;

        let container: SwalTokenContainer = serde_json::from_slice(&decoded_bytes)
            .map_err(|e| anyhow::anyhow!("Invalid token container JSON: {}", e))?;

        let signature_bytes = crate::crypto::hex_decode(&container.signature)
            .map_err(|e| anyhow::anyhow!("Invalid signature hex format: {}", e))?;

        let inner_payload: SwalTokenPayload = serde_json::from_str(&container.payload)
            .map_err(|e| anyhow::anyhow!("Invalid inner token payload JSON: {}", e))?;

        let public_key_bytes = crate::crypto::hex_decode(&inner_payload.public_key_hex)
            .map_err(|e| anyhow::anyhow!("Invalid public key hex format: {}", e))?;

        // 1. Verify cryptographic signature
        if !NodeIdentity::verify(&public_key_bytes, container.payload.as_bytes(), &signature_bytes) {
            anyhow::bail!("Invalid token signature (forgery detected)");
        }

        // 2. Check expiration time
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs();

        if inner_payload.expires_at < now {
            anyhow::bail!("Token has expired");
        }

        Ok(inner_payload)
    }
}

use std::sync::{RwLock, OnceLock};
use std::collections::{HashMap, HashSet};
use std::time::{Instant, Duration};

/// Key representation for an Access Control List permission entry.
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct AclKey {
    pub node_id: String,
    pub action: String,
    pub resource: String,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    allowed: bool,
    expires_at: Instant,
}

/// Mesh Access Control List Manager with TTL Cache support.
pub struct MeshAuthAcl {
    granted: RwLock<HashSet<AclKey>>,
    cache: RwLock<HashMap<AclKey, CacheEntry>>,
    ttl: Duration,
}

impl MeshAuthAcl {
    /// Creates a new MeshAuthAcl instance with the specified TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            granted: RwLock::new(HashSet::new()),
            cache: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// Checks if a node is allowed to perform a given action on a resource.
    /// Consults the TTL cache first, falling back to the persistent store if expired or missing.
    pub fn check_permission(&self, node_id: &str, action: &str, resource: &str) -> bool {
        let key = AclKey {
            node_id: node_id.to_string(),
            action: action.to_string(),
            resource: resource.to_string(),
        };

        // Check cache
        {
            if let Ok(cache_read) = self.cache.read() {
                if let Some(entry) = cache_read.get(&key) {
                    if Instant::now() < entry.expires_at {
                        return entry.allowed;
                    }
                }
            }
        }

        // Cache miss or expired: evaluate underlying permissions
        let allowed = {
            if let Ok(granted_read) = self.granted.read() {
                granted_read.contains(&key)
            } else {
                false
            }
        };

        // Populate cache
        if let Ok(mut cache_write) = self.cache.write() {
            cache_write.insert(
                key,
                CacheEntry {
                    allowed,
                    expires_at: Instant::now() + self.ttl,
                },
            );
        }

        allowed
    }

    /// Grants permission for a node to perform a given action on a resource.
    pub fn grant_permission(&self, node_id: &str, action: &str, resource: &str) {
        let key = AclKey {
            node_id: node_id.to_string(),
            action: action.to_string(),
            resource: resource.to_string(),
        };

        if let Ok(mut granted_write) = self.granted.write() {
            granted_write.insert(key.clone());
        }

        // Keep cache consistent
        if let Ok(mut cache_write) = self.cache.write() {
            cache_write.insert(
                key,
                CacheEntry {
                    allowed: true,
                    expires_at: Instant::now() + self.ttl,
                },
            );
        }
    }

    /// Revokes permission for a node to perform a given action on a resource.
    pub fn revoke_permission(&self, node_id: &str, action: &str, resource: &str) {
        let key = AclKey {
            node_id: node_id.to_string(),
            action: action.to_string(),
            resource: resource.to_string(),
        };

        if let Ok(mut granted_write) = self.granted.write() {
            granted_write.remove(&key);
        }

        // Keep cache consistent
        if let Ok(mut cache_write) = self.cache.write() {
            cache_write.insert(
                key,
                CacheEntry {
                    allowed: false,
                    expires_at: Instant::now() + self.ttl,
                },
            );
        }
    }
}

static GLOBAL_ACL: OnceLock<MeshAuthAcl> = OnceLock::new();

/// Returns the global thread-safe MeshAuthAcl instance.
pub fn get_global_acl() -> &'static MeshAuthAcl {
    GLOBAL_ACL.get_or_init(|| MeshAuthAcl::new(Duration::from_secs(5)))
}

/// Standalone function helper to check permission.
pub fn check_permission(node_id: &str, action: &str, resource: &str) -> bool {
    get_global_acl().check_permission(node_id, action, resource)
}

/// Standalone function helper to grant permission.
pub fn grant_permission(node_id: &str, action: &str, resource: &str) {
    get_global_acl().grant_permission(node_id, action, resource)
}

/// Standalone function helper to revoke permission.
pub fn revoke_permission(node_id: &str, action: &str, resource: &str) {
    get_global_acl().revoke_permission(node_id, action, resource)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::node::NodeIdentity;

    #[test]
    fn test_valid_swal_token_authentication() {
        let identity = NodeIdentity::generate();
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expires_at = now + 3600; // 1 hour in the future

        let token = SwalToken::create(&identity, expires_at, Some("test-ws".to_string()))
            .expect("Should generate valid token");

        let payload = token.verify().expect("Should authenticate successfully");
        assert_eq!(payload.node_id, identity.node_id.as_str());
        assert_eq!(payload.workspace_id, Some("test-ws".to_string()));
    }

    #[test]
    fn test_invalid_swal_token_base64_rejection() {
        let token = SwalToken {
            token: "not-a-valid-base64-string!!!".to_string(),
        };

        let result = token.verify();
        assert!(result.is_err(), "Invalid base64 token must be rejected");
        assert!(result.unwrap_err().to_string().contains("base64"));
    }

    #[test]
    fn test_expired_swal_token_rejection() {
        let identity = NodeIdentity::generate();
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expires_at = now - 60; // 1 minute in the past

        let token = SwalToken::create(&identity, expires_at, None)
            .expect("Should generate token");

        let result = token.verify();
        assert!(result.is_err(), "Expired token must be rejected");
        assert!(result.unwrap_err().to_string().contains("expired"));
    }

    #[test]
    fn test_forged_signature_swal_token_rejection() {
        let identity = NodeIdentity::generate();
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expires_at = now + 3600;

        let mut token = SwalToken::create(&identity, expires_at, None)
            .expect("Should generate token");

        // Manually decode, alter the signature or payload, and re-encode to forge it
        let decoded_bytes = crate::crypto::base64_decode(&token.token).unwrap();
        let mut container: SwalTokenContainer = serde_json::from_slice(&decoded_bytes).unwrap();

        // Tamper with the signature hex
        container.signature = "deadbeef".repeat(8);

        let container_bytes = serde_json::to_vec(&container).unwrap();
        token.token = crate::crypto::base64_encode(container_bytes);

        let result = token.verify();
        assert!(result.is_err(), "Forged token must be strictly rejected");
        assert!(result.unwrap_err().to_string().contains("forgery"));
    }

    #[test]
    fn acl_default_deny() {
        let acl = MeshAuthAcl::new(Duration::from_secs(1));
        assert!(!acl.check_permission("node_1", "read", "memory_chunk"));
        assert!(!acl.check_permission("node_2", "write", "mesh_channel"));
    }

    #[test]
    fn acl_grant_revoke_cycle() {
        let acl = MeshAuthAcl::new(Duration::from_secs(1));
        let node = "node_abc";
        let action = "sync";
        let resource = "vector_store";

        // Default deny
        assert!(!acl.check_permission(node, action, resource));

        // Grant
        acl.grant_permission(node, action, resource);
        assert!(acl.check_permission(node, action, resource));

        // Revoke
        acl.revoke_permission(node, action, resource);
        assert!(!acl.check_permission(node, action, resource));
    }

    #[test]
    fn acl_cache_expires() {
        let acl = MeshAuthAcl::new(Duration::from_millis(100));
        let node = "node_xyz";
        let action = "read";
        let resource = "secure_channel";

        let key = AclKey {
            node_id: node.to_string(),
            action: action.to_string(),
            resource: resource.to_string(),
        };

        // Grant
        acl.grant_permission(node, action, resource);
        assert!(acl.check_permission(node, action, resource));

        // Manually remove from `granted` to simulate out-of-band backend update
        {
            let mut granted_write = acl.granted.write().unwrap();
            granted_write.remove(&key);
        }

        // Should still return true (cached)
        assert!(acl.check_permission(node, action, resource), "Should return true from cache before TTL expiration");

        // Wait for TTL to expire
        std::thread::sleep(Duration::from_millis(150));

        // Should now return false
        assert!(!acl.check_permission(node, action, resource), "Should return false after cache TTL expires");
    }
}
