//! Realtime WebSocket live event synchronizer for Maloca nodes.
//!
//! Exposes realtime broadcast for proposals, decisions, votes, and beliefs graph updates
//! to Maloca portals and edge nodes. Supports multi-subscriber fan-out, event filtering by
//! `project_id` and `event_type`, slow-client drop policy tracking, and WebSocket protocol handling.

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;
use tracing::{info, warn};

/// Supported event categories for Maloca governance and knowledge graph synchronization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MalocaEventType {
    Proposals,
    Votes,
    Decisions,
    Beliefs,
    Custom(String),
}

impl MalocaEventType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Proposals => "proposals",
            Self::Votes => "votes",
            Self::Decisions => "decisions",
            Self::Beliefs => "beliefs",
            Self::Custom(s) => s.as_str(),
        }
    }

    pub fn parse_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "proposals" | "proposal" => Self::Proposals,
            "votes" | "vote" => Self::Votes,
            "decisions" | "decision" => Self::Decisions,
            "beliefs" | "belief" => Self::Beliefs,
            other => Self::Custom(other.to_string()),
        }
    }
}

/// A real-time event broadcasted across Maloca nodes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MalocaEvent {
    pub event_id: String,
    pub project_id: Option<String>,
    pub event_type: String,
    pub timestamp: i64,
    pub payload: serde_json::Value,
}

impl MalocaEvent {
    pub fn new(
        event_type: impl Into<String>,
        project_id: Option<impl Into<String>>,
        payload: serde_json::Value,
    ) -> Self {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let event_id = format!("evt_{}_{}", ts, uuid::Uuid::new_v4().simple());
        Self {
            event_id,
            project_id: project_id.map(|p| p.into()),
            event_type: event_type.into(),
            timestamp: ts,
            payload,
        }
    }

    pub fn proposal(project_id: Option<impl Into<String>>, payload: serde_json::Value) -> Self {
        Self::new("proposals", project_id, payload)
    }

    pub fn vote(project_id: Option<impl Into<String>>, payload: serde_json::Value) -> Self {
        Self::new("votes", project_id, payload)
    }

    pub fn decision(project_id: Option<impl Into<String>>, payload: serde_json::Value) -> Self {
        Self::new("decisions", project_id, payload)
    }

    pub fn belief(project_id: Option<impl Into<String>>, payload: serde_json::Value) -> Self {
        Self::new("beliefs", project_id, payload)
    }
}

/// Filter criteria for subscribed WebSocket sessions.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MalocaEventFilter {
    pub project_id: Option<String>,
    pub event_type: Option<String>,
}

impl MalocaEventFilter {
    pub fn new(project_id: Option<String>, event_type: Option<String>) -> Self {
        Self { project_id, event_type }
    }

    /// Checks whether an event matches this filter.
    pub fn matches(&self, event: &MalocaEvent) -> bool {
        if let Some(ref proj) = self.project_id {
            if event.project_id.as_deref() != Some(proj.as_str()) {
                return false;
            }
        }
        if let Some(ref evt_type) = self.event_type {
            if !event.event_type.eq_ignore_ascii_case(evt_type) {
                return false;
            }
        }
        true
    }
}

/// Statistics snapshot for a broadcaster.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BroadcasterStats {
    pub capacity: usize,
    pub active_subscribers: usize,
    pub total_published: u64,
    pub total_dropped: u64,
}

/// Thread-safe broadcaster for Maloca live event synchronization.
#[derive(Clone)]
pub struct MalocaEventBroadcaster {
    sender: broadcast::Sender<MalocaEvent>,
    capacity: usize,
    total_published: Arc<AtomicU64>,
    total_dropped: Arc<AtomicU64>,
}

