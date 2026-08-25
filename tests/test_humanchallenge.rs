use chrono::Utc;
use std::time::Duration;
use xavier::humanchallenge::{
    ChallengeStatus, ChallengeType, HumanChallengeCron, HumanChallengeCronConfig,
    HumanChallengeEvent, HumanChallengeStore, SessionScanner,
};
use xavier::session::types::{SessionEvent, SessionEventType};

#[test]
fn test_humanchallenge_store_crud() {
    let store = HumanChallengeStore::in_memory().expect("in-memory store init");

    let event = HumanChallengeEvent {
        id: "hc-test-01".to_string(),
        session_id: "session-abc".to_string(),
        challenge_type: ChallengeType::Contradiction,
        description: "Contradictory decision detected".to_string(),
        raw_content: "Decision A contradicts Decision B".to_string(),
        confidence_score: 0.95,
        status: ChallengeStatus::Candidate,
        created_at: Utc::now(),
        answered_at: None,
        response: None,
        points_awarded: 0,
        privacy_p4_local_only: true,
    };

    store.save_event(&event).expect("save event");

    let retrieved = store
        .get_event_by_id("hc-test-01")
        .expect("get event query")
        .expect("event must exist");
    assert_eq!(retrieved.id, "hc-test-01");
    assert_eq!(retrieved.challenge_type, ChallengeType::Contradiction);
    assert_eq!(retrieved.status, ChallengeStatus::Candidate);

    // Answer challenge
    let answered_ok = store
        .answer_challenge("hc-test-01", "Clarified: choosing A", 10)
        .expect("answer event");
    assert!(answered_ok);

    let answered = store
        .get_event_by_id("hc-test-01")
        .expect("get event query")
        .expect("event must exist");
    assert_eq!(answered.status, ChallengeStatus::Answered);
    assert_eq!(answered.points_awarded, 10);
    assert!(answered.answered_at.is_some());
}

#[test]
fn test_humanchallenge_farming_summary() {
    let store = HumanChallengeStore::in_memory().expect("in-memory store init");

    let types = [
        ChallengeType::Contradiction,
        ChallengeType::Decision,
        ChallengeType::Execution,
        ChallengeType::Assumption,
        ChallengeType::Clarification,
    ];

    for (i, c_type) in types.iter().enumerate() {
        let event = HumanChallengeEvent {
            id: format!("hc-event-{}", i),
            session_id: "session-123".to_string(),
            challenge_type: *c_type,
            description: format!("Test event for {:?}", c_type),
            raw_content: "raw text".to_string(),
            confidence_score: 0.88,
            status: ChallengeStatus::Candidate,
            created_at: Utc::now(),
            answered_at: None,
            response: None,
            points_awarded: 0,
            privacy_p4_local_only: true,
        };
        store.save_event(&event).expect("save event");
    }

    let ym = Utc::now().format("%Y-%m").to_string();
    let summary = store.get_farming_summary(&ym).expect("get farming summary");
    assert_eq!(summary.year_month, ym);
    assert_eq!(summary.total_points, 0);

    // Answer 2 events
    store.answer_challenge("hc-event-0", "Answer 0", 10).expect("answer");
    store.answer_challenge("hc-event-1", "Answer 1", 10).expect("answer");

    let summary2 = store.get_farming_summary(&ym).expect("get farming summary");
    assert_eq!(summary2.total_points, 20);
    assert_eq!(summary2.answered_count, 2);
}

#[test]
fn test_humanchallenge_scanner_heuristics() {
    let scanner = SessionScanner::new();
    let events = vec![
        SessionEvent {
            session_id: "sess-001".to_string(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            content: Some("Decidí que la base de datos debe ser PostgreSQL, sin embargo en el archivo anterior declaré SQLite.".to_string()),
            metadata: None,
        }
    ];

    let detected = scanner.scan_session_events(&events);
    assert!(!detected.is_empty());
    assert_eq!(detected[0].challenge_type, ChallengeType::Contradiction);
}

#[test]
fn test_humanchallenge_cron_process_and_award() {
    let store = HumanChallengeStore::in_memory().expect("in-memory store");
    let cron = HumanChallengeCron::with_store(
        HumanChallengeCronConfig {
            db_path: std::path::PathBuf::from(":memory:"),
            scan_interval: Duration::from_secs(60),
            enabled: true,
        },
        store,
    );

    let events = vec![
        SessionEvent {
            session_id: "sess-cron-01".to_string(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            content: Some("He decidido migrar a NixOS pero sin embargo mantengo Ubuntu.".to_string()),
            metadata: None,
        }
    ];

    let count = cron.process_events(&events).expect("process events");
    assert_eq!(count, 1);
}
