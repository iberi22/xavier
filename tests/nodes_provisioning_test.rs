//! Comprehensive test suite for SWAL Node Provisioning (CLI + HTTP + Registry + Certs)
//!
//! Covers:
//! 1. Registry CRUD + disk persistence (survives re-opening SQLite database).
//! 2. Default visibility = `NodeVisibility::Private`.
//! 3. Rejection of personal SSH keys (`--key` flag).
//! 4. Rejection of CLI `--token` without `XAVIER_ALLOW_CLI_TOKEN=1`.
//! 5. Certificate issuance and verification: valid signature passes, signature from another wallet fails.
//! 6. Deprovisioning / revocation: normal revocation -> `Revoked`, failed remote deprovisioning -> `PartialRevocation`.
//! 7. HTTP endpoint `GET /mesh/public/nodes`: returns only public nodes, excludes private nodes, metadata only.

use axum::response::IntoResponse;
use ed25519_dalek::SigningKey;
use http_body_util::BodyExt;
use rand::rngs::OsRng;
use std::sync::Arc;
use tempfile::tempdir;

use xavier::nodes::{
    issue_cert, resolve_token, validate_no_personal_ssh_key, verify_cert, MockProvisioner,
    NodeRecord, NodeRegistry, NodeSecretsManager, NodeStatus, NodeVisibility, Provider,
    ProvisioningEngine, PublicNodeInfo,
};

fn sample_record(node_id: &str, provider: Provider, visibility: NodeVisibility) -> NodeRecord {
    NodeRecord {
        node_id: node_id.to_string(),
        provider,
        visibility,
        status: NodeStatus::Active,
        pubkey: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
        cert: None,
        host_key_fingerprint: Some("SHA256:mockfingerprint1234567890".to_string()),
        lease_id: Some("lease-sample-uuid-12345".to_string()),
        created_at: 1700000000,
        last_heartbeat: Some(1700000500),
    }
}

// ---------------------------------------------------------------------------
// 1. Registry persistence & CRUD
// ---------------------------------------------------------------------------

#[test]
fn test_registry_persistence_survives_db_reopen() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("nodes_test_registry.db");

    let rec1 = sample_record(
        "xv1-node-supabase-1",
        Provider::Supabase,
        NodeVisibility::Public,
    );
    let rec2 = sample_record("xv1-node-neon-2", Provider::Neon, NodeVisibility::Private);

    // Initial session: register records
    {
        let registry = NodeRegistry::open_path(&db_path).unwrap();
        registry.register(&rec1).unwrap();
        registry.register(&rec2).unwrap();
    }

    // Second session: re-open DB from disk and verify persistence
    {
        let registry = NodeRegistry::open_path(&db_path).unwrap();
        let loaded1 = registry.get("xv1-node-supabase-1").unwrap().unwrap();
        assert_eq!(loaded1.node_id, "xv1-node-supabase-1");
        assert_eq!(loaded1.provider, Provider::Supabase);
        assert_eq!(loaded1.visibility, NodeVisibility::Public);
        assert_eq!(loaded1.status, NodeStatus::Active);
        assert_eq!(
            loaded1.host_key_fingerprint.as_deref(),
            Some("SHA256:mockfingerprint1234567890")
        );

        let loaded2 = registry.get("xv1-node-neon-2").unwrap().unwrap();
        assert_eq!(loaded2.node_id, "xv1-node-neon-2");
        assert_eq!(loaded2.visibility, NodeVisibility::Private);

        // Update status and touch heartbeat
        registry
            .update_status("xv1-node-supabase-1", NodeStatus::Degraded)
            .unwrap();
        registry
            .touch_heartbeat("xv1-node-supabase-1", 1700001000)
            .unwrap();
    }

    // Third session: verify updates persisted after another reopen
    {
        let registry = NodeRegistry::open_path(&db_path).unwrap();
        let loaded = registry.get("xv1-node-supabase-1").unwrap().unwrap();
        assert_eq!(loaded.status, NodeStatus::Degraded);
        assert_eq!(loaded.last_heartbeat, Some(1700001000));

        // Remove node
        registry.remove("xv1-node-supabase-1").unwrap();
        assert!(registry.get("xv1-node-supabase-1").unwrap().is_none());
        assert_eq!(registry.list().unwrap().len(), 1);
    }
}

// ---------------------------------------------------------------------------
// 2. Default visibility = Private
// ---------------------------------------------------------------------------