impl MalocaEventBroadcaster {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity.max(16));
        Self {
            sender,
            capacity: capacity.max(16),
            total_published: Arc::new(AtomicU64::new(0)),
            total_dropped: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn publish(
        &self,
        event: MalocaEvent,
    ) -> Result<usize, broadcast::error::SendError<MalocaEvent>> {
        self.total_published.fetch_add(1, Ordering::Relaxed);
        let res = self.sender.send(event)?;
        Ok(res)
    }

    pub fn subscribe(&self) -> MalocaSubscriber {
        MalocaSubscriber {
            receiver: self.sender.subscribe(),
            filters: Vec::new(),
            skipped_count: 0,
            broadcaster_dropped_ref: Arc::clone(&self.total_dropped),
        }
    }

    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn stats(&self) -> BroadcasterStats {
        BroadcasterStats {
            capacity: self.capacity,
            active_subscribers: self.subscriber_count(),
            total_published: self.total_published.load(Ordering::Relaxed),
            total_dropped: self.total_dropped.load(Ordering::Relaxed),
        }
    }
}

impl Default for MalocaEventBroadcaster {
    fn default() -> Self {
        Self::new(1024)
    }
}

/// Subscriber handle managing event filtering and slow-client lag tracking.
pub struct MalocaSubscriber {
    receiver: broadcast::Receiver<MalocaEvent>,
    filters: Vec<MalocaEventFilter>,
    skipped_count: u64,
    broadcaster_dropped_ref: Arc<AtomicU64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MalocaSubscriberError {
    Lagged(u64),
    Closed,
}

impl MalocaSubscriber {
    pub fn add_filter(&mut self, filter: MalocaEventFilter) {
        self.filters.push(filter);
    }

    pub fn clear_filters(&mut self) {
        self.filters.clear();
    }

    pub fn filters(&self) -> &[MalocaEventFilter] {
        &self.filters
    }

    pub fn skipped_count(&self) -> u64 {
        self.skipped_count
    }

    fn matches_filters(&self, event: &MalocaEvent) -> bool {
        if self.filters.is_empty() {
            return true;
        }
        self.filters.iter().any(|f| f.matches(event))
    }

    /// Asynchronously receives the next matching event.
    /// In case of client lag (broadcast buffer overflow), updates lag metrics and returns `Err(Lagged(n))`.
    pub async fn recv(&mut self) -> Result<MalocaEvent, MalocaSubscriberError> {
        loop {
            match self.receiver.recv().await {
                Ok(event) => {
                    if self.matches_filters(&event) {
                        return Ok(event);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    self.skipped_count += n;
                    self.broadcaster_dropped_ref.fetch_add(n, Ordering::Relaxed);
                    return Err(MalocaSubscriberError::Lagged(n));
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(MalocaSubscriberError::Closed);
                }
            }
        }
    }
}

/// Client-to-server WebSocket protocol messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsClientMessage {
    Subscribe {
        #[serde(default)]
        project_id: Option<String>,
        #[serde(default)]
        event_type: Option<String>,
    },
    Unsubscribe {
        #[serde(default)]
        project_id: Option<String>,
        #[serde(default)]
        event_type: Option<String>,
    },
    Ping,
}

/// Server-to-client WebSocket protocol messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsServerMessage {
    Event(MalocaEvent),
    Subscribed {
        #[serde(default)]
        project_id: Option<String>,
        #[serde(default)]
        event_type: Option<String>,
    },
    Unsubscribed {
        #[serde(default)]
        project_id: Option<String>,
        #[serde(default)]
        event_type: Option<String>,
    },
    Pong,
    Lagged {
        skipped: u64,
    },
    Error {
        message: String,
    },
}

/// Axum WebSocket route handler for Maloca live sync stream.
pub async fn ws_maloca_live_sync_handler(
    ws: WebSocketUpgrade,
    State(broadcaster): State<Arc<MalocaEventBroadcaster>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_maloca_ws_socket(socket, broadcaster))
}

