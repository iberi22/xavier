//! feat-incident-ticket-dedup: Alert tickets carry a value-free fingerprint so repeated samples dedupe.
#![allow(unused_imports)]

use std::fs;
use xavier::self_manage::{ticket_create, TicketCreateArgs};

xavier::isolate_test_process!();

#[test]
fn incident_ticket_deduplicates_on_same_fingerprint() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();
    std::env::set_var("HOME", &data_dir);
    std::env::set_var("XAVIER_CONFIG_PATH", &data_dir);
    std::env::set_var("XAVIER_RUNTIME_STATE_PATH", &data_dir);
    std::env::set_var("XAVIER_STATE_DIR", &data_dir);
    std::env::set_var("XAVIER_DATA_DIR", &data_dir);

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
fn incident_ticket_creates_fresh_for_different_fingerprint() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();
    std::env::set_var("HOME", &data_dir);
    std::env::set_var("XAVIER_CONFIG_PATH", &data_dir);
    std::env::set_var("XAVIER_RUNTIME_STATE_PATH", &data_dir);
    std::env::set_var("XAVIER_STATE_DIR", &data_dir);
    std::env::set_var("XAVIER_DATA_DIR", &data_dir);

    let args3 = TicketCreateArgs {
        title: "[Incident] WARN alert".to_string(),
        body: "Alert body warn".to_string(),
        labels: Some(vec!["runtime".to_string(), "incident".to_string()]),
        severity: "warn".to_string(),
        fingerprint: Some("alert:mem:warn".to_string()),
        backend: Some("maloca".to_string()),
    };

    let res3 = ticket_create(args3).expect("create fresh warn ticket");
    assert!(
        !res3.deduplicated,
        "third ticket should be fresh (different fingerprint)"
    );
}
