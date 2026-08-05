//! Maloca WebSocket Live Feed (MS-004)
//!
//! Real-time event feed consumed by swal-backoffice (`/maloca/ws/feed`):
//! subscribes to the internal `XavierEventBus` and streams every event as a
//! JSON frame. Also answers client `ping` frames with `pong`.
//!
//! Contract:
//!   WS /maloca/ws/feed
//!   Server frames: { "type": "xavier_event", "event": {...} } | { "type": "ping" }

use crate::coordination::events::XavierEventBus;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Extension, WebSocketUpgrade};
use axum::response::IntoResponse;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{info, warn};

pub async fn ws_live_feed(
    Extension(bus): Extension<XavierEventBus>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, bus))
}

async fn handle_socket(mut socket: WebSocket, bus: XavierEventBus) {
    let mut rx = bus.subscribe();

    // Heartbeat ping every 25s to keep the connection alive through proxies.
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

    info!("maloca ws feed: client connected");

    loop {
        tokio::select! {
            // Inbound client messages (subscribe/unsubscribe/filter — parsed, currently ack-only).
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                            if v.get("type").and_then(|t| t.as_str()) == Some("ping") {
                                let _ = socket.send(Message::Text(
                                    serde_json::json!({ "type": "pong" }).to_string().into(),
                                )).await;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!("maloca ws feed: client disconnected");
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
            // Heartbeat tick.
            _ = ping_rx.recv() => {
                if socket.send(Message::Text(serde_json::json!({ "type": "ping" }).to_string().into())).await.is_err() {
                    break;
                }
            }
        }
    }

    ping_task.abort();
}
