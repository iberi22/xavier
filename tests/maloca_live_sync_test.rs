use serde_json::json;
use xavier::server::maloca::live_sync::{
    BroadcasterStats, MalocaEvent, MalocaEventBroadcaster, MalocaEventFilter,
    MalocaSubscriberError, WsClientMessage, WsServerMessage,
};

#[tokio::test]
async fn test_multi_subscriber_fan_out() {
    let broadcaster = MalocaEventBroadcaster::new(64);

    let mut sub1 = broadcaster.subscribe();
    let mut sub2 = broadcaster.subscribe();
    let mut sub3 = broadcaster.subscribe();

    assert_eq!(broadcaster.subscriber_count(), 3);

    let event = MalocaEvent::proposal(
        Some("project-alpha"),
        json!({"title": "Upgrade Quorum", "body": "Increase threshold"}),
    );

    let published_count = broadcaster.publish(event.clone()).unwrap();
    assert_eq!(published_count, 3);

    let rec1 = sub1.recv().await.unwrap();
    let rec2 = sub2.recv().await.unwrap();
    let rec3 = sub3.recv().await.unwrap();

    assert_eq!(rec1, event);
    assert_eq!(rec2, event);
    assert_eq!(rec3, event);
}

#[tokio::test]
async fn test_event_type_and_project_filtering() {
    let broadcaster = MalocaEventBroadcaster::new(64);

    // Subscriber 1: Wildcard (no filter)
    let mut sub_all = broadcaster.subscribe();

    // Subscriber 2: Filter by proposals
    let mut sub_proposals = broadcaster.subscribe();
    sub_proposals.add_filter(MalocaEventFilter::new(None, Some("proposals".to_string())));

    // Subscriber 3: Filter by project-beta and decisions
    let mut sub_beta_decisions = broadcaster.subscribe();
    sub_beta_decisions.add_filter(MalocaEventFilter::new(
        Some("project-beta".to_string()),
        Some("decisions".to_string()),
    ));

    // Event 1: Proposal for project-alpha
    let evt_p1 = MalocaEvent::proposal(Some("project-alpha"), json!({"id": "prop-1"}));
    broadcaster.publish(evt_p1.clone()).unwrap();

    // Event 2: Decision for project-beta
    let evt_d1 = MalocaEvent::decision(Some("project-beta"), json!({"id": "dec-1"}));
    broadcaster.publish(evt_d1.clone()).unwrap();

    // Event 3: Belief update for project-beta
    let evt_b1 = MalocaEvent::belief(Some("project-beta"), json!({"id": "bel-1"}));
    broadcaster.publish(evt_b1.clone()).unwrap();

    // Event 4: Vote for project-alpha
    let evt_v1 = MalocaEvent::vote(Some("project-alpha"), json!({"choice": "yes"}));
    broadcaster.publish(evt_v1.clone()).unwrap();

    // sub_all receives all 4 events
    assert_eq!(sub_all.recv().await.unwrap(), evt_p1);
    assert_eq!(sub_all.recv().await.unwrap(), evt_d1);
    assert_eq!(sub_all.recv().await.unwrap(), evt_b1);
    assert_eq!(sub_all.recv().await.unwrap(), evt_v1);

    // sub_proposals receives only evt_p1
    assert_eq!(sub_proposals.recv().await.unwrap(), evt_p1);

    // sub_beta_decisions receives only evt_d1
    assert_eq!(sub_beta_decisions.recv().await.unwrap(), evt_d1);
}

#[tokio::test]
async fn test_slow_client_drop_policy() {
    // Broadcaster with minimum buffer capacity 16
    let broadcaster = MalocaEventBroadcaster::new(16);

    let mut fast_sub = broadcaster.subscribe();
    let mut slow_sub = broadcaster.subscribe();

    // Publish 30 events into buffer of capacity 16 while fast_sub consumes immediately
    for i in 0..30 {
        let evt = MalocaEvent::proposal(Some("project-alpha"), json!({ "sequence": i }));
        broadcaster.publish(evt).unwrap();

        // Fast subscriber reads each event immediately without lag
        let ev = fast_sub.recv().await.unwrap();
        assert_eq!(ev.payload["sequence"], i);
    }

    // Slow subscriber tries to read and receives Lagged error indicating dropped messages
    let err = slow_sub.recv().await.unwrap_err();
    if let MalocaSubscriberError::Lagged(skipped) = err {
        assert!(skipped > 0);
    } else {
        panic!("Expected Lagged error for slow subscriber, got {:?}", err);
    }

    // Broadcaster stats track dropped messages accurately
    let stats: BroadcasterStats = broadcaster.stats();
    assert_eq!(stats.total_published, 30);
    assert!(stats.total_dropped > 0);
}

#[tokio::test]
async fn test_ws_message_serde() {
    // Client message serde
    let client_sub = WsClientMessage::Subscribe {
        project_id: Some("proj-1".to_string()),
        event_type: Some("proposals".to_string()),
    };
    let json_str = serde_json::to_string(&client_sub).unwrap();
    assert!(json_str.contains("\"type\":\"subscribe\""));
    assert!(json_str.contains("\"project_id\":\"proj-1\""));

    let de_client: WsClientMessage = serde_json::from_str(&json_str).unwrap();
    if let WsClientMessage::Subscribe {
        project_id,
        event_type,
    } = de_client
    {
        assert_eq!(project_id, Some("proj-1".to_string()));
        assert_eq!(event_type, Some("proposals".to_string()));
    } else {
        panic!("Deserialized unexpected variant");
    }

    // Server message serde
    let evt = MalocaEvent::vote(Some("proj-2"), json!({"choice": "abstain"}));
    let server_evt = WsServerMessage::Event(evt);
    let json_srv = serde_json::to_string(&server_evt).unwrap();
    assert!(json_srv.contains("\"type\":\"event\""));

    let server_lagged = WsServerMessage::Lagged { skipped: 15 };
    let json_lag = serde_json::to_string(&server_lagged).unwrap();
    assert!(json_lag.contains("\"type\":\"lagged\""));
    assert!(json_lag.contains("\"skipped\":15"));
}
