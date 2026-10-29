//! feat-incident-ticket-dedup: Alert tickets carry a value-free fingerprint so repeated samples dedupe.
use serial_test::serial;
use std::fs;
use xavier::self_manage::{ticket_create, TicketCreateArgs};

#[path = "router_harness/mod.rs"]
mod router_harness;

xavier::isolate_test_process!();

#[test]
#[serial]
fn incident_ticket_deduplicates_on_same_fingerprint() {
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

    let args1 = TicketCreateArgs {
        title: "[Incident] CRITICAL alert".to_string(),
        body: "Alert body".to_string(),
        labels: Some(vec!["runtime".to_string(), "incident".to_string()]),
        severity: "critical".to_string(),
        fingerprint: Some("alert:cpu:critical".to_string()),
        backend: Some("maloca".to_string()),
    };

    let res1 = ticket_create(args1).expect("create first ticket");
    assert!(!res1.deduplicated, "first ticket should be fresh");
    assert_eq!(res1.backend, "maloca");

    let args2 = TicketCreateArgs {
        title: "[Incident] CRITICAL alert new sample".to_string(),
        body: "Alert body new sample".to_string(),
        labels: Some(vec!["runtime".to_string(), "incident".to_string()]),
        severity: "critical".to_string(),
        fingerprint: Some("alert:cpu:critical".to_string()),
        backend: Some("maloca".to_string()),
    };

    let res2 = ticket_create(args2).expect("create duplicate ticket");
    assert!(res2.deduplicated, "second ticket should be deduplicated");
    assert_eq!(res2.id, res1.id, "should map to the same ticket id");
}

#[test]
#[serial]
fn incident_ticket_creates_fresh_for_different_fingerprint() {
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

    let args1 = TicketCreateArgs {
        title: "[Incident] WARN alert".to_string(),
        body: "Alert body warn".to_string(),
        labels: Some(vec!["runtime".to_string(), "incident".to_string()]),
        severity: "warn".to_string(),
        fingerprint: Some("alert:mem:warn".to_string()),
        backend: Some("maloca".to_string()),
    };

    let res1 = ticket_create(args1.clone()).expect("create first ticket");
    assert!(!res1.deduplicated, "first ticket should be fresh");

    let args2 = TicketCreateArgs {
        fingerprint: Some("alert:cpu:warn".to_string()),
        ..args1
    };
    let res2 = ticket_create(args2).expect("create ticket with different fingerprint");
    assert!(
        !res2.deduplicated,
        "different fingerprint should create a fresh ticket"
    );
    assert_ne!(
        res1.id, res2.id,
        "different fingerprints need distinct tickets"
    );

    let tickets = xavier::maloca::MalocaStore::open_default().list_support();
    assert_eq!(
        tickets.len(),
        2,
        "both tickets must be persisted in the same store"
    );
    assert!(tickets.iter().any(|ticket| ticket.id == res1.id));
    assert!(tickets.iter().any(|ticket| ticket.id == res2.id));
}
