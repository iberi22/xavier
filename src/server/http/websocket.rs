//! WebSocket server for real-time communication
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::server::events::{WsEvent, WsMessage};
use crate::AppState;
use axum::{
    extract::{ws::Message, ws::WebSocket, State, WebSocketUpgrade},
    response::IntoResponse,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

#[derive(Clone)]
pub struct ShutdownState {
    pub shutdown_signalled: Arc<AtomicU64>,
    shutdown_tx: Arc<broadcast::Sender<()>>,
}
impl ShutdownState {
    pub fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            shutdown_signalled: Arc::new(AtomicU64::new(0)),
            shutdown_tx: Arc::new(shutdown_tx),
        }
    }
    pub fn request_shutdown(&self, reason: &'static str) {
        let prev = self.shutdown_signalled.fetch_add(1, Ordering::SeqCst);
        if prev == 0 {
            info!("Shutdown requested: {}", reason);
            let _ = self.shutdown_tx.send(());
        }
    }
    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown_signalled.load(Ordering::SeqCst) > 0
    }
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }
    pub fn seconds_since_shutdown(&self) -> u64 {
        let val = self.shutdown_signalled.load(Ordering::SeqCst);
        if val == 0 {
            0
        } else {
            val
        }
    }
}

#[derive(Debug, Default, Clone)]
struct WsSubscriptions {
    agent_ids: std::collections::HashSet<String>,
    project_ids: std::collections::HashSet<String>,
    event_types: std::collections::HashSet<String>,
}
impl WsSubscriptions {
    fn matches(&self, event: &crate::server::events::RealtimeEvent) -> bool {
        if self.agent_ids.is_empty() && self.project_ids.is_empty() && self.event_types.is_empty() {
            return false;
        }
        if !self.agent_ids.is_empty() && !self.agent_ids.contains(&event.agent_id) {
            return false;
        }
        if !self.project_ids.is_empty() {
            match &event.project_id {
                Some(p) if self.project_ids.contains(p) => {}
                _ => return false,
            }
        }
        if !self.event_types.is_empty() && !self.event_types.contains(&event.event_type) {
            return false;
        }
        true
    }
}

pub async fn ws_events_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let rx = state
        .workspace_registry
        .default_context_sync()
        .and_then(|ctx| ctx.workspace.event_tx_channel().map(|tx| tx.subscribe()));
    ws.on_upgrade(move |socket| handle_ws_socket(socket, rx))
}

async fn handle_ws_socket(
    mut socket: WebSocket,
    mut event_rx: Option<broadcast::Receiver<crate::server::events::RealtimeEvent>>,
) {
    let mut subscriptions = WsSubscriptions::default();
    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                            match ws_msg {
                                WsMessage::Subscribe { agent_id, project_id, event_type } => {
                                    if let Some(id) = agent_id { subscriptions.agent_ids.insert(id); }
                                    if let Some(id) = project_id { subscriptions.project_ids.insert(id); }
                                    if let Some(id) = event_type { subscriptions.event_types.insert(id); }
                                    let _ = socket.send(Message::Text(serde_json::to_string(&WsEvent::SubscriptionConfirmed).unwrap_or_default().into())).await;
                                }
                                WsMessage::Unsubscribe { agent_id, project_id, event_type } => {
                                    if let Some(id) = agent_id { subscriptions.agent_ids.remove(&id); }
                                    if let Some(id) = project_id { subscriptions.project_ids.remove(&id); }
                                    if let Some(id) = event_type { subscriptions.event_types.remove(&id); }
                                    let _ = socket.send(Message::Text(serde_json::to_string(&WsEvent::SubscriptionConfirmed).unwrap_or_default().into())).await;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            event_res = async { if let Some(rx) = &mut event_rx { rx.recv().await.ok() } else { None } } => {
                if let Some(event) = event_res {
                    if subscriptions.matches(&event) {
                        let _ = socket.send(Message::Text(serde_json::to_string(&WsEvent::Event(event)).unwrap_or_default().into())).await;
                    }
                }
            }
        }
    }
}
impl Default for ShutdownState {
    fn default() -> Self {
        Self::new()
    }
}