async fn handle_maloca_ws_socket(mut socket: WebSocket, broadcaster: Arc<MalocaEventBroadcaster>) {
    let mut subscriber = broadcaster.subscribe();
    info!("maloca ws live sync: client connected");

    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<WsClientMessage>(&text) {
                            Ok(WsClientMessage::Subscribe { project_id, event_type }) => {
                                subscriber.add_filter(MalocaEventFilter::new(project_id.clone(), event_type.clone()));
                                let resp = WsServerMessage::Subscribed { project_id, event_type };
                                let _ = socket.send(Message::Text(serde_json::to_string(&resp).unwrap_or_default().into())).await;
                            }
                            Ok(WsClientMessage::Unsubscribe { project_id, event_type }) => {
                                let target = MalocaEventFilter::new(project_id.clone(), event_type.clone());
                                subscriber.filters.retain(|f| f != &target);
                                let resp = WsServerMessage::Unsubscribed { project_id, event_type };
                                let _ = socket.send(Message::Text(serde_json::to_string(&resp).unwrap_or_default().into())).await;
                            }
                            Ok(WsClientMessage::Ping) => {
                                let resp = WsServerMessage::Pong;
                                let _ = socket.send(Message::Text(serde_json::to_string(&resp).unwrap_or_default().into())).await;
                            }
                            Err(e) => {
                                let resp = WsServerMessage::Error { message: format!("invalid client payload: {}", e) };
                                let _ = socket.send(Message::Text(serde_json::to_string(&resp).unwrap_or_default().into())).await;
                            }
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!("maloca ws live sync: client disconnected");
                        break;
                    }
                    Some(Err(e)) => {
                        warn!("maloca ws live sync: read error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
            res = subscriber.recv() => {
                match res {
                    Ok(event) => {
                        let msg = WsServerMessage::Event(event);
                        if let Ok(json) = serde_json::to_string(&msg) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(MalocaSubscriberError::Lagged(skipped)) => {
                        let msg = WsServerMessage::Lagged { skipped };
                        if let Ok(json) = serde_json::to_string(&msg) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(MalocaSubscriberError::Closed) => {
                        break;
                    }
                }
            }
        }
    }
    info!("maloca ws live sync: socket session finished");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_parsing() {
        assert_eq!(MalocaEventType::parse_str("proposals"), MalocaEventType::Proposals);
        assert_eq!(MalocaEventType::parse_str("PROPOSAL"), MalocaEventType::Proposals);
        assert_eq!(MalocaEventType::parse_str("votes"), MalocaEventType::Votes);
        assert_eq!(MalocaEventType::parse_str("decisions"), MalocaEventType::Decisions);
        assert_eq!(MalocaEventType::parse_str("beliefs"), MalocaEventType::Beliefs);
        assert_eq!(
            MalocaEventType::parse_str("custom_type"),
            MalocaEventType::Custom("custom_type".to_string())
        );
    }

    #[test]
    fn test_filter_matching() {
        let filter_proj = MalocaEventFilter::new(Some("proj-1".to_string()), None);
        let filter_type = MalocaEventFilter::new(None, Some("proposals".to_string()));
        let filter_both = MalocaEventFilter::new(Some("proj-1".to_string()), Some("proposals".to_string()));

        let event_match = MalocaEvent::proposal(Some("proj-1"), serde_json::json!({"title": "P1"}));
        let event_other_proj = MalocaEvent::proposal(Some("proj-2"), serde_json::json!({"title": "P2"}));
        let event_other_type = MalocaEvent::vote(Some("proj-1"), serde_json::json!({"choice": "yes"}));

        assert!(filter_proj.matches(&event_match));
        assert!(filter_proj.matches(&event_other_type));
        assert!(!filter_proj.matches(&event_other_proj));

        assert!(filter_type.matches(&event_match));
        assert!(filter_type.matches(&event_other_proj));
        assert!(!filter_type.matches(&event_other_type));

        assert!(filter_both.matches(&event_match));
        assert!(!filter_both.matches(&event_other_proj));
        assert!(!filter_both.matches(&event_other_type));
    }
}
