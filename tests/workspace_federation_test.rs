use std::collections::HashMap;
use tempfile::tempdir;
use xavier::mesh::{NodeId, PeerInfo, PeerRegistry};

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
        capabilities: Vec::new(),
    };

    peer.shared_workspace_ids
        .push("workspace-alpha".to_string());
    peer.shared_workspace_tokens
        .insert("workspace-alpha".to_string(), "mock-token-xyz".to_string());

    registry.add_peer(peer).unwrap();
    assert_eq!(registry.list_peers().len(), 1);

    // Reload registry from file
    let reloaded = PeerRegistry::load_from(storage_path).unwrap();
    let loaded_peer = reloaded.get_peer(&node_id).unwrap();

    assert_eq!(loaded_peer.shared_workspace_ids.len(), 1);
    assert_eq!(loaded_peer.shared_workspace_ids[0], "workspace-alpha");
    assert_eq!(
        loaded_peer
            .shared_workspace_tokens
            .get("workspace-alpha")
            .unwrap(),
        "mock-token-xyz"
    );
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

#[test]
fn test_federated_search_request_serde() {
    let json_str = r#"
    {
        "local_dbs": ["local1", "local2"],
        "peer_nodes": ["node_a", "node_b"],
        "propagate_to_mesh": true,
        "max_hops": 3
    }
    "#;

    let req: xavier::memory::schema::FederatedSearchRequest =
        serde_json::from_str(json_str).unwrap();
    assert_eq!(
        req.local_dbs,
        vec!["local1".to_string(), "local2".to_string()]
    );
    assert_eq!(
        req.peer_nodes,
        vec!["node_a".to_string(), "node_b".to_string()]
    );
    assert!(req.propagate_to_mesh);
    assert_eq!(req.max_hops, 3);

    // Test default serialization/deserialization with missing fields
    let json_empty = "{}";
    let req_empty: xavier::memory::schema::FederatedSearchRequest =
        serde_json::from_str(json_empty).unwrap();
    assert!(req_empty.local_dbs.is_empty());
    assert!(req_empty.peer_nodes.is_empty());
    assert!(!req_empty.propagate_to_mesh);
    assert_eq!(req_empty.max_hops, 1);
}
#[tokio::test]

