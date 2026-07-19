use std::collections::HashMap;
use tempfile::tempdir;
use xavier::mesh::{PeerInfo, PeerRegistry, NodeId};

// Import our handlers from the binary crate if possible, or define mock tests.
// Since CLI handlers are in the main binary target of `xavier`, let's check if they can be verified directly.
// Let's test the PeerRegistry serialization/deserialization with our extended fields!

#[test]
fn test_peer_registry_with_workspace_federation_fields() {
    let dir = tempdir().unwrap();
    let storage_path = dir.path().join("peers_federation.json");

    let mut registry = PeerRegistry::load_from(storage_path.clone()).unwrap();

    let node_id = NodeId("xv1-federated-peer".to_string());
    let mut peer = PeerInfo {
        node_id: node_id.clone(),
        alias: Some("Federated Peer".to_string()),
        endpoint_url: "http://localhost:8006".to_string(),
        public_key_hex: "aabbccdd".to_string(),
        added_at: 1000,
        last_seen_at: None,
        sync_enabled: true,
        is_cloud: false,
        iroh_addr: None,
        shared_workspace_ids: Vec::new(),
        shared_workspace_tokens: HashMap::new(),
    };

    peer.shared_workspace_ids.push("workspace-alpha".to_string());
    peer.shared_workspace_tokens.insert("workspace-alpha".to_string(), "mock-token-xyz".to_string());

    registry.add_peer(peer).unwrap();
    assert_eq!(registry.list_peers().len(), 1);

    // Reload registry from file
    let reloaded = PeerRegistry::load_from(storage_path).unwrap();
    let loaded_peer = reloaded.get_peer(&node_id).unwrap();

    assert_eq!(loaded_peer.shared_workspace_ids.len(), 1);
    assert_eq!(loaded_peer.shared_workspace_ids[0], "workspace-alpha");
    assert_eq!(loaded_peer.shared_workspace_tokens.get("workspace-alpha").unwrap(), "mock-token-xyz");
}

#[tokio::test]
async fn test_workspace_sharing_token_roundtrip() {
    let identity = xavier::mesh::node::NodeIdentity::generate();
    let payload_data = serde_json::json!({
        "node_id": "xv1-localnode",
        "endpoint": "http://localhost:8006",
        "public_key_hex": xavier::crypto::hex_encode(&identity.public_key),
        "workspace_id": "default-workspace",
        "expires_at": chrono::Utc::now().timestamp() + 3600,
    });

    let payload_str = serde_json::to_string(&payload_data).unwrap();
    let signature = identity.sign(payload_str.as_bytes());

    let token_data = serde_json::json!({
        "payload": payload_str,
        "signature": xavier::crypto::hex_encode(&signature),
    });

    let token_json = serde_json::to_string(&token_data).unwrap();
    let token = xavier::crypto::base64_encode(token_json);

    let decoded_bytes = xavier::crypto::base64_decode(&token).unwrap();
    let decoded_token_data: serde_json::Value = serde_json::from_slice(&decoded_bytes).unwrap();
    let decoded_payload_str = decoded_token_data["payload"].as_str().unwrap();
    let decoded_payload: serde_json::Value = serde_json::from_str(decoded_payload_str).unwrap();

    assert_eq!(decoded_payload["workspace_id"], "default-workspace");
    assert_eq!(decoded_payload["node_id"], "xv1-localnode");
}

#[tokio::test]
async fn test_data_consent_token_revocation() {
    // Set a temporary config directory to avoid overwriting real user files
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("XAVIER_CONFIG_DIR", dir.path().to_str().unwrap());

    // 1. Initial active consents should be empty
    let initial_active = xavier::mesh::DataConsentManager::list_active_consents().unwrap();
    assert!(initial_active.is_empty());

    // 2. Register an active consent
    let token_id = "test-token-123".to_string();
    let consent = xavier::mesh::ActiveConsent {
        token_id: token_id.clone(),
        workspace_id: "workspace-alpha".to_string(),
        expires_at: chrono::Utc::now().timestamp() as u64 + 3600, // active
        token: "mock-base64-token-string".to_string(),
    };

    xavier::mesh::DataConsentManager::register_active_consent(consent).unwrap();

    // Check that it's listed as active
    let active_list = xavier::mesh::DataConsentManager::list_active_consents().unwrap();
    assert_eq!(active_list.len(), 1);
    assert_eq!(active_list[0].token_id, token_id);
    assert_eq!(active_list[0].workspace_id, "workspace-alpha");

    // Check that it's NOT revoked
    let is_rev = xavier::mesh::DataConsentManager::is_token_revoked(&token_id).unwrap();
    assert!(!is_rev);

    // 3. Revoke the consent
    xavier::mesh::DataConsentManager::revoke_consent(&token_id).unwrap();

    // Check that it's now revoked
    let is_rev_after = xavier::mesh::DataConsentManager::is_token_revoked(&token_id).unwrap();
    assert!(is_rev_after);

    // Check that it's no longer listed as active
    let active_list_after = xavier::mesh::DataConsentManager::list_active_consents().unwrap();
    assert!(active_list_after.is_empty());

    // 4. Test fallback token ID extraction and revocation in the same sequence
    let payload_str = "{\"workspace_id\":\"test-ws\",\"node_id\":\"node1\"}";
    let fallback_token_id = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(payload_str.as_bytes());
        format!("hash-{}", &xavier::crypto::hex_encode(&hasher.finalize())[..16])
    };

    // Assert fallback initially not revoked
    assert!(!xavier::mesh::DataConsentManager::is_token_revoked(&fallback_token_id).unwrap());

    // Revoke the fallback token_id
    xavier::mesh::DataConsentManager::revoke_consent(&fallback_token_id).unwrap();

    // Assert fallback now revoked
    assert!(xavier::mesh::DataConsentManager::is_token_revoked(&fallback_token_id).unwrap());
}
