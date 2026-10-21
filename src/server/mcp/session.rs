//! MCP session management
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use super::types::*;
use crate::workspace::WorkspaceContext;
use crate::AppState;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use futures_util::stream::StreamExt;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};
use std::time::{Duration, Instant};
use tracing::info;

pub const MCP_PROTOCOL_VERSION: &str = "2026-07-28";
pub const ERROR_HEADER_MISMATCH: i32 = -32020;
pub const DEFAULT_PAYLOAD_LIMIT_BYTES: usize = 4 * 1024 * 1024; // 4MB
pub const DEFAULT_TIMEOUT_DURATION: Duration = Duration::from_secs(30);

// ── MCP Session Lifecycle ──────────────────────────────────────────

/// Session states for Model Context Protocol sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum McpSessionState {
    Connected,
    Streaming,
    TimedOut,
    Disconnected,
    Closed,
}

/// Errors related to session lifecycle management.
#[derive(Debug, thiserror::Error)]
pub enum McpSessionError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("Payload size limit exceeded: {actual} bytes exceeds limit of {limit} bytes")]
    PayloadLimitExceeded { limit: usize, actual: usize },
    #[error("Session timed out after {0:?}")]
    TimedOut(Duration),
    #[error("Session disconnected by client (abrupt EOF)")]
    Disconnected,
    #[error("Invalid session state transition from {from:?} to {to:?}")]
    InvalidStateTransition {
        from: McpSessionState,
        to: McpSessionState,
    },
}

/// Represents an active or historical MCP session.
#[derive(Debug, Clone)]
pub struct McpSession {
    pub id: String,
    pub state: McpSessionState,
    pub created_at: Instant,
    pub last_activity: Instant,
    pub bytes_transferred: usize,
    pub payload_limit_bytes: usize,
    pub timeout_duration: Duration,
}

static GLOBAL_SESSION_MANAGER: LazyLock<McpSessionManager> = LazyLock::new(McpSessionManager::new);

/// Thread-safe manager for tracking MCP session lifecycles.
#[derive(Debug, Clone)]
pub struct McpSessionManager {
    sessions: Arc<RwLock<HashMap<String, McpSession>>>,
}

impl Default for McpSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl McpSessionManager {
    /// Create a new session manager instance.
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Access global shared session manager.
    pub fn global() -> &'static McpSessionManager {
        &GLOBAL_SESSION_MANAGER
    }

    /// Register or create a session with optional custom payload limit and timeout.
    pub fn create_session(
        &self,
        id: impl Into<String>,
        payload_limit: Option<usize>,
        timeout: Option<Duration>,
    ) -> McpSession {
        let id_str = id.into();
        let now = Instant::now();
        let session = McpSession {
            id: id_str.clone(),
            state: McpSessionState::Connected,
            created_at: now,
            last_activity: now,
            bytes_transferred: 0,
            payload_limit_bytes: payload_limit.unwrap_or(DEFAULT_PAYLOAD_LIMIT_BYTES),
            timeout_duration: timeout.unwrap_or(DEFAULT_TIMEOUT_DURATION),
        };

        if let Ok(mut map) = self.sessions.write() {
            map.insert(id_str, session.clone());
        }
        session
    }

    /// Retrieve session snapshot if present.
    pub fn get_session(&self, id: &str) -> Option<McpSession> {
        self.sessions
            .read()
            .ok()
            .and_then(|map| map.get(id).cloned())
    }

    /// Retrieve current state of session.
    pub fn get_state(&self, id: &str) -> Option<McpSessionState> {
        self.get_session(id).map(|s| s.state)
    }

    /// Transition session to a target state.
    pub fn transition_state(
        &self,
        id: &str,
        target_state: McpSessionState,
    ) -> Result<McpSessionState, McpSessionError> {
        let mut map = self
            .sessions
            .write()
            .map_err(|_| McpSessionError::SessionNotFound(id.to_string()))?;

        let session = map
            .get_mut(id)
            .ok_or_else(|| McpSessionError::SessionNotFound(id.to_string()))?;

        let old_state = session.state;
        session.state = target_state;
        session.last_activity = Instant::now();
        Ok(old_state)
    }

    /// Record payload bytes transferred in session, failing if limit exceeded.
    pub fn record_bytes(&self, id: &str, bytes: usize) -> Result<usize, McpSessionError> {
        let mut map = self
            .sessions
            .write()
            .map_err(|_| McpSessionError::SessionNotFound(id.to_string()))?;

        let session = map
            .get_mut(id)
            .ok_or_else(|| McpSessionError::SessionNotFound(id.to_string()))?;

        if bytes > session.payload_limit_bytes {
            session.state = McpSessionState::Closed;
            return Err(McpSessionError::PayloadLimitExceeded {
                limit: session.payload_limit_bytes,
                actual: bytes,
            });
        }

        session.bytes_transferred += bytes;
        session.last_activity = Instant::now();
        Ok(session.bytes_transferred)
    }

    /// Close session gracefully.
    pub fn close_session(&self, id: &str) -> Result<(), McpSessionError> {
        self.transition_state(id, McpSessionState::Closed)?;
        Ok(())
    }

    /// Mark session disconnected due to abrupt EOF or client channel drop.
    pub fn handle_disconnect(&self, id: &str) -> Result<(), McpSessionError> {
        self.transition_state(id, McpSessionState::Disconnected)?;
        Ok(())
    }

    /// Prune sessions inactive longer than max_idle.
    pub fn prune_stale_sessions(&self, max_idle: Duration) -> usize {
        let mut map = match self.sessions.write() {
            Ok(m) => m,
            Err(_) => return 0,
        };

        let now = Instant::now();
        let initial = map.len();
        map.retain(|_, session| now.duration_since(session.last_activity) <= max_idle);
        initial - map.len()
    }

    /// Return total active session count.
    pub fn active_session_count(&self) -> usize {
        self.sessions.read().map(|m| m.len()).unwrap_or(0)
    }
}

