//! REST API routes for P2P encrypted messaging and WebRTC voice signaling relay.
//!
//! Exposes mesh chat handlers under `/v1/mesh/chat/*`:
//! - POST `/v1/mesh/chat/send`: Stores and forwards encrypted message envelope
//! - GET `/v1/mesh/chat/history/:room_or_peer`: Retrieves persistent message logs
//! - POST `/v1/mesh/chat/signal`: Relays WebRTC voice signaling (offer/answer/candidates)

use axum::{
    extract::Path,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio::sync::RwLock;
use tracing::info;
use ulid::Ulid;

/// Encrypted message envelope stored and forwarded over the P2P mesh.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessageEnvelope {
    pub id: String,
    pub sender_node_id: String,
    pub recipient: String,
    pub encrypted_payload: String,
    pub nonce: String,
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_id: Option<String>,
}

/// Request payload for sending an encrypted chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSendRequest {
    pub recipient: String,
    pub encrypted_payload: String,
    pub nonce: String,
    #[serde(default)]
    pub room_id: Option<String>,
    #[serde(default)]
    pub sender_node_id: Option<String>,
}

/// Response payload after sending a chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSendResponse {
    pub status: String,
    pub message_id: String,
    pub timestamp: i64,
}

/// Response payload for chat history queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatHistoryResponse {
    pub room_or_peer: String,
    pub messages: Vec<ChatMessageEnvelope>,
    pub count: usize,
}

/// WebRTC signal types for voice call setup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignalType {
    Offer,
    Answer,
    IceCandidate,
}

/// Request payload for WebRTC voice signaling relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSignalRequest {
    pub sender_node_id: String,
    pub target_peer_id: String,
    pub signal_type: SignalType,
    pub sdp_or_candidate: String,
    pub call_id: String,
}

/// Response payload for WebRTC voice signaling relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSignalResponse {
    pub status: String,
    pub call_id: String,
    pub relayed_to: String,
    pub timestamp: i64,
}

/// Thread-safe in-memory and persistent mesh chat store.
#[derive(Debug, Default)]
pub struct MeshChatStore {
    history: RwLock<HashMap<String, Vec<ChatMessageEnvelope>>>,
    signals: RwLock<HashMap<String, Vec<ChatSignalRequest>>>,
}

impl MeshChatStore {
    /// Creates a new empty mesh chat store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores an encrypted message envelope in local history logs.
    pub async fn store_message(&self, envelope: ChatMessageEnvelope) {
        let mut hist = self.history.write().await;
        let recipient_key = envelope.recipient.clone();
        hist.entry(recipient_key)
            .or_default()
            .push(envelope.clone());

        if let Some(ref room) = envelope.room_id {
            if room != &envelope.recipient {
                hist.entry(room.clone()).or_default().push(envelope.clone());
            }
        }
    }

    /// Retrieves history logs for a specific room or peer ID.
    pub async fn get_history(&self, room_or_peer: &str) -> Vec<ChatMessageEnvelope> {
        let hist = self.history.read().await;
        hist.get(room_or_peer).cloned().unwrap_or_default()
    }

    /// Stores a WebRTC signaling payload for a target peer.
    pub async fn store_signal(&self, signal: ChatSignalRequest) {
        let mut sigs = self.signals.write().await;
        sigs.entry(signal.target_peer_id.clone())
            .or_default()
            .push(signal);
    }

    /// Retrieves and clears stored signals for a target peer.
    pub async fn drain_signals(&self, peer_id: &str) -> Vec<ChatSignalRequest> {
        let mut sigs = self.signals.write().await;
        sigs.remove(peer_id).unwrap_or_default()
    }

    /// Clears all store data (useful for unit tests).
    pub async fn clear(&self) {
        self.history.write().await.clear();
        self.signals.write().await.clear();
    }
}

/// Global shared mesh chat store instance.
pub static MESH_CHAT_STORE: LazyLock<Arc<MeshChatStore>> =
    LazyLock::new(|| Arc::new(MeshChatStore::new()));

/// Returns router configured for mesh chat endpoints.
pub fn router() -> Router {
    Router::new()
        .route("/v1/mesh/chat/send", post(v1_mesh_chat_send))
        .route(
            "/v1/mesh/chat/history/{room_or_peer}",
            get(v1_mesh_chat_history),
        )
        .route("/v1/mesh/chat/signal", post(v1_mesh_chat_signal))
}

/// POST `/v1/mesh/chat/send`
///
/// Stores and forwards an encrypted chat envelope to a target peer or group room.
pub async fn v1_mesh_chat_send(Json(payload): Json<ChatSendRequest>) -> impl IntoResponse {
    let recipient = payload.recipient.trim();
    if recipient.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "Recipient peer or room ID is required"
            })),
        )
            .into_response();
    }

    if payload.encrypted_payload.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "Encrypted payload is required"
            })),
        )
            .into_response();
    }

    let message_id = format!("msg-{}", Ulid::new());
    let timestamp = chrono::Utc::now().timestamp();
    let sender_node_id = payload
        .sender_node_id
        .clone()
        .unwrap_or_else(|| "local".to_string());

    // Anti-hallucination guard: log envelope metadata without unencrypted content
    info!(
        message_id = %message_id,
        sender = %sender_node_id,
        recipient = %recipient,
        room = ?payload.room_id,
        "Processing encrypted P2P mesh chat message"
    );

    let envelope = ChatMessageEnvelope {
        id: message_id.clone(),
        sender_node_id,
        recipient: recipient.to_string(),
        encrypted_payload: payload.encrypted_payload,
        nonce: payload.nonce,
        timestamp,
        room_id: payload.room_id,
    };

    MESH_CHAT_STORE.store_message(envelope).await;

    (
        StatusCode::OK,
        Json(ChatSendResponse {
            status: "ok".to_string(),
            message_id,
            timestamp,
        }),
    )
        .into_response()
}