#[test]
fn test_default_visibility_is_private() {
    // 1. Default trait impl
    assert_eq!(NodeVisibility::default(), NodeVisibility::Private);

    // 2. Parsing matches
    let parsed: NodeVisibility = "private".parse().unwrap();
    assert_eq!(parsed, NodeVisibility::Private);

    let parsed_public: NodeVisibility = "public".parse().unwrap();
    assert_eq!(parsed_public, NodeVisibility::Public);

    // 3. Invalid visibility fails
    assert!("invalid_vis".parse::<NodeVisibility>().is_err());
}

// ---------------------------------------------------------------------------
// 3. Rejection of personal SSH keys
// ---------------------------------------------------------------------------

#[test]
fn test_rejection_of_personal_ssh_key() {
    // Supplying any personal key path must be strictly rejected
    let res = validate_no_personal_ssh_key(Some("~/.ssh/id_rsa"));
    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    assert!(err_msg.contains("strictly prohibited"));
    assert!(err_msg.contains("REQ-030"));

    let res_ed = validate_no_personal_ssh_key(Some("/home/user/.ssh/id_ed25519"));
    assert!(res_ed.is_err());

    // None or empty string passes (dedicated keypair generated automatically)
    assert!(validate_no_personal_ssh_key(None).is_ok());
    assert!(validate_no_personal_ssh_key(Some("")).is_ok());
    assert!(validate_no_personal_ssh_key(Some("   ")).is_ok());
}

// ---------------------------------------------------------------------------
// 4. Rejection of CLI token without test env var
// ---------------------------------------------------------------------------

#[test]
fn test_rejection_of_cli_token_without_allow_env() {
    std::env::remove_var("XAVIER_ALLOW_CLI_TOKEN");
    std::env::remove_var("XAVIER_NODE_TOKEN");

    // Passing token via CLI argument without XAVIER_ALLOW_CLI_TOKEN=1 must fail
    let res = resolve_token(Some("sbp_sensitive_secret_token"));
    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    assert!(err_msg.contains("Security violation"));
    assert!(err_msg.contains("XAVIER_ALLOW_CLI_TOKEN=1"));

    // Enabling test mode env var allows flag
    std::env::set_var("XAVIER_ALLOW_CLI_TOKEN", "1");
    let res_allowed = resolve_token(Some("sbp_test_token_1234"));
    assert!(res_allowed.is_ok());
    assert_eq!(res_allowed.unwrap(), "sbp_test_token_1234");
    std::env::remove_var("XAVIER_ALLOW_CLI_TOKEN");

    // Production mode: reading from XAVIER_NODE_TOKEN env var is allowed
    std::env::set_var("XAVIER_NODE_TOKEN", "sbp_prod_env_token_5678");
    let res_env = resolve_token(None);
    assert!(res_env.is_ok());
    assert_eq!(res_env.unwrap(), "sbp_prod_env_token_5678");
    std::env::remove_var("XAVIER_NODE_TOKEN");
}

// ---------------------------------------------------------------------------
// 5. Certificate issuance & wallet isolation
// ---------------------------------------------------------------------------

#[test]
fn test_cert_issuance_and_wallet_isolation() {
    let wallet1_sk = SigningKey::generate(&mut OsRng);
    let wallet1_pk = wallet1_sk.verifying_key().to_bytes();

    let wallet2_sk = SigningKey::generate(&mut OsRng);
    let wallet2_pk = wallet2_sk.verifying_key().to_bytes();

    let node_sk = SigningKey::generate(&mut OsRng);
    let node_pk = node_sk.verifying_key().to_bytes();
    let node_id = "xv1-node-crypto-test";

    // 1. Issue cert signed by wallet1
    let cert = issue_cert(&wallet1_sk, &node_pk, node_id, 3600).unwrap();
    assert_eq!(cert.node_id, node_id);
    assert!(!cert.is_expired());

    // 2. Verification against wallet1's expected pubkey succeeds
    let valid_wallet1 = verify_cert(&cert, Some(&wallet1_pk)).unwrap();
    assert!(
        valid_wallet1,
        "Valid signature must verify with wallet 1 key"
    );

    // 3. Verification against wallet2's expected pubkey FAILS (wallet isolation)
    let valid_wallet2 = verify_cert(&cert, Some(&wallet2_pk)).unwrap();
    assert!(
        !valid_wallet2,
        "Certificate from wallet 1 must NOT verify against wallet 2"
    );

    // 4. Tampering node_id in cert payload fails verification
    let mut tampered_cert = cert.clone();
    tampered_cert.node_id = "xv1-node-tampered-id".to_string();
    assert!(!verify_cert(&tampered_cert, Some(&wallet1_pk)).unwrap());

    // 5. Expired cert is rejected
    let mut expired_cert = cert.clone();
    expired_cert.expiry = 1000;
    assert!(!verify_cert(&expired_cert, Some(&wallet1_pk)).unwrap());
}