/// Wraps execution of an MCP query with timeout detection and session state updates.
pub async fn execute_query_with_timeout<F, T>(
    manager: &McpSessionManager,
    session_id: &str,
    timeout_dur: Duration,
    fut: F,
) -> Result<T, McpSessionError>
where
    F: std::future::Future<Output = T>,
{
    let _ = manager.transition_state(session_id, McpSessionState::Streaming);
    match tokio::time::timeout(timeout_dur, fut).await {
        Ok(result) => {
            let _ = manager.transition_state(session_id, McpSessionState::Connected);
            Ok(result)
        }
        Err(_) => {
            let _ = manager.transition_state(session_id, McpSessionState::TimedOut);
            Err(McpSessionError::TimedOut(timeout_dur))
        }
    }
}

/// Streams large tool responses and detects abrupt client disconnect / EOF.
pub async fn stream_tool_response_with_cancellation<S, B>(
    manager: &McpSessionManager,
    session_id: &str,
    mut stream: S,
    tx: tokio::sync::mpsc::Sender<B>,
) -> Result<usize, McpSessionError>
where
    S: futures_util::stream::Stream<Item = B> + Unpin,
{
    let _ = manager.transition_state(session_id, McpSessionState::Streaming);
    let mut total_chunks = 0;

    while let Some(chunk) = stream.next().await {
        if tx.send(chunk).await.is_err() {
            let _ = manager.transition_state(session_id, McpSessionState::Disconnected);
            return Err(McpSessionError::Disconnected);
        }
        total_chunks += 1;
    }

    let _ = manager.transition_state(session_id, McpSessionState::Connected);
    Ok(total_chunks)
}

/// Mcp delete handler to gracefully close session.
pub async fn mcp_delete_handler(headers: HeaderMap) -> Response {
    let session_id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("default");

    match McpSessionManager::global().close_session(session_id) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "session_closed",
                "session_id": session_id
            })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": error.to_string()
            })),
        )
            .into_response(),
    }
}

/// Mcp sse stream handler for server-sent events notification stream.
pub async fn mcp_sse_handler(headers: HeaderMap) -> Response {
    let session_id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("default");

    let manager = McpSessionManager::global();
    if manager.get_session(session_id).is_none() {
        manager.create_session(session_id, None, None);
    }
    let _ = manager.transition_state(session_id, McpSessionState::Connected);

    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        format!(
            "event: connected\ndata: {{\"session_id\": \"{}\"}}\n\n",
            session_id
        ),
    )
        .into_response()
}

