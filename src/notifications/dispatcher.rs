// SPDX-License-Identifier: MIT OR LICENSE-MESH
use crate::notifications::{Notification, NOTIFICATIONS};
use tokio::sync::mpsc;
use tracing::info;

pub struct NotificationDispatcher {
    telegram_tx: Option<mpsc::Sender<Notification>>,
}

impl NotificationDispatcher {
    pub fn new() -> Self {
        Self {
            telegram_tx: None,
        }
    }

    pub fn with_telegram(mut self, tx: mpsc::Sender<Notification>) -> Self {
        self.telegram_tx = Some(tx);
        self
    }

    pub fn start(self) {
        let mut rx = NOTIFICATIONS.subscribe();
        
        tokio::spawn(async move {
            info!("Notification Dispatcher started in background.");
            while let Ok(notification) = rx.recv().await {
                if let Some(tx) = &self.telegram_tx {
                    let _ = tx.send(notification).await;
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
