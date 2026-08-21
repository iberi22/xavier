use std::sync::Arc;
use std::thread;
use xavier::mesh::data_consent::{ConsentLevel, DataConsentManager};
use xavier::mesh::node::NodeId;
use xavier::mesh::protocol::{ChunkRef, MeshManifest};
use xavier::mesh::p2p::sync_filter::{
    SyncFilter, SyncFilterConfig, SyncFilterDecision, SyncFilterError,
};

#[test]
fn test_sync_filter_initialization_defaults_and_config() {
    let filter = SyncFilter::default();
    assert!(!filter.opt_in());
    assert!(filter.is_sync_enabled());
    assert!(!filter.is_strict_sanitization());
    assert!(!filter.is_outbound_allowed());

    let stats = filter.stats();
    assert!(!stats.opt_in);
    assert_eq!(stats.blocked_outbound_count, 0);
    assert_eq!(stats.allowed_outbound_count, 0);
    assert_eq!(stats.local_allowed_count, 0);

    let config = SyncFilterConfig {
        opt_in: true,
        sync_enabled: true,
        strict_sanitization: true,
    };
    let configured = SyncFilter::from_config(config);
    assert!(configured.opt_in());
    assert!(configured.is_sync_enabled());
    assert!(configured.is_strict_sanitization());
    assert!(configured.is_outbound_allowed());
}

#[test]
fn test_local_usage_always_allowed() {
    let filter = SyncFilter::new(false);
    assert!(!filter.opt_in());

    // Local usage must always be permitted regardless of opt_in status
    for _ in 0..10 {
        assert!(filter.is_local_usage_allowed());
    }

    let stats = filter.stats();
    assert_eq!(stats.local_allowed_count, 10);
    assert_eq!(stats.blocked_outbound_count, 0);
}

#[test]
fn test_outbound_blocked_when_opt_in_false() {
    let filter = SyncFilter::new(false);
    assert_eq!(
        filter.evaluate_outbound(),
        SyncFilterDecision::BlockedOptInRequired
    );

    let payload = serde_json::json!({"data": "sensitive_local_record"});
    assert_eq!(
        filter.filter_outbound_replication(&payload),
        Err(SyncFilterError::OptInRequired)
    );

    assert_eq!(
        filter.filter_outbound_chunk("chunk_123", b"raw_sqlite_bytes"),
        Err(SyncFilterError::OptInRequired)
    );

    let manifest = MeshManifest {
        node_id: NodeId("node_local".into()),
        chunks: vec![ChunkRef {
            hash: "c1".into(),
            document_count: 5,
            created_at: 100,
        }],
        generated_at: 100,
    };
    assert_eq!(
        filter.filter_outbound_manifest(&manifest).unwrap_err(),
        SyncFilterError::OptInRequired
    );

    assert_eq!(
        filter.filter_outbound_memory("mem_001", "local SQLite content"),
        Err(SyncFilterError::OptInRequired)
    );

    let stats = filter.stats();
    assert_eq!(stats.blocked_outbound_count, 5);
    assert_eq!(stats.allowed_outbound_count, 0);
}

#[test]
fn test_outbound_allowed_when_opt_in_true() {
    let filter = SyncFilter::new(true);
    assert_eq!(filter.evaluate_outbound(), SyncFilterDecision::Allowed);

    let payload = serde_json::json!({"data": "public_mesh_record"});
    let res = filter.filter_outbound_replication(&payload).expect("should succeed");
    assert_eq!(res, payload);

    assert!(filter
        .filter_outbound_chunk("chunk_123", b"raw_bytes")
        .is_ok());

    let manifest = MeshManifest {
        node_id: NodeId("node_remote".into()),
        chunks: vec![],
        generated_at: 200,
    };
    let manifest_res = filter
        .filter_outbound_manifest(&manifest)
        .expect("should succeed");
    assert_eq!(manifest_res.node_id, NodeId("node_remote".into()));

    let mem_res = filter
        .filter_outbound_memory("mem_002", "shared memory content")
        .expect("should succeed");
    assert_eq!(mem_res, "shared memory content");

    let stats = filter.stats();
    assert_eq!(stats.allowed_outbound_count, 5);
    assert_eq!(stats.blocked_outbound_count, 0);
}

#[test]
fn test_sync_disabled_toggle() {
    let filter = SyncFilter::new(true);
    assert!(filter.is_outbound_allowed());

    filter.set_sync_enabled(false);
    assert!(!filter.is_sync_enabled());
    assert!(!filter.is_outbound_allowed());

    assert_eq!(
        filter.evaluate_outbound(),
        SyncFilterDecision::BlockedDisabled
    );

    let payload = serde_json::json!({"test": 1});
    match filter.filter_outbound_replication(&payload) {
        Err(SyncFilterError::SyncDisabled(reason)) => {
            assert!(reason.contains("off"));
        }
        other => panic!("expected SyncDisabled, got {:?}", other),
    }

    assert_eq!(filter.stats().blocked_outbound_count, 2);
}

#[test]
fn test_dynamic_opt_in_toggling() {
    let filter = SyncFilter::new(false);
    assert_eq!(
        filter.evaluate_outbound(),
        SyncFilterDecision::BlockedOptInRequired
    );

    filter.set_opt_in(true);
    assert_eq!(filter.evaluate_outbound(), SyncFilterDecision::Allowed);

    filter.set_opt_in(false);
    assert_eq!(
        filter.evaluate_outbound(),
        SyncFilterDecision::BlockedOptInRequired
    );

    let stats = filter.stats();
    assert_eq!(stats.blocked_outbound_count, 2);
    assert_eq!(stats.allowed_outbound_count, 1);
}

