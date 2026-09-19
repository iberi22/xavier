//! Axum REST & WebSocket endpoints for telecom integration (Issue #2372 / WAVE-26.10)

use axum::{
    extract::{ws::Message as WsMessage, ws::WebSocket, ws::WebSocketUpgrade, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error)]
pub enum GatewayApiError {
    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Room not found: {0}")]
    NotFound(String),

    #[error("Internal server error: {0}")]
    Internal(String),
}

impl IntoResponse for GatewayApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, msg) = match self {
            GatewayApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            GatewayApiError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            GatewayApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRoomRequest {
    pub room_id: String,
    pub title: String,
    pub creator_node: String,
    pub clearance_level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub room_id: String,
    pub sender_node: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInfo {
    pub room_id: String,
    pub title: String,
    pub creator_node: String,
    pub message_count: usize,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsSessionMessage {
    pub event_type: String,
    pub room_id: String,
    pub sender: String,
    pub payload: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone)]
pub struct TelecomRouterState {
    pub rooms: Arc<RwLock<HashMap<String, RoomInfo>>>,
    pub messages: Arc<RwLock<HashMap<String, Vec<WsSessionMessage>>>>,
}

impl TelecomRouterState {
    pub fn new() -> Self {
        Self {
            rooms: Arc::new(RwLock::new(HashMap::new())),
            messages: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for TelecomRouterState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn telecom_routes(state: TelecomRouterState) -> Router {
    Router::new()
        .route(
            "/v1/telecom/rooms",
            post(handle_create_room).get(handle_list_rooms),
        )
        .route("/v1/telecom/messages", post(handle_send_message))
        .route("/v1/telecom/ws", get(handle_ws_upgrade))
        .with_state(state)
}

pub async fn handle_create_room(
    State(state): State<TelecomRouterState>,
    Json(req): Json<CreateRoomRequest>,
) -> Result<Json<RoomInfo>, GatewayApiError> {
    if req.room_id.is_empty() || req.title.is_empty() {
        return Err(GatewayApiError::BadRequest(
            "Missing room_id or title".into(),
        ));
    }

    let mut lock = state.rooms.write().await;
    if lock.contains_key(&req.room_id) {
        return Err(GatewayApiError::BadRequest("Room already exists".into()));
    }

    let room = RoomInfo {
        room_id: req.room_id.clone(),
        title: req.title,
        creator_node: req.creator_node,
        message_count: 0,
        created_at: Utc::now(),
    };

    lock.insert(req.room_id, room.clone());
    Ok(Json(room))
}

pub async fn handle_list_rooms(State(state): State<TelecomRouterState>) -> Json<Vec<RoomInfo>> {
    let lock = state.rooms.read().await;
    let list: Vec<RoomInfo> = lock.values().cloned().collect();
    Json(list)
}

pub async fn handle_send_message(
    State(state): State<TelecomRouterState>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<WsSessionMessage>, GatewayApiError> {
    let mut rooms = state.rooms.write().await;
    let room = rooms
        .get_mut(&req.room_id)
        .ok_or_else(|| GatewayApiError::NotFound(format!("Room {} not found", req.room_id)))?;

    room.message_count += 1;

    let msg = WsSessionMessage {
        event_type: "chat_message".into(),
        room_id: req.room_id.clone(),
        sender: req.sender_node,
        payload: req.content,
        timestamp: Utc::now(),
    };

    let mut msgs = state.messages.write().await;
    msgs.entry(req.room_id).or_default().push(msg.clone());

    Ok(Json(msg))
}

pub async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(_state): State<TelecomRouterState>,
) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    while let Some(Ok(msg)) = socket.recv().await {
        if let WsMessage::Text(text) = msg {
            if socket
                .send(WsMessage::Text(format!("ack: {}", text).into()))
                .await
                .is_err()
            {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_list_rooms() {
        let state = TelecomRouterState::new();
        let req = CreateRoomRequest {
            room_id: "sec-01".into(),
            title: "Security Ops".into(),
            creator_node: "node_alice".into(),
            clearance_level: Some("Confidential".into()),
        };

        let res = handle_create_room(State(state.clone()), Json(req)).await;
        assert!(res.is_ok());
        let info = res.unwrap().0;
        assert_eq!(info.room_id, "sec-01");

        let list = handle_list_rooms(State(state.clone())).await.0;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Security Ops");
    }

    #[tokio::test]
    async fn test_send_message_flow() {
        let state = TelecomRouterState::new();
        let room_req = CreateRoomRequest {
            room_id: "chat-01".into(),
            title: "General".into(),
            creator_node: "admin".into(),
            clearance_level: None,
        };
        let _ = handle_create_room(State(state.clone()), Json(room_req))
            .await
            .unwrap();

        let msg_req = SendMessageRequest {
            room_id: "chat-01".into(),
            sender_node: "alice".into(),
            content: "Hello mesh".into(),
        };

        let res = handle_send_message(State(state.clone()), Json(msg_req)).await;
        assert!(res.is_ok());
        let msg = res.unwrap().0;
        assert_eq!(msg.payload, "Hello mesh");

        let lock = state.rooms.read().await;
        assert_eq!(lock.get("chat-01").unwrap().message_count, 1);
    }

    #[tokio::test]
    async fn test_send_message_room_not_found() {
        let state = TelecomRouterState::new();
        let msg_req = SendMessageRequest {
            room_id: "nonexistent".into(),
            sender_node: "alice".into(),
            content: "Hello mesh".into(),
        };

        let res = handle_send_message(State(state), Json(msg_req)).await;
        assert!(res.is_err());
    }
}