/// Mcp post handler.
pub async fn mcp_post_handler(
    State(state): State<AppState>,
    Extension(workspace): Extension<WorkspaceContext>,
    claims: Option<Extension<crate::security::auth::Claims>>,
    write_caller: Option<
        Extension<crate::adapters::inbound::http::middleware::rate_limit::MemoryWriteCallerKey>,
    >,
    client: Option<Extension<axum::extract::ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    static PRUNE_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let manager = McpSessionManager::global();

    if PRUNE_COUNTER
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        .is_multiple_of(100)
    {
        manager.prune_stale_sessions(DEFAULT_TIMEOUT_DURATION);
    }

    let session_id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("default");

    if manager.get_session(session_id).is_none() {
        // Unknown ids are adopted, not rejected with 404: sessions live in memory, so
        // every daemon restart or prune would otherwise break all connected clients.
        if manager.active_session_count() > 10_000 {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(error_response(
                    None,
                    XAVIER_ERROR_INTERNAL,
                    "Server at capacity: too many active MCP sessions".to_string(),
                )),
            )
                .into_response();
        }
        manager.create_session(session_id, None, None);
    }

    if let Err(err) = manager.record_bytes(session_id, body.len()) {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(error_response(
                None,
                XAVIER_ERROR_VALIDATION,
                err.to_string(),
            )),
        )
            .into_response();
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error_response(
                    None,
                    -32700,
                    format!("Parse error: {error}"),
                )),
            )
                .into_response();
        }
    };

    if let Some((id, message)) = framing_error(&payload) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error_response(id, -32600, message)),
        )
            .into_response();
    }

    // Spec 2026-07-28: Request Metadata Validation
    if let Err(mismatch) = validate_request_headers(&headers, &payload) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error_response(None, ERROR_HEADER_MISMATCH, mismatch)),
        )
            .into_response();
    }

    let claims_ref = claims.as_ref().map(|c| &c.0);
    let _ = manager.transition_state(session_id, McpSessionState::Streaming);
    let write_key = write_caller.map(|Extension(key)| key.0).unwrap_or_else(|| {
        crate::adapters::inbound::http::middleware::rate_limit::memory_write_caller_key(
            None,
            None,
            claims_ref.map(|c| c.sub.as_str()),
            client.map(|extension| extension.0 .0),
        )
    });
    let caller = super::tools_memory::McpCaller {
        space: None,
        clearance: claims_ref
            .map(|c| crate::security::clearance::role_clearance(c.role))
            .unwrap_or(crate::security::clearance::ClearanceLevel::Unclassified),
    };
    let result = super::tools_memory::with_memory_write_caller_key(
        write_key,
        super::tools_memory::with_mcp_caller(
            caller,
            dispatch_mcp_value(state, workspace, claims_ref, payload),
        ),
    )
    .await;
    let _ = manager.transition_state(session_id, McpSessionState::Connected);

    let active_session_id = if session_id == "default" {
        let fresh = ulid::Ulid::new().to_string();
        // Register it so the client can send it back without a 404.
        manager.create_session(fresh.clone(), None, None);
        fresh
    } else {
        session_id.to_string()
    };

    match result {
        Ok(Some(response)) => (
            StatusCode::OK,
            [("mcp-session-id", active_session_id)],
            Json(response),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::ACCEPTED,
            [("mcp-session-id", active_session_id)],
        )
            .into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error).into_response(),
    }
}

/// Cheap JSON-RPC framing checks done before dispatch. Returns the id to echo
/// (when usable) and the message for a -32600 response.
fn framing_error(payload: &Value) -> Option<(Option<Value>, String)> {
    let object = match payload {
        Value::Array(_) => return Some((None, "Invalid Request: batch not supported".to_string())),
        Value::Object(object) => object,
        _ => return Some((None, "Invalid Request: expected a JSON object".to_string())),
    };
    let id = object
        .get("id")
        .filter(|v| v.is_string() || v.is_number())
        .cloned();
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some((id, "Invalid Request: jsonrpc must be \"2.0\"".to_string()));
    }
    let has_method = object.get("method").is_some_and(Value::is_string);
    let is_response = object.contains_key("result") || object.contains_key("error");
    if !has_method && !is_response {
        return Some((id, "Invalid Request: missing method".to_string()));
    }
    None
}