#[test]
fn test_consent_manager_integration() {
    let node_id = NodeId("xv1-data-node".to_string());
    let mut mgr = DataConsentManager::new(node_id);
    mgr.set_consent("cpu_usage", ConsentLevel::Metadata);
    mgr.set_consent("private_memory", ConsentLevel::None);

    let filter = SyncFilter::new(false);
    let sample = serde_json::json!({
        "node_id": "xv1-data-node",
        "metric_name": "cpu_usage",
        "value": 42.0,
        "timestamp": 12345
    });

    // When opt_in is false -> returns OptInRequired
    assert_eq!(
        filter.filter_with_consent_manager(&mgr, "cpu_usage", &sample),
        Err(SyncFilterError::OptInRequired)
    );

    // Enable opt_in
    filter.set_opt_in(true);

    // ConsentLevel::Metadata -> sanitizes and returns metadata
    let sanitized = filter
        .filter_with_consent_manager(&mgr, "cpu_usage", &sample)
        .expect("should succeed")
        .expect("should return value");
    assert!(sanitized.get("metric_name").is_some());
    assert!(sanitized.get("node_id").is_none());

    // ConsentLevel::None -> sanitizes to None
    let private_res = filter
        .filter_with_consent_manager(&mgr, "private_memory", &sample)
        .expect("should succeed");
    assert!(private_res.is_none());
}

#[test]
fn test_sanitization_edge_cases() {
    let filter = SyncFilter::new(true);

    // Empty chunk hash or data
    assert_eq!(
        filter.filter_outbound_chunk("", b"data"),
        Err(SyncFilterError::SanitizationFailed(
            "Empty chunk hash or payload".into()
        ))
    );
    assert_eq!(
        filter.filter_outbound_chunk("hash1", b""),
        Err(SyncFilterError::SanitizationFailed(
            "Empty chunk hash or payload".into()
        ))
    );

    // Empty memory ID
    assert_eq!(
        filter.filter_outbound_memory("", "content"),
        Err(SyncFilterError::SanitizationFailed(
            "Memory ID cannot be empty".into()
        ))
    );
}

#[test]
fn test_stats_reset_and_strict_sanitization() {
    let filter = SyncFilter::new(true);
    filter.set_strict_sanitization(true);
    assert!(filter.is_strict_sanitization());

    filter.is_local_usage_allowed();
    let _ = filter.evaluate_outbound();

    let stats_before = filter.stats();
    assert_eq!(stats_before.local_allowed_count, 1);
    assert_eq!(stats_before.allowed_outbound_count, 1);

    filter.reset_stats();

    let stats_after = filter.stats();
    assert_eq!(stats_after.local_allowed_count, 0);
    assert_eq!(stats_after.allowed_outbound_count, 0);
    assert_eq!(stats_after.blocked_outbound_count, 0);
    assert!(stats_after.opt_in);
}

#[test]
fn test_thread_safety_concurrent_access() {
    let filter = Arc::new(SyncFilter::new(false));
    let mut handles = vec![];

    for i in 0..10 {
        let f = Arc::clone(&filter);
        handles.push(thread::spawn(move || {
            for j in 0..100 {
                f.is_local_usage_allowed();
                if (i + j) % 2 == 0 {
                    f.set_opt_in(true);
                } else {
                    f.set_opt_in(false);
                }
                let _ = f.evaluate_outbound();
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let stats = filter.stats();
    assert_eq!(stats.local_allowed_count, 1000);
    assert_eq!(
        stats.blocked_outbound_count + stats.allowed_outbound_count,
        1000
    );
}

#[test]
fn test_decision_and_error_methods() {
    let allowed = SyncFilterDecision::Allowed;
    assert!(allowed.is_allowed());
    assert!(!allowed.is_blocked());

    let blocked_opt_in = SyncFilterDecision::BlockedOptInRequired;
    assert!(!blocked_opt_in.is_allowed());
    assert!(blocked_opt_in.is_blocked());

    let blocked_disabled = SyncFilterDecision::BlockedDisabled;
    assert!(!blocked_disabled.is_allowed());
    assert!(blocked_disabled.is_blocked());

    let blocked_sanitization = SyncFilterDecision::BlockedSanitizationFailed("err".into());
    assert!(!blocked_sanitization.is_allowed());
    assert!(blocked_sanitization.is_blocked());

    let json = serde_json::to_string(&allowed).unwrap();
    let de: SyncFilterDecision = serde_json::from_str(&json).unwrap();
    assert_eq!(de, SyncFilterDecision::Allowed);

    let err_opt_in = SyncFilterError::OptInRequired;
    assert_eq!(
        format!("{}", err_opt_in),
        "Outbound sync blocked: Data Node opt-in consent is required (opt_in == false)"
    );

    let err_disabled = SyncFilterError::SyncDisabled("off".into());
    assert_eq!(
        format!("{}", err_disabled),
        "Outbound sync blocked: sync disabled (off)"
    );

    let err_sanitization = SyncFilterError::SanitizationFailed("bad schema".into());
    assert_eq!(
        format!("{}", err_sanitization),
        "Outbound sync blocked: sanitization failed (bad schema)"
    );

    let err_policy = SyncFilterError::PolicyViolation("no share".into());
    assert_eq!(
        format!("{}", err_policy),
        "Outbound sync blocked by policy: no share"
    );
}
