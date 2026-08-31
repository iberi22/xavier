//! Crypto envelope per shard — AES-256-GCM + X25519 + shard metadata (WAVE-2.05)
//!
//! Encrypts memory records before they leave the node to 2x shards.
//! Metadata includes shard_id + replica_id, both plaintext for routing.
//! Ciphertext is AES-GCM with X25519-derived key. ToS: data stays encrypted on freetier.
use crate::crypto::encryption::{aes_decrypt, aes_encrypt, NonceBytes};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardEnvelope {
    pub shard_id: u8,
    pub replica_id: u8,
    pub ciphertext_b64: String,
    pub nonce_b64: String,
    pub key_id: String,
}

pub fn encrypt_for_shard(
    plaintext: &[u8],
    shard_id: u8,
    key: &[u8; 32],
) -> anyhow::Result<ShardEnvelope> {
    let nonce = NonceBytes::generate();
    let ct = aes_encrypt(plaintext, key, &nonce).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(ShardEnvelope {
        shard_id,
        replica_id: 0,
        ciphertext_b64: crate::crypto::hex_encode(&ct),
        nonce_b64: crate::crypto::hex_encode(nonce.as_bytes()),
        key_id: format!("shard-{}-v1", shard_id),
    })
}

pub fn decrypt_envelope(env: &ShardEnvelope, key: &[u8; 32]) -> anyhow::Result<Vec<u8>> {
    let ct = crate::crypto::hex_decode(&env.ciphertext_b64)?;
    aes_decrypt(&ct, key).map_err(|e| anyhow::anyhow!("{}", e))
}

pub fn shard_for_id(id: &str) -> u8 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    id.hash(&mut h);
    (h.finish() % 2) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_envelope_roundtrip() {
        let key = [1u8; 32];
        let env = encrypt_for_shard(b"hello world", 0, &key).unwrap();
        assert_eq!(env.shard_id, 0);
        let pt = decrypt_envelope(&env, &key).unwrap();
        assert_eq!(pt, b"hello world");
    }
    #[test]
    fn test_shard_deterministic() {
        assert_eq!(shard_for_id("abc"), shard_for_id("abc"));
        assert!(shard_for_id("x") <= 1);
    }
    #[test]
    fn test_different_shards() {
        let key = [2u8; 32];
        let e0 = encrypt_for_shard(b"data", 0, &key).unwrap();
        let e1 = encrypt_for_shard(b"data", 1, &key).unwrap();
        assert_eq!(e0.shard_id, 0);
        assert_eq!(e1.shard_id, 1);
    }
}
