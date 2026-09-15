use axum::response::sse::Event;
use axum::response::IntoResponse;
use futures_util::StreamExt;
use std::time::Duration;
use tokio::time::sleep;
use xavier::notifications::{
    create_notification_sse_stream, create_notification_sse_stream_with_keepalive, IslandId,
    NOTIFICATIONS,
};

async fn event_to_string(evt: Event) -> String {
    use axum::response::sse::Sse;
    use futures_util::stream;
    use std::convert::Infallible;

    let sse_stream = Sse::new(stream::once(async move { Ok::<Event, Infallible>(evt) }));
    let resp = sse_stream.into_response();
    let body_bytes = axum::body::to_bytes(resp.into_body(), 65536)
        .await
        .expect("Failed to read SSE event body bytes");
    String::from_utf8(body_bytes.to_vec()).expect("Valid UTF-8")
}

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
    assert!(
        received.is_ok(),
        "Timed out waiting for notification broadcast"
    );
    let received_notif = received.unwrap().expect("Failed to receive from broadcast");
    assert_eq!(received_notif.title, "SSE Stream Push");
    assert_eq!(received_notif.island_id.as_str(), "system");

    let _ = send_handle.await;
}

#[tokio::test]
async fn test_sse_reconnect_handshake_and_keepalive() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    // Create stream with short keepalive duration for fast test execution (50ms)
    let keepalive_dur = Duration::from_millis(50);
    let mut stream = create_notification_sse_stream_with_keepalive(keepalive_dur);

    // 1. First event should be connection handshake with retry: 3000ms
    let handshake_evt = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .expect("Timeout waiting for handshake event")
        .expect("Stream closed unexpectedly")
        .expect("Event error");

    let handshake_str = event_to_string(handshake_evt).await;
    assert!(
        handshake_str.contains("retry:3000") || handshake_str.contains("retry: 3000"),
        "Handshake should set retry to 3000ms: {handshake_str}"
    );

    // 2. Publish a notification and verify notification event fields (ID for reconnect, retry: 3000)
    let send_handle = tokio::spawn(async move {
        sleep(Duration::from_millis(20)).await;
        let _ = NOTIFICATIONS
            .notify(
                IslandId::Agents,
                "Resilient Task Update",
                "Testing SSE reconnect and event ID tracking",
                "info",
            )
            .await;
    });

    let _ = send_handle.await;

    // Receive subsequent events (notification and keepalive)
    let mut received_notification = false;
    let mut received_keepalive = false;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while !received_notification || !received_keepalive {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let evt = tokio::time::timeout(remaining, stream.next())
            .await
            .expect("Timeout waiting for stream event")
            .expect("Stream closed unexpectedly")
            .expect("Event error");

        let evt_str = event_to_string(evt).await;
        if evt_str.contains("event:notification") || evt_str.contains("event: notification") {
            assert!(
                evt_str.contains("id:"),
                "Should contain notification id field for last-event-id tracking: {evt_str}"
            );
            assert!(
                evt_str.contains("Resilient Task Update"),
                "Should contain notification payload data: {evt_str}"
            );
            assert!(
                evt_str.contains("retry:3000") || evt_str.contains("retry: 3000"),
                "Notification event should contain retry: 3000ms: {evt_str}"
            );
            received_notification = true;
        } else if evt_str.contains(":keepalive") || evt_str.contains(": keepalive") {
            received_keepalive = true;
        }
    }

    assert!(received_notification, "Should have received notification event");
    assert!(received_keepalive, "Should have received keepalive comment ping");
}

#[tokio::test]
async fn test_default_sse_stream_creation() {
    let mut stream = create_notification_sse_stream();
    let handshake_evt = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .expect("Timeout waiting for handshake event")
        .expect("Stream closed unexpectedly")
        .expect("Event error");

    let handshake_str = event_to_string(handshake_evt).await;
    assert!(
        handshake_str.contains("retry:3000") || handshake_str.contains("retry: 3000"),
        "Default SSE stream handshake should set retry to 3000ms: {handshake_str}"
    );
}
