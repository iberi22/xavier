//! Gateway plugin system for Xavier DocBot.
//!
//! Defines the `GatewayAdapter` trait and a `GatewayRegistry` that can hold
//! multiple chat-platform adapters (Telegram, WhatsApp, Discord, Slack, HTTP).
//! Each gateway receives inbound user messages, routes them through the RAG
//! pipeline, and sends back answers with citations.
//!
//! # Feature flags
//! - `telegram`  — enables `TelegramDocGateway`
//! - `whatsapp`  — enables `WhatsAppGateway` (Meta Cloud API)
//! - `discord`   — enables `DiscordGateway` (serenity)
//! - `slack`     — enables `SlackGateway` (slack-morphism)
//!
//! (all gated so the binary stays lean by default)

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

pub mod telegram_docbot;
pub mod whatsapp;

// ── Core types ──────────────────────────────────────────────────────────────────

/// An inbound message received from any gateway channel.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    /// Unique message ID within the source gateway.
    pub message_id: String,
    /// The gateway that delivered this message (e.g. "telegram", "whatsapp").
    pub gateway: String,
    /// Channel or chat identifier (chat_id, phone number, channel ID…).
    pub channel_id: String,
    /// Sender identifier within that gateway.
    pub sender_id: String,
    /// Display name of the sender, if available.
    pub sender_name: Option<String>,
    /// Text content of the message.
    pub text: String,
    /// Optional: attached file bytes (for document uploads via bot).
    pub attachment: Option<MessageAttachment>,
}

/// A file attachment from a gateway message.
#[derive(Debug, Clone)]
pub struct MessageAttachment {
    pub file_name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

/// An outbound message to send back through a gateway.
#[derive(Debug, Clone)]
pub struct OutboundMessage {
    pub channel_id: String,
    pub text: String,
    /// Markdown formatting supported by the target gateway.
    pub parse_mode: ParseMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParseMode {
    #[default]
    Plain,
    Markdown,
    Html,
}

// ── Trait ──────────────────────────────────────────────────────────────────────

/// Unified interface for all chat-platform gateways.
///
/// Implementors must be `Send + Sync` (used behind `Arc` in the registry).
#[async_trait]
pub trait GatewayAdapter: Send + Sync {
    /// Short name for the gateway (e.g. "telegram", "whatsapp").
    fn name(&self) -> &str;

    /// Start listening for inbound messages. This method runs until the gateway
    /// shuts down (long-polling, webhooks, etc.).
    ///
    /// `handler` is called for every inbound message; it returns the reply text.
    async fn start(&self, handler: Arc<dyn MessageHandler>) -> Result<()>;

    /// Send a single outbound message to a channel.
    async fn send(&self, msg: OutboundMessage) -> Result<()>;

    /// Health probe — returns `true` if the gateway is reachable.
    async fn health(&self) -> bool {
        true
    }
}

/// Callback trait for processing inbound messages.
/// Implemented by the RAG pipeline dispatcher.
#[async_trait]
pub trait MessageHandler: Send + Sync {
    async fn handle(&self, msg: InboundMessage) -> Result<String>;
}

// ── Registry ───────────────────────────────────────────────────────────────────

/// Runtime registry of all active gateways.
#[derive(Clone, Default)]
pub struct GatewayRegistry {
    adapters: Arc<RwLock<HashMap<String, Arc<dyn GatewayAdapter>>>>,
}

impl GatewayRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a gateway adapter.
    pub async fn register(&self, adapter: Arc<dyn GatewayAdapter>) {
        let name = adapter.name().to_string();
        self.adapters.write().await.insert(name.clone(), adapter);
        info!(gateway = %name, "gateway registered");
    }

    /// Broadcast a message to a specific gateway by name.
    pub async fn send(&self, gateway: &str, msg: OutboundMessage) -> Result<()> {
        let adapters = self.adapters.read().await;
        match adapters.get(gateway) {
            Some(adapter) => adapter.send(msg).await,
            None => {
                warn!(gateway = %gateway, "gateway not found in registry");
                Err(anyhow::anyhow!("gateway '{}' not registered", gateway))
            }
        }
    }

    /// Returns the names of all registered gateways.
    pub async fn registered(&self) -> Vec<String> {
        self.adapters.read().await.keys().cloned().collect()
    }

    /// Health check across all registered gateways.
    pub async fn health_all(&self) -> HashMap<String, bool> {
        let adapters = self.adapters.read().await;
        let mut results = HashMap::new();
        for (name, adapter) in adapters.iter() {
            results.insert(name.clone(), adapter.health().await);
        }
        results
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct EchoGateway {
        name: String,
        send_count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl GatewayAdapter for EchoGateway {
        fn name(&self) -> &str {
            &self.name
        }

        async fn start(&self, _handler: Arc<dyn MessageHandler>) -> Result<()> {
            Ok(())
        }

        async fn send(&self, _msg: OutboundMessage) -> Result<()> {
            self.send_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_registry_register_and_list() {
        let registry = GatewayRegistry::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let gw = Arc::new(EchoGateway {
            name: "echo".to_string(),
            send_count: counter.clone(),
        });
        registry.register(gw).await;

        let names = registry.registered().await;
        assert!(names.contains(&"echo".to_string()));
    }

    #[tokio::test]
    async fn test_registry_send() {
        let registry = GatewayRegistry::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let gw = Arc::new(EchoGateway {
            name: "echo".to_string(),
            send_count: counter.clone(),
        });
        registry.register(gw).await;

        let msg = OutboundMessage {
            channel_id: "ch-1".to_string(),
            text: "Hello!".to_string(),
            parse_mode: ParseMode::Markdown,
        };
        registry.send("echo", msg).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_registry_send_unknown_gateway() {
        let registry = GatewayRegistry::new();
        let msg = OutboundMessage {
            channel_id: "ch-1".to_string(),
            text: "Hi".to_string(),
            parse_mode: ParseMode::Plain,
        };
        let result = registry.send("nonexistent", msg).await;
        assert!(result.is_err());
    }
}
