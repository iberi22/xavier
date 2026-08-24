use crate::notifications::{Notification, NOTIFICATIONS};
use tokio::sync::mpsc;
use tracing::info;

pub struct NotificationDispatcher {
    telegram_tx: Option<mpsc::Sender<Notification>>,
}

impl NotificationDispatcher {
    /// New.
    pub fn new() -> Self {
        Self {
            telegram_tx: None,
        }
    }

    /// With telegram.
    pub fn with_telegram(mut self, tx: mpsc::Sender<Notification>) -> Self {
        self.telegram_tx = Some(tx);
        self
    }

    /// Start.
    pub fn start(self) {
        let mut rx = NOTIFICATIONS.subscribe();
        
        tokio::spawn(async move {
            info!("Notification Dispatcher started in background.");
            if let Err(e) = crate::notifications::ensure_memory_pool().await {
                tracing::warn!("Notification Dispatcher: memory pool lazy init warning: {}", e);
            }
            while let Ok(notification) = rx.recv().await {
                if let Some(tx) = &self.telegram_tx {
                    if let Err(e) = tx.send(notification).await {
                        tracing::warn!("Notification Dispatcher: failed to send to telegram channel: {}", e);
                    }
                }
            }
        });
    }
}

impl Default for NotificationDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::IslandId;
    use tokio::sync::mpsc;
    use tokio::time::{timeout, Duration};

    #[test]
    fn test_dispatcher_new_and_default() {
        let dispatcher1 = NotificationDispatcher::new();
        assert!(dispatcher1.telegram_tx.is_none());

        let dispatcher2 = NotificationDispatcher::default();
        assert!(dispatcher2.telegram_tx.is_none());
    }

    #[test]
    fn test_dispatcher_with_telegram() {
        let (tx, _rx) = mpsc::channel(10);
        let dispatcher = NotificationDispatcher::new().with_telegram(tx);
        assert!(dispatcher.telegram_tx.is_some());
    }

    #[tokio::test]
    async fn test_dispatcher_routing() {
        let (tx, mut rx) = mpsc::channel(10);
        let dispatcher = NotificationDispatcher::new().with_telegram(tx);
        dispatcher.start();

        let _ = NOTIFICATIONS.notify(IslandId::System, "Dispatcher Test Title", "Dispatcher Test Body", "info").await;

        let received = timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("timeout waiting for dispatcher routing")
            .expect("channel closed");

        assert_eq!(received.title, "Dispatcher Test Title");
        assert_eq!(received.body, "Dispatcher Test Body");
    }
}