// ---------------------------------------------------------------------------
// 6. Revocation lifecycle & partial revocation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_node_revocation_and_partial_revocation() {
    let registry = Arc::new(NodeRegistry::open_in_memory().unwrap());
    let secrets = NodeSecretsManager::new();
    let wallet_sk = SigningKey::generate(&mut OsRng);

    // Case A: Successful remote deprovisioning -> Revoked
    {
        let provisioner = Arc::new(MockProvisioner::new());
        let engine = ProvisioningEngine::new(registry.clone(), secrets.clone(), provisioner);

        let record = engine
            .provision_node(
                &wallet_sk,
                Provider::Supabase,
                NodeVisibility::Private,
                Some("sbp_test_token_a".to_string()),
                None,
                None,
                3600,
                3600,
            )
            .await
            .unwrap();

        assert_eq!(record.status, NodeStatus::Active);

        let status = engine.remove_node(&record.node_id).await.unwrap();
        assert_eq!(status, NodeStatus::Revoked);

        let rec_after = registry.get(&record.node_id).unwrap().unwrap();
        assert_eq!(rec_after.status, NodeStatus::Revoked);
    }

    // Case B: Failed remote deprovisioning -> PartialRevocation
    {
        let failing_provisioner = Arc::new(MockProvisioner::with_failing_deprovision(
            "Remote BaaS endpoint returned 502 Bad Gateway",
        ));
        let engine_failing =
            ProvisioningEngine::new(registry.clone(), secrets.clone(), failing_provisioner);

        let record = engine_failing
            .provision_node(
                &wallet_sk,
                Provider::Neon,
                NodeVisibility::Private,
                Some("neon_test_token_b".to_string()),
                None,
                None,
                3600,
                3600,
            )
            .await
            .unwrap();

        let status = engine_failing.remove_node(&record.node_id).await.unwrap();
        assert_eq!(status, NodeStatus::PartialRevocation);

        let rec_after = registry.get(&record.node_id).unwrap().unwrap();
        assert_eq!(rec_after.status, NodeStatus::PartialRevocation);
    }
}

// ---------------------------------------------------------------------------
// 7. HTTP GET /mesh/public/nodes endpoint
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_http_get_mesh_public_nodes_filters_private_nodes() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("http_test_registry.db");
    std::env::set_var("XAVIER_NODE_REGISTRY_PATH", db_path.to_str().unwrap());

    // Populate registry with 1 public node and 2 private nodes
    {
        let registry = NodeRegistry::open_path(&db_path).unwrap();
        let pub_node = sample_record(
            "xv1-public-visible",
            Provider::Supabase,
            NodeVisibility::Public,
        );
        let priv_node1 = sample_record(
            "xv1-private-hidden-1",
            Provider::Neon,
            NodeVisibility::Private,
        );
        let priv_node2 = sample_record(
            "xv1-private-hidden-2",
            Provider::Vps,
            NodeVisibility::Private,
        );

        registry.register(&pub_node).unwrap();
        registry.register(&priv_node1).unwrap();
        registry.register(&priv_node2).unwrap();
    }

    // Call the adapter HTTP handler
    let response =
        xavier::adapters::inbound::http::handlers::nodes::list_public_nodes_handler().await;
    let response_into = response.into_response();
    assert_eq!(response_into.status(), axum::http::StatusCode::OK);

    let body_bytes = response_into
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let public_nodes: Vec<PublicNodeInfo> = serde_json::from_slice(&body_bytes).unwrap();

    // 1. Only 1 public node returned
    assert_eq!(public_nodes.len(), 1);
    assert_eq!(public_nodes[0].node_id, "xv1-public-visible");
    assert_eq!(public_nodes[0].provider, Provider::Supabase);
    assert_eq!(public_nodes[0].status, NodeStatus::Active);
    assert_eq!(
        public_nodes[0].pubkey,
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    );
    assert_eq!(public_nodes[0].last_heartbeat, Some(1700000500));

    // 2. Verify complete absence of private nodes
    let json_str = String::from_utf8_lossy(&body_bytes);
    assert!(!json_str.contains("xv1-private-hidden-1"));
    assert!(!json_str.contains("xv1-private-hidden-2"));
    assert!(!json_str.contains("lease-sample-uuid-12345"));

    std::env::remove_var("XAVIER_NODE_REGISTRY_PATH");
}
