use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use xavier::mesh::p2p::fallback::{OfflineBuffer, SyncEvent, SyncStatus};

#[tokio::test]
async fn test_in_memory_offline_buffer_lifecycle() {
    let buffer = OfflineBuffer::in_memory().expect("failed to create in-memory buffer");

    assert!(buffer.is_connected());
    assert_eq!(buffer.pending_count().unwrap(), 0);
    assert_eq!(buffer.total_count().unwrap(), 0);

    // Enqueue an event while online
    let event1 = SyncEvent::new("maloca.proposal.created", b"proposal_data_1".to_vec());
    buffer.enqueue(event1.clone()).unwrap();

    assert_eq!(buffer.pending_count().unwrap(), 1);
    assert_eq!(buffer.total_count().unwrap(), 1);

    // Disconnect network
    let reconnected = buffer.set_connected(false);
    assert!(!reconnected);
    assert!(!buffer.is_connected());

    // Enqueue another event while offline
    let event2 = SyncEvent::new("maloca.vote.submitted", b"vote_data_2".to_vec());
    buffer.enqueue(event2.clone()).unwrap();

    assert_eq!(buffer.pending_count().unwrap(), 2);
    assert_eq!(buffer.total_count().unwrap(), 2);

    let pending = buffer.get_pending().unwrap();
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].event_type, "maloca.proposal.created");
    assert_eq!(pending[1].event_type, "maloca.vote.submitted");
}

#[tokio::test]
async fn test_sync_or_buffer_behavior() {
    let buffer = OfflineBuffer::in_memory().expect("failed to create buffer");

    // Connected case: transport send succeeds
    let event = SyncEvent::new("maloca.decision.finalized", b"decision_data".to_vec());
    let sent = buffer
        .sync_or_buffer(event.clone(), |_evt| async { Ok(()) })
        .await
        .unwrap();

    assert!(sent);
    assert_eq!(buffer.pending_count().unwrap(), 0);

    // Connected case: transport send fails -> transparently buffered
    let sent_fail = buffer
        .sync_or_buffer(event.clone(), |_evt| async {
            Err(anyhow::anyhow!("Network timeout"))
        })
        .await
        .unwrap();

    assert!(!sent_fail);
    assert_eq!(buffer.pending_count().unwrap(), 1);

    let pending = buffer.get_pending().unwrap();
    assert_eq!(pending[0].retry_count, 1);
    assert_eq!(pending[0].status, SyncStatus::Failed);
    assert_eq!(
        pending[0].last_error.as_deref(),
        Some("Network timeout")
    );

    // Disconnected case: directly buffered without calling transport
    buffer.set_connected(false);
    let event_offline = SyncEvent::new("maloca.belief.synced", b"belief_data".to_vec());
    let sent_offline = buffer
        .sync_or_buffer(event_offline, |_evt| async { Ok(()) })
        .await
        .unwrap();

    assert!(!sent_offline);
    assert_eq!(buffer.pending_count().unwrap(), 2);
}

#[tokio::test]
async fn test_reconnect_automatic_replay() {
    let buffer = OfflineBuffer::in_memory().expect("failed to create buffer");

    // Buffer 3 events while offline
    buffer.set_connected(false);
    buffer.enqueue_event("event_1", b"data1".to_vec()).unwrap();
    buffer.enqueue_event("event_2", b"data2".to_vec()).unwrap();
    buffer.enqueue_event("event_3", b"data3".to_vec()).unwrap();

    assert_eq!(buffer.pending_count().unwrap(), 3);

    // Simulate reconnection signal
    let reconnected = buffer.set_connected(true);
    assert!(reconnected);

    let replayed_events = Arc::new(Mutex::new(Vec::new()));
    let replayed_clone = replayed_events.clone();

    // Replay pending events upon reconnection
    let replay_result = buffer
        .replay_pending(|evt| {
            let replayed = replayed_clone.clone();
            async move {
                replayed.lock().await.push(evt.event_type.clone());
                Ok(())
            }
        })
        .await
        .unwrap();

    assert_eq!(replay_result.total_processed, 3);
    assert_eq!(replay_result.succeeded, 3);
    assert_eq!(replay_result.failed, 0);
    assert_eq!(replay_result.remaining_pending, 0);
    assert_eq!(buffer.pending_count().unwrap(), 0);

    let processed = replayed_events.lock().await;
    assert_eq!(*processed, vec!["event_1", "event_2", "event_3"]);
}