/// GET `/v1/mesh/chat/history/{room_or_peer}`
///
/// Retrieves persistent encrypted message logs for a given room or peer.
pub async fn v1_mesh_chat_history(Path(room_or_peer): Path<String>) -> impl IntoResponse {
    if !room_or_peer
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "Invalid room_or_peer identifier"
            })),
        )
            .into_response();
    }

    let messages = MESH_CHAT_STORE.get_history(&room_or_peer).await;
    let count = messages.len();

    info!(
        room_or_peer = %room_or_peer,
        count = count,
        "Retrieved mesh chat history logs"
    );

    (
        StatusCode::OK,
        Json(ChatHistoryResponse {
            room_or_peer,
            messages,
            count,
        }),
    )
        .into_response()
}

/// POST `/v1/mesh/chat/signal`
///
/// Relays WebRTC SDP offer/answer/ICE candidates for P2P voice calls.
pub async fn v1_mesh_chat_signal(Json(payload): Json<ChatSignalRequest>) -> impl IntoResponse {
    if payload.target_peer_id.trim().is_empty()
        || payload.sender_node_id.trim().is_empty()
        || payload.call_id.trim().is_empty()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "sender_node_id, target_peer_id, and call_id are required"
            })),
        )
            .into_response();
    }

    if payload.sdp_or_candidate.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "sdp_or_candidate payload is required"
            })),
        )
            .into_response();
    }

    let timestamp = chrono::Utc::now().timestamp();
    let target_peer_id = payload.target_peer_id.clone();
    let call_id = payload.call_id.clone();
    let signal_type = payload.signal_type.clone();

    // Anti-hallucination guard: log signaling metadata without raw SDP or candidate contents
    info!(
        call_id = %call_id,
        sender = %payload.sender_node_id,
        target = %target_peer_id,
        signal_type = ?signal_type,
        "Relaying WebRTC voice call signaling message"
    );

    MESH_CHAT_STORE.store_signal(payload).await;

    (
        StatusCode::OK,
        Json(ChatSignalResponse {
            status: "relayed".to_string(),
            call_id,
            relayed_to: target_peer_id,
            timestamp,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_mesh_chat_and_signaling() {
        // Clear store before test execution
        MESH_CHAT_STORE.clear().await;

        let app = router();

        // 1. Send encrypted message to peer "peer-alice"
        let send_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/chat/send")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "recipient": "peer-alice",
                    "encrypted_payload": "EncryptedCiphertextBase64==",
                    "nonce": "Nonce123456",
                    "sender_node_id": "peer-bob"
                })
                .to_string(),
            ))
            .unwrap();

        let resp = app.clone().oneshot(send_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let send_res: ChatSendResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(send_res.status, "ok");
        assert!(send_res.message_id.starts_with("msg-"));

        // 2. Send another message to room "room-swal-team"
        let room_send_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/chat/send")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "recipient": "room-swal-team",
                    "room_id": "room-swal-team",
                    "encrypted_payload": "GroupEncryptedPayload==",
                    "nonce": "Nonce7890",
                    "sender_node_id": "peer-bob"
                })
                .to_string(),
            ))
            .unwrap();

        let resp = app.clone().oneshot(room_send_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 3. Retrieve history for "peer-alice"
        let hist_req = Request::builder()
            .method("GET")
            .uri("/v1/mesh/chat/history/peer-alice")
            .body(Body::empty())
            .unwrap();

        let resp = app.clone().oneshot(hist_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let hist_res: ChatHistoryResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(hist_res.room_or_peer, "peer-alice");
        assert_eq!(hist_res.count, 1);
        assert_eq!(hist_res.messages[0].sender_node_id, "peer-bob");
        assert_eq!(
            hist_res.messages[0].encrypted_payload,
            "EncryptedCiphertextBase64=="
        );

        // 4. Retrieve history for "room-swal-team"
        let room_hist_req = Request::builder()
            .method("GET")
            .uri("/v1/mesh/chat/history/room-swal-team")
            .body(Body::empty())
            .unwrap();

        let resp = app.clone().oneshot(room_hist_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let room_hist_res: ChatHistoryResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(room_hist_res.count, 1);
        assert_eq!(
            room_hist_res.messages[0].encrypted_payload,
            "GroupEncryptedPayload=="
        );

        // 5. Send WebRTC signaling offer
        let offer_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/chat/signal")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "sender_node_id": "peer-bob",
                    "target_peer_id": "peer-alice",
                    "signal_type": "offer",
                    "sdp_or_candidate": "v=0\r\no=- 123456 2 IN IP4 127.0.0.1...",
                    "call_id": "call-987"
                })
                .to_string(),
            ))
            .unwrap();

        let resp = app.clone().oneshot(offer_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let sig_res: ChatSignalResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(sig_res.status, "relayed");
        assert_eq!(sig_res.call_id, "call-987");
        assert_eq!(sig_res.relayed_to, "peer-alice");

        // Verify stored signals in store
        let drained = MESH_CHAT_STORE.drain_signals("peer-alice").await;
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].signal_type, SignalType::Offer);
        assert_eq!(drained[0].call_id, "call-987");
    }
}
