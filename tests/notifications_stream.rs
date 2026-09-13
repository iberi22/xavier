use std::time::Duration;
use tokio::time::sleep;
use xavier::notifications::{IslandId, NOTIFICATIONS};

#[tokio::test]
async fn test_notifications_stream_endpoint() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    // Verify NOTIFICATIONS subscription works
    let mut rx = NOTIFICATIONS.subscribe();

    // Spawn a task sending a notification
    let send_handle = tokio::spawn(async move {
        sleep(Duration::from_millis(50)).await;
        let _ = NOTIFICATIONS
            .notify(
                IslandId::System,
                "SSE Stream Push",
                "Verified live event delivery via SSE broadcast",
                "info",
            )
            .await;
    });

    // Receive the broadcasted notification
    let received = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;
    assert!(received.is_ok(), "Timed out waiting for notification broadcast");
    let received_notif = received.unwrap().expect("Failed to receive from broadcast");
    assert_eq!(received_notif.title, "SSE Stream Push");
    assert_eq!(received_notif.island_id.as_str(), "system");

    let _ = send_handle.await;
}
