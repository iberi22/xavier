use rusqlite::Connection;
use tempfile::NamedTempFile;
use xavier::server::maloca::rewards::{ContributionTracker, ContributionTrackerConfig};

#[test]
fn test_in_memory_tracker_initialization() {
    let key = [0x42u8; 32];
    let tracker = ContributionTracker::in_memory(key).expect("Failed to create in-memory tracker");
    let metrics = tracker.get_metrics("node_01", "2026-03-30").unwrap();
    assert!(metrics.is_none());
}

#[test]
fn test_file_backed_tracker_initialization() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();
    let key = [0x12u8; 32];

    let config = ContributionTrackerConfig {
        db_path: db_path.clone(),
        encryption_key: key,
    };

    let tracker = ContributionTracker::new(config).expect("Failed to create file-backed tracker");

    // Record activity
    let recorded = tracker
        .record_activity_for_date("node_01", "2026-03-30", 3600, 1024 * 1024 * 1024)
        .unwrap();

    assert_eq!(recorded.node_id, "node_01");
    assert_eq!(recorded.record_date, "2026-03-30");
    assert_eq!(recorded.active_uptime_secs, 3600);
    assert_eq!(recorded.bytes_sent, 1024 * 1024 * 1024);
    // 1 hour uptime (1.0 pt) + 1 GB transferred (2.0 pt) = 3.0 pts
    assert_eq!(recorded.contribution_score, 3.0);

    // Verify persistence by opening a new tracker instance with same file & key
    let config2 = ContributionTrackerConfig {
        db_path,
        encryption_key: key,
    };
    let tracker2 = ContributionTracker::new(config2).unwrap();
    let fetched = tracker2
        .get_metrics("node_01", "2026-03-30")
        .unwrap()
        .expect("Record should exist");

    assert_eq!(fetched.node_id, "node_01");
    assert_eq!(fetched.contribution_score, 3.0);
}

#[test]
fn test_metric_aggregation_single_and_multi_day() {
    let key = [0xAAu8; 32];
    let tracker = ContributionTracker::in_memory(key).unwrap();

    // Day 1: 1800 secs (0.5 hrs), 512MB (0.5 GB) -> score = 0.5 + 1.0 = 1.5
    let day1 = tracker
        .record_activity_for_date("node_agg", "2026-03-29", 1800, 512 * 1024 * 1024)
        .unwrap();
    assert_eq!(day1.contribution_score, 1.5);

    // Day 1 accumulation: add another 1800 secs and 512MB -> 3600 secs (1.0 hr), 1GB (2.0 pt) -> score = 3.0
    let day1_updated = tracker
        .record_activity_for_date("node_agg", "2026-03-29", 1800, 512 * 1024 * 1024)
        .unwrap();
    assert_eq!(day1_updated.active_uptime_secs, 3600);
    assert_eq!(day1_updated.bytes_sent, 1024 * 1024 * 1024);
    assert_eq!(day1_updated.contribution_score, 3.0);

    // Day 2: 7200 secs (2.0 hrs), 2GB (4.0 pt) -> score = 6.0
    let day2 = tracker
        .record_activity_for_date("node_agg", "2026-03-30", 7200, 2 * 1024 * 1024 * 1024)
        .unwrap();
    assert_eq!(day2.contribution_score, 6.0);

    // Historical list
    let list = tracker.list_metrics_for_node("node_agg").unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].record_date, "2026-03-30"); // descending order
    assert_eq!(list[1].record_date, "2026-03-29");

    // Total cumulative score: 3.0 + 6.0 = 9.0
    let total_score = tracker.get_total_contribution_score("node_agg").unwrap();
    assert_eq!(total_score, 9.0);
}

#[test]
fn test_encryption_privacy_and_wrong_key() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_path_buf();
    let key1 = [0x11u8; 32];
    let key2 = [0x22u8; 32];

    let config1 = ContributionTrackerConfig {
        db_path: db_path.clone(),
        encryption_key: key1,
    };
    let tracker1 = ContributionTracker::new(config1).unwrap();
    tracker1
        .record_activity_for_date("node_enc", "2026-03-30", 3600, 10000)
        .unwrap();

    // Directly open SQLite raw connection to verify payload is encrypted BLOB, not plaintext JSON
    let raw_conn = Connection::open(&db_path).unwrap();
    let mut stmt = raw_conn
        .prepare("SELECT encrypted_payload FROM data_node_metrics WHERE node_id = 'node_enc'")
        .unwrap();
    let mut rows = stmt.query([]).unwrap();
    let row = rows.next().unwrap().unwrap();
    let raw_blob: Vec<u8> = row.get(0).unwrap();

    // Verify raw blob does not contain plaintext string "node_enc" or "contribution_score"
    let raw_str = String::from_utf8_lossy(&raw_blob);
    assert!(!raw_str.contains("node_enc"));
    assert!(!raw_str.contains("contribution_score"));

    // Verify reading with wrong key returns an error due to AES-256-GCM authentication failure
    let config2 = ContributionTrackerConfig {
        db_path,
        encryption_key: key2,
    };
    let tracker2 = ContributionTracker::new(config2).unwrap();
    let result = tracker2.get_metrics("node_enc", "2026-03-30");
    assert!(result.is_err());
}

#[test]
fn test_record_activity_current_date() {
    let key = [0x55u8; 32];
    let tracker = ContributionTracker::in_memory(key).unwrap();

    let metrics = tracker.record_activity("node_today", 3600, 1024).unwrap();
    assert_eq!(metrics.node_id, "node_today");
    assert_eq!(metrics.active_uptime_secs, 3600);
    assert_eq!(metrics.bytes_sent, 1024);

    let list = tracker.list_metrics_for_node("node_today").unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].node_id, "node_today");
}

#[test]
fn test_edge_cases_zero_and_saturating_overflow() {
    let key = [0x99u8; 32];
    let tracker = ContributionTracker::in_memory(key).unwrap();

    // Zero uptime and zero bytes
    let zero_m = tracker
        .record_activity_for_date("node_edge", "2026-03-30", 0, 0)
        .unwrap();
    assert_eq!(zero_m.contribution_score, 0.0);

    // Large numbers and saturating additions
    let large_m = tracker
        .record_activity_for_date("node_edge", "2026-03-30", u64::MAX, u64::MAX)
        .unwrap();
    assert_eq!(large_m.active_uptime_secs, u64::MAX);
    assert_eq!(large_m.bytes_sent, u64::MAX);

    // Score calculation rounding check
    let calc = ContributionTracker::calculate_score(1234, 56789012);
    assert!(calc > 0.0);
}

#[test]
fn test_multiple_data_nodes_isolation() {
    let key = [0x33u8; 32];
    let tracker = ContributionTracker::in_memory(key).unwrap();

    tracker
        .record_activity_for_date("node_A", "2026-03-30", 3600, 1000)
        .unwrap();
    tracker
        .record_activity_for_date("node_B", "2026-03-30", 7200, 2000)
        .unwrap();

    let metrics_a = tracker.list_metrics_for_node("node_A").unwrap();
    let metrics_b = tracker.list_metrics_for_node("node_B").unwrap();

    assert_eq!(metrics_a.len(), 1);
    assert_eq!(metrics_b.len(), 1);
    assert_eq!(metrics_a[0].node_id, "node_A");
    assert_eq!(metrics_b[0].node_id, "node_B");

    let score_a = tracker.get_total_contribution_score("node_A").unwrap();
    let score_b = tracker.get_total_contribution_score("node_B").unwrap();
    assert_ne!(score_a, score_b);
}
