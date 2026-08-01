use serde_json::Value;
use tokio::net::TcpListener;
use xavier::codebase::connection_manager::ConnectionManager;
use xavier::notifications::{IslandId, NOTIFICATIONS, SENT_EMAILS};

async fn setup_test() {
    std::env::set_var("XAVIER_TEST", "true");
    let _ = ConnectionManager::global().connect("memory", ".");
    let _ = ConnectionManager::global()
        .with_conn("memory", |conn| {
            conn.execute(
                "CREATE TABLE IF NOT EXISTS notifications (
                id TEXT PRIMARY KEY,
                island_id TEXT NOT NULL,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                read INTEGER NOT NULL DEFAULT 0,
                severity TEXT NOT NULL
            )",
                [],
            )?;
            Ok(())
        })
        .await;
}

#[tokio::test]
async fn test_notification_delivery_and_persistence() {
    setup_test().await;
    // Clean old state
    let _ = NOTIFICATIONS.delete_all().await;

    // Trigger a notification
    let notification = NOTIFICATIONS
        .notify(
            IslandId::System,
            "Integration Test Title",
            "Integration Test Body",
            "info",
        )
        .await
        .expect("send notification");

    assert_eq!(notification.title, "Integration Test Title");
    assert_eq!(notification.body, "Integration Test Body");
    assert_eq!(notification.severity, "info");

    // Check persistence
    let list = NOTIFICATIONS
        .list_notifications()
        .await
        .expect("list notifications");
    assert!(!list.is_empty());
    let persisted = list
        .iter()
        .find(|n| n.id == notification.id)
        .expect("find persisted");
    assert_eq!(persisted.title, "Integration Test Title");
    assert_eq!(persisted.body, "Integration Test Body");
    assert_eq!(persisted.severity, "info");
    assert_eq!(persisted.read, false);

    // Mark as read
    NOTIFICATIONS
        .mark_as_read(&notification.id)
        .await
        .expect("mark as read");
    let list_after = NOTIFICATIONS
        .list_notifications()
        .await
        .expect("list notifications after");
    let persisted_after = list_after
        .iter()
        .find(|n| n.id == notification.id)
        .expect("find persisted after");
    assert_eq!(persisted_after.read, true);
}

#[tokio::test]
async fn test_email_notification_delivery() {
    setup_test().await;
    // Clear sent emails
    {
        let mut emails = SENT_EMAILS.lock().await;
        emails.clear();
    }

    // Trigger a notification
    let _ = NOTIFICATIONS
        .notify(
            IslandId::Agents,
            "Email Test Title",
            "Email Test Body",
            "success",
        )
        .await
        .expect("send notification");

    // Check email delivery
    let emails = SENT_EMAILS.lock().await;
    assert!(!emails.is_empty());
    let sent_email = emails
        .iter()
        .find(|e| e.title == "Email Test Title")
        .expect("find sent email");
    assert_eq!(sent_email.body, "Email Test Body");
}

#[tokio::test]
async fn test_webhook_subscription_management_and_delivery() {
    setup_test().await;
    // Clear subscriptions
    let subs = NOTIFICATIONS
        .list_subscriptions()
        .await
        .expect("list subscriptions");
    for sub in subs {
        let _ = NOTIFICATIONS.remove_subscription(&sub.id).await;
    }

    // Start a mock HTTP server to receive the webhook
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind random test port");
    let addr = listener.local_addr().expect("read local address");
    let mock_webhook_url = format!("http://{addr}/webhook-target");

    let received_webhook_data = std::sync::Arc::new(tokio::sync::Mutex::new(None));
    let received_webhook_data_clone = received_webhook_data.clone();

    let axum_app = axum::Router::new().route(
        "/webhook-target",
        axum::routing::post(move |axum::Json(payload): axum::Json<Value>| {
            let data = received_webhook_data_clone.clone();
            async move {
                let mut guard = data.lock().await;
                *guard = Some(payload);
                axum::http::StatusCode::OK
            }
        }),
    );

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, axum_app)
            .await
            .expect("serve mock webhook receiver");
    });

    // Add subscription
    let subscription = NOTIFICATIONS
        .add_subscription(&mock_webhook_url, vec!["errors".to_string()])
        .await
        .expect("add subscription");

    assert_eq!(subscription.url, mock_webhook_url);
    assert_eq!(subscription.event_types, vec!["errors".to_string()]);
    assert_eq!(subscription.active, true);

    // Get list of subscriptions
    let list = NOTIFICATIONS
        .list_subscriptions()
        .await
        .expect("list subscriptions after add");
    assert!(list.iter().any(|s| s.id == subscription.id));

    // Trigger a non-matching notification (System)
    let _ = NOTIFICATIONS
        .notify(
            IslandId::System,
            "Should not deliver",
            "Because event type doesn't match",
            "info",
        )
        .await
        .expect("notify system");

    // Wait a short time and assert no webhook received
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
    {
        let guard = received_webhook_data.lock().await;
        assert!(guard.is_none());
    }

    // Trigger a matching notification (Errors)
    let _ = NOTIFICATIONS
        .notify(
            IslandId::Errors,
            "Critical Webhook Test",
            "Webhook matches errors",
            "error",
        )
        .await
        .expect("notify errors");

    // Wait and assert webhook received
    let mut received = None;
    for _ in 0..20 {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        let guard = received_webhook_data.lock().await;
        if guard.is_some() {
            received = guard.clone();
            break;
        }
    }

    let payload = received.expect("webhook payload should be received");
    assert_eq!(payload["title"], "Critical Webhook Test");
    assert_eq!(payload["body"], "Webhook matches errors");

    // Remove subscription
    NOTIFICATIONS
        .remove_subscription(&subscription.id)
        .await
        .expect("remove subscription");
    let list_after = NOTIFICATIONS
        .list_subscriptions()
        .await
        .expect("list subscriptions after remove");
    assert!(!list_after.iter().any(|s| s.id == subscription.id));

    server_handle.abort();
}
