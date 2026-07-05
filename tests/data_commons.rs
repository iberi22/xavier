use xavier::agents::anomaly_scanner::AnomalyScannerAgent;
use xavier::mesh::crypto_gating::{AccessRequest, CryptoGatingService};
use xavier::mesh::governance::DaoGovernanceSystem;
use xavier::mesh::telemetry::TelemetryPayload;

#[tokio::test]
async fn test_data_commons_phase_2_e2e_flow() {
    // 1. Emitting Node
    // Generates a mock crash log with PII
    let raw_crash = "Panic! Thread 'main' panicked at src/main.rs:42: Error processing user email belal@example.com from IP 192.168.1.50 in path C:\\Users\\belal\\project\\";

    // The telemetry module automatically scrubs PII when creating the payload
    let payload = TelemetryPayload::new_scrubbed("panic", raw_crash, None);

    // Verify Anonymization
    assert!(!payload.sanitized_message.contains("belal@example.com"));
    assert!(!payload.sanitized_message.contains("192.168.1.50"));
    assert!(payload.sanitized_message.contains("[REDACTED]"));

    // Serialize to JSON
    let raw_json = serde_json::to_string(&payload).unwrap();

    // 2. Encryption (Local Node)
    let crypto_service = CryptoGatingService::new();
    let encrypted_payload = crypto_service.encrypt_payload(&raw_json);

    // Validate IPFS CID generation
    assert!(encrypted_payload.ipfs_cid.starts_with("Qm"));
    println!(
        "Simulating IPFS Upload... Payload pinned at CID: {}",
        encrypted_payload.ipfs_cid
    );

    // 3. Network Storage (Simulated)
    // The `encrypted_payload` is stored in the Mesh/Supabase.

    // 4. Attacker Node (Tries to decrypt without valid Wallet/Signature)
    let invalid_access_req = AccessRequest {
        wallet_address: "ATTACKER_WALLET".to_string(),
        signature: "invalid_sig".to_string(),
    };

    // Attacker fails validation, doesn't get the symmetric key
    let access_result = crypto_service.validate_access(&invalid_access_req);
    assert!(access_result.is_err());

    // 5. Maintainer Node (Pays XAV or is whitelisted, uses correct signature)
    let valid_access_req = AccessRequest {
        wallet_address: "MAINTAINER_NODE_1".to_string(),
        signature: "valid_signature".to_string(),
    };

    // Maintainer gets the symmetric key
    let symmetric_key = crypto_service.validate_access(&valid_access_req).unwrap();

    // Decrypt the payload
    let decrypted_json = crypto_service
        .decrypt_payload(&encrypted_payload, &symmetric_key)
        .unwrap();
    assert_eq!(raw_json, decrypted_json);

    // 6. Autonomous Remediation (Maintainer runs AnomalyScanner)
    let scanner = AnomalyScannerAgent::new();
    let report = scanner.scan_telemetry(&decrypted_json).await;

    // The payload was a Rust Panic
    assert_eq!(report.anomaly_type, "Rust Core Panic");
    assert!(report.requires_human);
    assert!(!report.is_false_positive);
    assert_eq!(report.cluster_id.as_deref(), Some("CLUSTER_CORE_RUST"));

    // The agent decides to group it into Epics and wait for DAO Vote
    let action = scanner.execute_remediation(&report);
    assert!(action.contains("DAO Governance Vote"));

    // 7. DAO Governance Voting
    let mut dao = DaoGovernanceSystem::new();
    dao.submit_proposal(
        report.cluster_id.as_ref().unwrap(),
        "Fix Rust Panic",
        "Crash reported in core Rust.",
    )
    .await;

    // The issue is locked (PR not allowed)
    assert!(
        !dao.active_proposals
            .get("CLUSTER_CORE_RUST")
            .unwrap()
            .is_approved_for_pr
    );

    // Community votes (4 upvotes, 0 downvotes => Not enough quorum)
    for _ in 0..4 {
        dao.cast_vote("CLUSTER_CORE_RUST", true).await.unwrap();
    }
    assert!(
        !dao.active_proposals
            .get("CLUSTER_CORE_RUST")
            .unwrap()
            .is_approved_for_pr
    );

    // 5th vote comes in! (100% approval, minimum quorum met)
    dao.cast_vote("CLUSTER_CORE_RUST", true).await.unwrap();

    // The PR is unlocked!
    let final_proposal = dao.active_proposals.get("CLUSTER_CORE_RUST").unwrap();
    assert!(final_proposal.is_approved_for_pr);

    // A maintainer is randomly assigned to claim the bounty!
    assert!(final_proposal.assigned_maintainer.is_some());
    println!(
        "PR unlocked! Randomly assigned to: {}",
        final_proposal.assigned_maintainer.as_ref().unwrap()
    );

    println!("Data Commons Phase 2.2 E2E (Triage & Governance) passed successfully!");
}