fn validate_request_headers(headers: &HeaderMap, body: &Value) -> Result<(), String> {
    // H-3 fix (spec 2026-07-28 best-effort): these metadata headers are OPTIONAL for
    // client compatibility. Standard MCP clients (opencode/claude/gemini/cursor) do not
    // send Mcp-Method/Mcp-Name/Mcp-Protocol-Version, so we only REJECT when a header is
    // present but mismatches the body (a genuine integrity violation). Missing headers
    // are accepted with a debug log. This unblocks remote MCP for third-party subagents.

    // 1. Protocol Version (optional; reject only if present and mismatched)
    if let Some(header_version) = headers
        .get("mcp-protocol-version")
        .and_then(|v| v.to_str().ok())
    {
        if header_version != MCP_PROTOCOL_VERSION {
            tracing::debug!(
                header = %header_version,
                expected = %MCP_PROTOCOL_VERSION,
                "MCP protocol version mismatch (non-fatal, accepting)"
            );
        }
    }

    // body method is mandatory regardless
    // Responses (no method) carry nothing to compare against the headers.
    let Some(body_method) = body.get("method").and_then(|v| v.as_str()) else {
        return Ok(());
    };

    // 2. Method header (optional; reject only if present and mismatched)
    if let Some(header_method) = headers.get("mcp-method").and_then(|v| v.to_str().ok()) {
        if header_method != body_method {
            return Err(format!(
                "Method mismatch: header='{header_method}', body='{body_method}'"
            ));
        }
    }

    // 3. Name header (optional; reject only if present and mismatched) for tools/call,
    // resources/read, prompts/get
    if ["tools/call", "resources/read", "prompts/get"].contains(&body_method) {
        let header_name = headers
            .get("mcp-name")
            .and_then(|v| v.to_str().ok())
            .map(decode_mcp_header_value);

        if let Some(header_name) = header_name {
            let body_name: Option<&str> = match body_method {
                "tools/call" => body
                    .get("params")
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str()),
                "resources/read" => body
                    .get("params")
                    .and_then(|p| p.get("uri"))
                    .and_then(|n| n.as_str()),
                "prompts/get" => body
                    .get("params")
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str()),
                _ => None,
            };
            if let Some(body_name) = body_name {
                if header_name != body_name {
                    return Err(format!(
                        "Name mismatch: header='{header_name}', body='{body_name}'"
                    ));
                }
            }
        }
    }

    Ok(())
}

fn decode_mcp_header_value(value: &str) -> String {
    if value.starts_with("=?base64?") && value.ends_with("?=") {
        let b64 = &value[9..value.len() - 2];
        if let Some(decoded) = crate::crypto::base64_decode(b64) {
            if let Ok(utf8) = String::from_utf8(decoded) {
                return utf8;
            }
        }
    }
    value.to_string()
}

/// Dispatch mcp value.
pub async fn dispatch_mcp_value(
    state: AppState,
    workspace: WorkspaceContext,
    claims: Option<&crate::security::auth::Claims>,
    payload: Value,
) -> Result<Option<Value>, String> {
    match payload {
        Value::Array(messages) => {
            if messages.is_empty() {
                return Err("Invalid JSON-RPC batch: empty batch".to_string());
            }
            let mut responses = Vec::new();
            for message in messages {
                if let Some(response) =
                    dispatch_mcp_message(state.clone(), workspace.clone(), claims, message).await?
                {
                    responses.push(serde_json::to_value(response).map_err(|e| e.to_string())?);
                }
            }
            if responses.is_empty() {
                Ok(None)
            } else {
                Ok(Some(Value::Array(responses)))
            }
        }
        message => dispatch_mcp_message(state, workspace, claims, message)
            .await?
            .map(|response| serde_json::to_value(response).map_err(|e| e.to_string()))
            .transpose(),
    }
}

async fn dispatch_mcp_message(
    state: AppState,
    workspace: WorkspaceContext,
    claims: Option<&crate::security::auth::Claims>,
    message: Value,
) -> Result<Option<MCPResponse>, String> {
    let Some(object) = message.as_object() else {
        return Ok(error_response(
            None,
            -32600,
            "Invalid Request: expected object or batch".to_string(),
        ));
    };
    let kind = match classify_message(object) {
        Ok(kind) => kind,
        Err(message) => return Ok(error_response(None, -32600, message)),
    };
    match kind {
        IncomingKind::Request => {
            let mut request: MCPRequest =
                match serde_json::from_value(Value::Object(object.clone())) {
                    Ok(request) => request,
                    Err(e) => {
                        return Ok(error_response(
                            None,
                            -32600,
                            format!("Invalid Request: {e}"),
                        ))
                    }
                };
            // `id: null` deserializes to None; it is a request, not a notification.
            if request.id.is_none() && object.contains_key("id") {
                request.id = Some(Value::Null);
            }
            handle_mcp_request(state, workspace, claims, request).await
        }
        IncomingKind::Response => Ok(None),
    }
}

