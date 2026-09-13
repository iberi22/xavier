use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    response::{sse::Event, Response, Sse},
    routing::get,
    Router,
};
use futures_util::stream::{self, Stream};
use http_body_util::BodyExt;
use std::convert::Infallible;
use tokio::sync::broadcast::error::RecvError;
use tower::util::ServiceExt;

use xavier::notifications::{IslandId, NOTIFICATIONS};

#[derive(Clone, Default)]
struct TestState;

async fn stream_notifications_handler(
    State(_state): State<TestState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = NOTIFICATIONS.subscribe();
    let stream = stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(notification) => {
                    if let Ok(event) = Event::default().json_data(&notification) {
                        return Some((Ok(event), rx));
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!("Notification stream lagged by {} messages", skipped);
                    continue;
                }
                Err(RecvError::Closed) => {
                    tracing::info!("Notification stream broadcast channel closed");
                    return None;
                }
            }
        }
    });

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

fn build_test_router() -> Router {
    Router::new()
        .route("/notifications/stream", get(stream_notifications_handler))
        .with_state(TestState)
}

#[tokio::test]
async fn test_notifications_stream_endpoint() {
    let app = build_test_router();

    let req = Request::builder()
        .uri("/notifications/stream")
        .header("Accept", "text/event-stream")
        .body(Body::empty())
        .expect("failed to build request");

    let response: Response = app.oneshot(req).await.expect("failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "Expected content-type to contain text/event-stream, got {}",
        content_type
    );

    let mut body = response.into_body();

    let notification_title = format!("SSE Test Notification {}", uuid::Uuid::new_v4());
    let dispatched = NOTIFICATIONS
        .notify(
            IslandId::System,
            &notification_title,
            "Testing realtime SSE delivery",
            "info",
        )
        .await
        .expect("failed to dispatch notification");

    let mut received_event_text = String::new();
    let timeout_duration = std::time::Duration::from_secs(3);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout_duration {
        if let Ok(Some(Ok(frame))) =
            tokio::time::timeout(std::time::Duration::from_millis(500), body.frame()).await
        {
            if let Some(data) = frame.data_ref() {
                let text = String::from_utf8_lossy(data);
                received_event_text.push_str(&text);
                if received_event_text.contains(&dispatched.id) {
                    break;
                }
            }
        }
    }

    assert!(
        received_event_text.contains(&dispatched.id)
            || received_event_text.contains(&notification_title),
        "Expected SSE stream data to contain notification ID/title '{}', but got: {}",
        notification_title,
        received_event_text
    );
}