async fn test_workspace_sharing_with_namespace_acl_filtering() {
    // 1. Prepare three memory records in a test workspace
    let workspace_id = "test-ws-federated".to_string();

    let rec_public = xavier::memory::store::MemoryRecord {
        id: "rec-1".to_string(),
        workspace_id: workspace_id.clone(),
        path: "docs/publico/readme.md".to_string(),
        content: "Public info".to_string(),
        ..Default::default()
    };

    let rec_private = xavier::memory::store::MemoryRecord {
        id: "rec-2".to_string(),
        workspace_id: workspace_id.clone(),
        path: "docs/privado/secret.md".to_string(),
        content: "Secret info".to_string(),
        ..Default::default()
    };

    let rec_other = xavier::memory::store::MemoryRecord {
        id: "rec-3".to_string(),
        workspace_id: workspace_id.clone(),
        path: "other/publico/intro.md".to_string(),
        content: "Other public info".to_string(),
        ..Default::default()
    };

    let memories = [rec_public, rec_private, rec_other];

    // 2. Generate a node identity and create a shared token with namespaces: ["docs/publico"]
    let identity = xavier::mesh::node::NodeIdentity::generate();
    let payload_data = serde_json::json!({
        "node_id": "xv1-localnode",
        "endpoint": "http://localhost:8006",
        "public_key_hex": xavier::crypto::hex_encode(&identity.public_key),
        "workspace_id": workspace_id,
        "namespaces": vec!["docs/publico".to_string()],
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

    // 3. Emulate query_workspace_handler's token decoding and validation
    let decoded_bytes = xavier::crypto::base64_decode(&token).unwrap();
    let decoded_token_data: serde_json::Value = serde_json::from_slice(&decoded_bytes).unwrap();
    let decoded_payload_str = decoded_token_data["payload"].as_str().unwrap();
    let decoded_signature_hex = decoded_token_data["signature"].as_str().unwrap();
    let decoded_signature_bytes = xavier::crypto::hex_decode(decoded_signature_hex).unwrap();

    let inner_payload: serde_json::Value = serde_json::from_str(decoded_payload_str).unwrap();
    let public_key_hex = inner_payload["public_key_hex"].as_str().unwrap();
    let public_key_bytes = xavier::crypto::hex_decode(public_key_hex).unwrap();

    // Verify signature
    assert!(xavier::mesh::node::NodeIdentity::verify(
        &public_key_bytes,
        decoded_payload_str.as_bytes(),
        &decoded_signature_bytes
    ));

    // Extract allowed namespaces and workspace ID
    let token_workspace_id = inner_payload["workspace_id"].as_str().unwrap();
    let allowed_namespaces: Option<Vec<String>> = inner_payload
        .get("namespaces")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    assert_eq!(token_workspace_id, "test-ws-federated");
    assert_eq!(allowed_namespaces.as_ref().unwrap().len(), 1);
    assert_eq!(allowed_namespaces.as_ref().unwrap()[0], "docs/publico");

    // 4. Perform the segment-wise filtering logic
    let filtered_memories: Vec<&xavier::memory::store::MemoryRecord> =
        if let Some(ref namespaces) = allowed_namespaces {
            memories
                .iter()
                .filter(|r| {
                    namespaces.iter().any(|pattern| {
                        let record_clean = r.path.trim_end_matches('/');
                        let pattern_clean = pattern.trim_end_matches('/');
                        if record_clean == pattern_clean {
                            true
                        } else {
                            let prefix = format!("{}/", pattern_clean);
                            record_clean.starts_with(&prefix)
                        }
                    })
                })
                .collect()
        } else {
            memories.iter().collect()
        };

    // 5. Assert that only the records matching the allowed namespace are returned
    assert_eq!(filtered_memories.len(), 1);
    assert_eq!(filtered_memories[0].path, "docs/publico/readme.md");
    assert_eq!(filtered_memories[0].content, "Public info");
}

#[test]
fn test_namespace_acl_entry_and_consent_record_serde() {
    let entry = xavier::mesh::acl::NamespaceAclEntry {
        namespace_pattern: "docs/publico".to_string(),
        permissions: vec![xavier::enterprise::rbac::Permission::Read],
    };

    let serialized = serde_json::to_string(&entry).unwrap();
    let deserialized: xavier::mesh::acl::NamespaceAclEntry =
        serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized.namespace_pattern, "docs/publico");
    assert_eq!(deserialized.permissions.len(), 1);
    assert_eq!(
        deserialized.permissions[0],
        xavier::enterprise::rbac::Permission::Read
    );

    let consent = xavier::mesh::data_consent::ConsentRecord {
        namespace_filter: Some(vec!["docs/publico".to_string()]),
    };

    let consent_serialized = serde_json::to_string(&consent).unwrap();
    let consent_deserialized: xavier::mesh::data_consent::ConsentRecord =
        serde_json::from_str(&consent_serialized).unwrap();
    assert_eq!(
        consent_deserialized.namespace_filter.unwrap()[0],
        "docs/publico"
    );
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
        format!(
            "hash-{}",
            &xavier::crypto::hex_encode(hasher.finalize())[..16]
        )
    };

    // Assert fallback initially not revoked
    assert!(!xavier::mesh::DataConsentManager::is_token_revoked(&fallback_token_id).unwrap());

    // Revoke the fallback token_id
    xavier::mesh::DataConsentManager::revoke_consent(&fallback_token_id).unwrap();

    // Assert fallback now revoked
    assert!(xavier::mesh::DataConsentManager::is_token_revoked(&fallback_token_id).unwrap());
}