#[tokio::test]
async fn test_retry_limits_and_failed_events() {
    let buffer = OfflineBuffer::in_memory()
        .unwrap()
        .with_max_retries(3);

    let event = SyncEvent::new("failing_event", b"bad_payload".to_vec());
    buffer.enqueue(event.clone()).unwrap();

    let attempts = Arc::new(AtomicUsize::new(0));

    // Replay 1st attempt - fails
    let attempts_clone = attempts.clone();
    let res1 = buffer
        .replay_pending(|_| {
            attempts_clone.fetch_add(1, Ordering::SeqCst);
            async { Err(anyhow::anyhow!("Transient error 1")) }
        })
        .await
        .unwrap();

    assert_eq!(res1.failed, 1);
    assert_eq!(buffer.pending_count().unwrap(), 1);

    // Replay 2nd attempt - fails
    let attempts_clone = attempts.clone();
    let res2 = buffer
        .replay_pending(|_| {
            attempts_clone.fetch_add(1, Ordering::SeqCst);
            async { Err(anyhow::anyhow!("Transient error 2")) }
        })
        .await
        .unwrap();

    assert_eq!(res2.failed, 1);
    assert_eq!(buffer.pending_count().unwrap(), 1);

    // Replay 3rd attempt - fails (hits max_retries = 3)
    let attempts_clone = attempts.clone();
    let res3 = buffer
        .replay_pending(|_| {
            attempts_clone.fetch_add(1, Ordering::SeqCst);
            async { Err(anyhow::anyhow!("Transient error 3")) }
        })
        .await
        .unwrap();

    assert_eq!(res3.failed, 1);
    assert_eq!(attempts.load(Ordering::SeqCst), 3);

    // Event now exceeds max_retries, so pending_count excludes it
    assert_eq!(buffer.pending_count().unwrap(), 0);
    assert_eq!(buffer.total_count().unwrap(), 1);
}

#[tokio::test]
async fn test_sqlite_persistence_across_reopen() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("offline_buffer.db");

    {
        let buffer = OfflineBuffer::new(&db_path).unwrap();
        buffer.set_connected(false);

        buffer.enqueue_event("persisted_event_1", b"p1".to_vec()).unwrap();
        buffer.enqueue_event("persisted_event_2", b"p2".to_vec()).unwrap();

        assert_eq!(buffer.pending_count().unwrap(), 2);
    } // Drop buffer connection

    // Reopen buffer from disk
    let reopened_buffer = OfflineBuffer::new(&db_path).unwrap();
    assert_eq!(reopened_buffer.pending_count().unwrap(), 2);

    let pending = reopened_buffer.get_pending().unwrap();
    assert_eq!(pending[0].event_type, "persisted_event_1");
    assert_eq!(pending[1].event_type, "persisted_event_2");

    // Test clear_all
    let cleared = reopened_buffer.clear_all().unwrap();
    assert_eq!(cleared, 2);
    assert_eq!(reopened_buffer.pending_count().unwrap(), 0);
}

#[test]
fn test_sync_status_conversions() {
    assert_eq!(SyncStatus::Pending.as_str(), "pending");
    assert_eq!(SyncStatus::InFlight.as_str(), "in_flight");
    assert_eq!(SyncStatus::Failed.as_str(), "failed");
    assert_eq!(SyncStatus::Completed.as_str(), "completed");

    assert_eq!(SyncStatus::parse("pending"), SyncStatus::Pending);
    assert_eq!(SyncStatus::parse("in_flight"), SyncStatus::InFlight);
    assert_eq!(SyncStatus::parse("failed"), SyncStatus::Failed);
    assert_eq!(SyncStatus::parse("completed"), SyncStatus::Completed);
    assert_eq!(SyncStatus::parse("unknown_str"), SyncStatus::Pending);
}