enum IncomingKind {
    Request,
    Response,
}

fn classify_message(object: &serde_json::Map<String, Value>) -> Result<IncomingKind, String> {
    match object.get("jsonrpc").and_then(|value| value.as_str()) {
        Some("2.0") => {}
        _ => return Err("Invalid JSON-RPC message: jsonrpc must be \"2.0\"".to_string()),
    }
    if object.contains_key("method") {
        return Ok(IncomingKind::Request);
    }
    if object.contains_key("result") || object.contains_key("error") {
        return Ok(IncomingKind::Response);
    }
    Err("Invalid JSON-RPC message: missing method/result/error".to_string())
}

/// Failure of the generic argument validator. `Protocol` stays a JSON-RPC
/// error; `Tool` is surfaced as a tool result with `isError: true`.
enum ArgError {
    Protocol(MCPError),
    Tool(MCPError),
}

fn validate_tool_call_args(name: &str, arguments: &Value) -> Result<(), ArgError> {
    if !arguments.is_object() {
        return Err(ArgError::Protocol(MCPError {
            code: XAVIER_ERROR_VALIDATION,
            message: "arguments must be a JSON object".to_string(),
            data: None,
        }));
    }

    let tools = super::server::get_xavier_tools();
    let tool = tools.iter().find(|t| t.name == name);

    if let Some(tool) = tool {
        if let Some(required) = tool.input_schema.get("required").and_then(|r| r.as_array()) {
            for field in required {
                if let Some(field_name) = field.as_str() {
                    if arguments.get(field_name).is_none()
                        || arguments.get(field_name) == Some(&Value::Null)
                    {
                        return Err(ArgError::Tool(MCPError {
                            code: XAVIER_ERROR_VALIDATION,
                            message: format!("Missing required parameter: {}", field_name),
                            data: None,
                        }));
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_mcp_request(
    state: AppState,
    workspace: WorkspaceContext,
    claims: Option<&crate::security::auth::Claims>,
    request: MCPRequest,
) -> Result<Option<MCPResponse>, String> {
    let request_id = request.id.clone();
    let is_notification = request_id.is_none();
    if request.jsonrpc != "2.0" {
        return Ok(error_response(
            request_id,
            -32600,
            "Invalid Request".to_string(),
        ));
    }
    info!(method = %request.method, notification = is_notification, "mcp_request");

    let response = match request.method.as_str() {
        "initialize" => {
            let client_version = request
                .params
                .as_ref()
                .and_then(|p| p.get("protocolVersion"))
                .and_then(|v| v.as_str())
                .unwrap_or("2024-11-05");

            // Negociacion de version del handshake `initialize`: por esta via solo se
            // negocian revisiones "handshake". 2026-07-28 es una revision "moderna"
            // (envelope por request via `server/discover`) y devolverla aqui hace que un
            // cliente SDK la rechace con "Unsupported protocol version from the server".
            // Se hace eco de la revision del cliente cuando la conocemos y, si no, se
            // responde con la mas nueva negociable por handshake.
            const HANDSHAKE_KNOWN: [&str; 6] = [
                "2024-10-22",
                "2024-11-05",
                "2025-03-26",
                "2025-06-18",
                "2025-11-25",
                "2026-07-28",
            ];
            let response_version = if HANDSHAKE_KNOWN.contains(&client_version) {
                client_version
            } else {
                "2025-11-25"
            };

            Some(MCPResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.unwrap_or(Value::Null),
                result: Some(serde_json::json!({
                    "protocolVersion": response_version,
                    "capabilities": {
                        "tools": { "listChanged": false },
                        "resources": { "listChanged": false }
                    },
                    "serverInfo": { "name": "xavier-memory", "version": env!("CARGO_PKG_VERSION") }
                })),
                error: None,
            })
        }
        "notifications/initialized" => None,
        "ping" => Some(MCPResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.unwrap_or(Value::Null),
            result: Some(serde_json::json!({})),
            error: None,
        }),
        "resources/list" => Some(MCPResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.unwrap_or(Value::Null),
            result: Some(serde_json::json!({ "resources": super::server::get_xavier_resources() })),
            error: None,
        }),
        "resources/read" => {
            let params = request
                .params
                .clone()
                .unwrap_or_else(|| serde_json::json!({}));
            let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            match read_xavier_resource(&workspace, uri).await {
                Ok(Some((mime_type, text))) => Some(MCPResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request_id.clone().unwrap_or(Value::Null),
                    result: Some(serde_json::json!({
                        "contents": [{ "uri": uri, "mimeType": mime_type, "text": text }]
                    })),
                    error: None,
                }),
                Ok(None) => error_response(
                    request_id.clone(),
                    -32602,
                    format!("Unknown resource: {uri}"),
                ),
                Err(error) => error_response(request_id.clone(), XAVIER_ERROR_INTERNAL, error),
            }
        }
        "health/check" => {
            let health = crate::health::collect_health_sync();
            Some(MCPResponse {
                jsonrpc: "2.0".to_string(),
                id: request_id.clone().unwrap_or(Value::Null),
                result: Some(
                    serde_json::to_value(&health).unwrap_or_else(|_| serde_json::json!({})),
                ),
                error: None,
            })
        }
        "tools/list" => Some(MCPResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.unwrap_or(Value::Null),
            result: Some(serde_json::json!({ "tools": super::server::get_xavier_tools() })),
            error: None,
        }),
        "server/discover" => {
            let tools = super::server::get_xavier_tools();
            let resources = super::server::get_xavier_resources();
            Some(MCPResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.unwrap_or(Value::Null),
                result: Some(serde_json::json!({
                    "serverInfo": {
                        "name": "xavier-memory",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "protocolVersion": "2026-07-28",
                    // Requerido por el DiscoverResult del spec 2026-07-28: un cliente SDK
                    // valida el cuerpo y rechaza la respuesta si falta esta lista
                    // ("1 validation error for DiscoverResult ... supportedVersions").
                    // Va primero la revision moderna y despues las negociables por handshake.
                    "supportedVersions": [
                        "2026-07-28",
                        "2025-11-25",
                        "2025-06-18",
                        "2025-03-26",
                        "2024-11-05",
                    ],
                    "capabilities": {
                        "tools": {
                            "count": tools.len(),
                            "list": tools
                        },
                        "resources": {
                            "count": resources.len(),
                            "list": resources
                        },
                        "prompts": {
                            "count": 0,
                            "list": []
                        }
                    }
                })),
                error: None,
            })
        }
        "tools/call" => {
            let params = match request.params {
                Some(params) if params.is_object() => params,
                _ => {
                    return Ok(error_response(
                        request_id,
                        -32602,
                        "Invalid params: params must be an object".to_string(),
                    ))
                }
            };
            let name = match params.get("name").and_then(|v| v.as_str()) {
                Some(name) if !name.is_empty() => name,
                _ => {
                    return Ok(error_response(
                        request_id,
                        -32602,
                        "Invalid params: missing or empty tool name".to_string(),
                    ))
                }
            };
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let id = request_id.clone().unwrap_or(Value::Null);

            match validate_tool_call_args(name, &arguments) {
                Err(ArgError::Protocol(error)) => Some(MCPResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(error),
                }),
                Err(ArgError::Tool(error)) => Some(tool_error_response(id, error)),
                Ok(()) => Some(
                    match super::server::handle_tool_call(state, workspace, claims, name, arguments)
                        .await
                    {
                        Ok(result) => MCPResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            result: Some(result),
                            error: None,
                        },
                        Err(error) => {
                            let error = classify_mcp_error(error);
                            if error.code == XAVIER_ERROR_SECURITY
                                && (error.message.contains("Security policy violation")
                                    || error.message.contains("blocked by security policy"))
                            {
                                tool_error_response(id, error)
                            } else {
                                MCPResponse {
                                    jsonrpc: "2.0".to_string(),
                                    id,
                                    result: None,
                                    error: Some(error),
                                }
                            }
                        }
                    },
                ),
            }
        }
        _ if is_notification => None,
        _ => error_response(
            request.id,
            -32601,
            format!("Method not found: {}", request.method),
        ),
    };
    if is_notification {
        Ok(None)
    } else {
        Ok(response)
    }
}

