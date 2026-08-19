//! Maloca WebSocket Live Feed (MS-004)
//!
//! Real-time event feed consumed by swal-backoffice (`/maloca/ws/feed`):
//! subscribes to the internal `XavierEventBus` and streams every event as a
//! JSON frame. Also answers client `ping` frames with `pong`, handles WS protocol
//! ping/pong heartbeats, client disconnect detection, exponential backoff tracking,
//! and feed status reporting.
//!
//! Contract:
//!   WS /maloca/ws/feed
//!   Server frames: { "type": "xavier_event", "event": {...} } | { "type": "ping" }
//!   Status route: GET /maloca/feed/status

use crate::coordination::events::XavierEventBus;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Extension, WebSocketUpgrade};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{info, warn};

static CONNECTED_CLIENTS: AtomicUsize = AtomicUsize::new(0);
static LAST_HEARTBEAT_AT: AtomicI64 = AtomicI64::new(0);
static RECONNECT_COUNT: AtomicU64 = AtomicU64::new(0);

/// Feed status response containing active clients, last heartbeat timestamp, and backoff state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeedStatus {
    pub connected_clients: usize,
    pub last_heartbeat_at: Option<String>,
    pub last_heartbeat_timestamp: i64,
    pub reconnect_count: u64,
    pub reconnect_backoff_ms: u64,
    pub status: String,
}

/// Helper function to update `LAST_HEARTBEAT_AT` to current UTC timestamp (seconds).
pub fn record_heartbeat() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    LAST_HEARTBEAT_AT.store(now, Ordering::Relaxed);
}

/// Calculate current exponential backoff duration based on reconnect attempts.
pub fn calculate_reconnect_backoff_ms(attempts: u64) -> u64 {
    let base_ms = 500u64;
    let max_ms = 30_000u64;
    if attempts == 0 {
        return 0;
    }
    let exponent = (attempts - 1).min(6) as u32;
    (base_ms * (1u64 << exponent)).min(max_ms)
}

/// Retrieve the current feed status metadata.
pub fn get_feed_status() -> FeedStatus {
    let active = CONNECTED_CLIENTS.load(Ordering::Relaxed);
    let last_hb = LAST_HEARTBEAT_AT.load(Ordering::Relaxed);
    let reconnects = RECONNECT_COUNT.load(Ordering::Relaxed);
    let backoff_ms = calculate_reconnect_backoff_ms(reconnects);

    let last_heartbeat_at = if last_hb > 0 {
        use chrono::{TimeZone, Utc};
        Utc.timestamp_opt(last_hb, 0)
            .single()
            .map(|dt| dt.to_rfc3339())
    } else {
        None
    };

    let status = if active > 0 {
        "connected"
    } else if reconnects > 0 {
        "reconnecting"
    } else {
        "idle"
    };

    FeedStatus {
        connected_clients: active,
        last_heartbeat_at,
        last_heartbeat_timestamp: last_hb,
        reconnect_count: reconnects,
        reconnect_backoff_ms: backoff_ms,
        status: status.to_string(),
    }
}

pub async fn ws_live_feed(
    Extension(bus): Extension<XavierEventBus>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, bus))
}

async fn handle_socket(mut socket: WebSocket, bus: XavierEventBus) {
    let prev_clients = CONNECTED_CLIENTS.fetch_add(1, Ordering::Relaxed);
    if prev_clients == 0 {
        let reconnects = RECONNECT_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        let backoff_ms = calculate_reconnect_backoff_ms(reconnects);
        info!(
            "maloca ws feed: client reconnected after server restart / session drop (attempt #{}, backoff {}ms)",
            reconnects, backoff_ms
        );
    } else {
        info!(
            "maloca ws feed: client connected (active: {})",
            prev_clients + 1
        );
    }

    record_heartbeat();

    let mut rx = bus.subscribe();

    // Heartbeat ping task (every 25s) to keep connection alive and verify client responsiveness.
    let (ping_tx, mut ping_rx) = tokio::sync::mpsc::channel::<()>(1);
    let ping_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(25));
        loop {
            interval.tick().await;
            if ping_tx.send(()).await.is_err() {
                break;
            }
        }
    });

    loop {
        tokio::select! {
            // Inbound client messages: text pings, WebSocket Ping/Pong protocol frames.
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        record_heartbeat();
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                            if v.get("type").and_then(|t| t.as_str()) == Some("ping") {
                                let frame = serde_json::json!({ "type": "pong" }).to_string();
                                if socket.send(Message::Text(frame.into())).await.is_err() {
                                    warn!("maloca ws feed: failed sending pong response to client");
                                    break;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        record_heartbeat();
                        if socket.send(Message::Pong(data)).await.is_err() {
                            warn!("maloca ws feed: pong response failed");
                            break;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {
                        record_heartbeat();
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!("maloca ws feed: client disconnected");
                        break;
                    }
                    Some(Err(e)) => {
                        warn!("maloca ws feed: client read error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
            // Outbound events from the Xavier event bus.
            ev = rx.recv() => {
                match ev {
                    Ok(event) => {
                        let frame = serde_json::json!({
                            "type": "xavier_event",
                            "event": event,
                        });
                        if socket.send(Message::Text(frame.to_string().into())).await.is_err() {
                            warn!("maloca ws feed: send failed, dropping client");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        let frame = serde_json::json!({
                            "type": "lagged",
                            "skipped": n,
                        });
                        let _ = socket.send(Message::Text(frame.to_string().into())).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            // Heartbeat tick: send WS Ping or json ping frame to verify client presence.
            _ = ping_rx.recv() => {
                if socket.send(Message::Ping(vec![].into())).await.is_err() {
                    warn!("maloca ws feed: ping send failed, client connection lost");
                    break;
                }
            }
        }
    }

    ping_task.abort();
    CONNECTED_CLIENTS.fetch_sub(1, Ordering::Relaxed);
    info!("maloca ws feed: client disconnect processed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feed_status_recording_and_backoff() {
        record_heartbeat();
        let status = get_feed_status();
        assert!(status.last_heartbeat_timestamp > 0);
        assert!(status.last_heartbeat_at.is_some());

        assert_eq!(calculate_reconnect_backoff_ms(0), 0);
        assert_eq!(calculate_reconnect_backoff_ms(1), 500);
        assert_eq!(calculate_reconnect_backoff_ms(2), 1000);
        assert_eq!(calculate_reconnect_backoff_ms(3), 2000);
        assert_eq!(calculate_reconnect_backoff_ms(10), 30000);
    }
}
