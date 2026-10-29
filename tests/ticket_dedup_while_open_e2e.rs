//! feat-ticket-dedup-while-open: Self-manage ticket deduplication holds only while the earlier ticket is still open.
use serial_test::serial;
use std::fs;
use xavier::self_manage::{ticket_create, TicketCreateArgs};

#[path = "router_harness/mod.rs"]
mod router_harness;

xavier::isolate_test_process!();

#[test]
#[serial]
fn ticket_dedup_creates_new_ticket_when_previous_is_resolved() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();
    let _env = router_harness::RestoreEnv::set(&[
        ("HOME", data_dir.as_path()),
        ("XAVIER_CONFIG_PATH", data_dir.as_path()),
        ("XAVIER_RUNTIME_STATE_PATH", data_dir.as_path()),
        ("XAVIER_STATE_DIR", data_dir.as_path()),
        ("XAVIER_DATA_DIR", data_dir.as_path()),
    ]);

    let args = TicketCreateArgs {
        title: "Test recurrence ticket".to_string(),
        body: "Ticket body".to_string(),
        labels: Some(vec![]),
        severity: "warn".to_string(),
        fingerprint: Some("test:recurrence".to_string()),
        backend: Some("maloca".to_string()),
    };

    let res1 = ticket_create(args.clone()).expect("create first ticket");
    assert!(!res1.deduplicated, "first ticket should be fresh");

    let store = xavier::maloca::MalocaStore::open_default();
    store
        .update_support_status(
            &res1.id,
            xavier::maloca::support_flow::SupportStatus::Resolved,
        )
        .expect("update status to resolved");

    let res2 = ticket_create(args).expect("create second ticket after resolution");
    assert!(
        !res2.deduplicated,
        "second ticket should be fresh because the first one is resolved"
    );
    assert_ne!(res2.id, res1.id, "should map to a new ticket id");
}

#[test]
#[serial]
fn ticket_dedup_deduplicates_when_open() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();
    let _env = router_harness::RestoreEnv::set(&[
        ("HOME", data_dir.as_path()),
        ("XAVIER_CONFIG_PATH", data_dir.as_path()),
        ("XAVIER_RUNTIME_STATE_PATH", data_dir.as_path()),
        ("XAVIER_STATE_DIR", data_dir.as_path()),
        ("XAVIER_DATA_DIR", data_dir.as_path()),
    ]);

    let args = TicketCreateArgs {
        title: "Test recurrence ticket".to_string(),
        body: "Ticket body".to_string(),
        labels: Some(vec![]),
        severity: "warn".to_string(),
        fingerprint: Some("test:recurrence".to_string()),
        backend: Some("maloca".to_string()),
    };

    let res1 = ticket_create(args.clone()).expect("create first ticket");
    assert!(!res1.deduplicated, "first ticket should be fresh");

    let res2 = ticket_create(args).expect("create second ticket");
    assert!(
        res2.deduplicated,
        "second ticket should be deduped since the first is still open"
    );
    assert_eq!(res2.id, res1.id, "should map to the same ticket id");
}