/// Tool execution failure: a normal result with `isError: true` that keeps the
/// JSON-RPC style error object under `structuredContent`.
fn tool_error_response(id: Value, error: MCPError) -> MCPResponse {
    let result = serde_json::to_value(MCPToolResult {
        content: vec![MCPContent::Text(MCPTextContent {
            content_type: "text".to_string(),
            text: error.message.clone(),
        })],
        structured_content: Some(serde_json::json!({
            "error": { "code": error.code, "message": error.message }
        })),
        is_error: Some(true),
    })
    .unwrap_or_else(|_| serde_json::json!({ "isError": true }));
    MCPResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(result),
        error: None,
    }
}

fn error_response(id: Option<Value>, code: i32, message: String) -> Option<MCPResponse> {
    Some(MCPResponse {
        jsonrpc: "2.0".to_string(),
        id: id.unwrap_or(Value::Null),
        result: None,
        error: Some(MCPError {
            code,
            message,
            data: None,
        }),
    })
}

fn classify_mcp_error(err: anyhow::Error) -> MCPError {
    let mut message = err.to_string();
    let code = if let Some(tool) = message.strip_prefix("Tool not implemented: ") {
        message = format!("Unknown tool: {tool}");
        -32602
    } else if message.contains("Memory clearance exceeds caller clearance")
        || message.contains("Security policy violation")
        || message.contains("blocked by security policy")
        || message.contains("Forbidden")
        || message.contains("Insufficient permissions")
    {
        XAVIER_ERROR_SECURITY
    } else if message.contains("Memory write rate limit exceeded") {
        // types.rs has no rate-limit code. The message stays; the code stays internal.
        XAVIER_ERROR_INTERNAL
    } else if message.contains("Missing")
        || message.contains("must be")
        || message.contains("Invalid")
    {
        XAVIER_ERROR_VALIDATION
    } else if message.contains("not found") || message.contains("Memory not found") {
        XAVIER_ERROR_NOT_FOUND
    } else {
        XAVIER_ERROR_INTERNAL
    };

    MCPError {
        code,
        message,
        data: None,
    }
}

/// Read a named Xavier MCP resource by URI.
///
/// Returns `Ok(Some((mime_type, text)))` when the URI is known, `Ok(None)` when
/// it is not recognized (so the caller can emit a `-32602` params error), and
/// `Err` on a genuine failure reading the resource.
async fn read_xavier_resource(
    workspace: &crate::workspace::WorkspaceContext,
    uri: &str,
) -> Result<Option<(String, String)>, String> {
    match uri {
        "xavier://memory" => {
            let records = workspace
                .workspace
                .list_memory_records()
                .await
                .map_err(|e| e.to_string())?;
            let text = serde_json::to_string_pretty(&records).map_err(|e| e.to_string())?;
            Ok(Some(("application/json".to_string(), text)))
        }
        "xavier://projects" => {
            let records = workspace
                .workspace
                .list_memory_records()
                .await
                .map_err(|e| e.to_string())?;
            let mut projects = std::collections::BTreeMap::<String, usize>::new();
            for record in records {
                if let Some(project) = record
                    .metadata
                    .get("namespace")
                    .and_then(|n| n.get("project"))
                    .and_then(|p| p.as_str())
                {
                    *projects.entry(project.to_string()).or_insert(0) += 1;
                }
            }
            let text = serde_json::to_string_pretty(&projects).map_err(|e| e.to_string())?;
            Ok(Some(("application/json".to_string(), text)))
        }
        "xavier://health" => {
            let health = crate::health::collect_health_sync();
            let text = serde_json::to_string_pretty(&health).map_err(|e| e.to_string())?;
            Ok(Some(("application/json".to_string(), text)))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::mcp::tests::{post_json, test_router, test_state};
    use axum::http::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn header_less_clients_are_not_blocked_after_4mb() {
        xavier::isolate_test_process!();

        let (state, workspace) = test_state().await;
        let app = test_router(state, workspace);

        // 1MB payload (simulate 5 header-less requests with valid JSON)
        let valid_1mb_json = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {
                "padding": "a".repeat(1024 * 1024)
            }
        });

        for _ in 0..5 {
            let res = post_json(app.clone(), valid_1mb_json.clone()).await;
            assert_ne!(res.status(), StatusCode::FORBIDDEN);
            assert_ne!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
            assert_eq!(res.status(), StatusCode::OK);
        }

        // 5MB payload (exceeds 4MB limit, should be rejected)
        let valid_5mb_json = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {
                "padding": "a".repeat(5 * 1024 * 1024)
            }
        });
        let res = post_json(app.clone(), valid_5mb_json).await;
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
