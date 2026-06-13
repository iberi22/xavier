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
    http::{header::HeaderName, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde_json::Value;
use tracing::info;
use ulid::Ulid;

const MCP_SESSION_HEADER: &str = "mcp-session-id";

pub async fn mcp_post_handler(
    State(state): State<AppState>,
    Extension(workspace): Extension<WorkspaceContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Invalid JSON payload: {error}"),
            )
                .into_response();
        }
    };

    let session_header = match resolve_mcp_session_header(&headers, &payload) {
        Ok(value) => value,
        Err(error) => return (StatusCode::BAD_REQUEST, error).into_response(),
    };

    match dispatch_mcp_value(state, workspace, payload).await {
        Ok(Some(response)) => with_session_header(
            (StatusCode::OK, Json(response)).into_response(),
            session_header,
        ),
        Ok(None) => with_session_header(StatusCode::ACCEPTED.into_response(), session_header),
        Err(error) => (StatusCode::BAD_REQUEST, error).into_response(),
    }
}

pub async fn mcp_get_handler() -> impl IntoResponse {
    StatusCode::METHOD_NOT_ALLOWED
}
pub async fn mcp_delete_handler() -> impl IntoResponse {
    StatusCode::METHOD_NOT_ALLOWED
}

fn resolve_mcp_session_header(
    headers: &HeaderMap,
    payload: &Value,
) -> Result<Option<HeaderValue>, String> {
    if let Some(value) = headers.get(MCP_SESSION_HEADER) {
        if value.as_bytes().is_empty() {
            return Err("Mcp-Session-Id header must not be empty".to_string());
        }
        return Ok(Some(value.clone()));
    }

    if payload_method(payload).is_some_and(|method| method == "initialize") {
        let session_id = format!("xavier-{}", Ulid::new());
        let value = HeaderValue::from_str(&session_id)
            .map_err(|_| "Failed to generate MCP session header".to_string())?;
        return Ok(Some(value));
    }
    Ok(None)
}

fn payload_method(payload: &Value) -> Option<&str> {
    match payload {
        Value::Object(map) => map.get("method").and_then(|value| value.as_str()),
        Value::Array(items) => items.iter().find_map(payload_method),
        _ => None,
    }
}

fn with_session_header(mut response: Response, session_header: Option<HeaderValue>) -> Response {
    if let Some(value) = session_header {
        response
            .headers_mut()
            .insert(HeaderName::from_static(MCP_SESSION_HEADER), value);
    }
    response
}

pub async fn dispatch_mcp_value(
    state: AppState,
    workspace: WorkspaceContext,
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
                    dispatch_mcp_message(state.clone(), workspace.clone(), message).await?
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
        message => dispatch_mcp_message(state, workspace, message)
            .await?
            .map(|response| serde_json::to_value(response).map_err(|e| e.to_string()))
            .transpose(),
    }
}

async fn dispatch_mcp_message(
    state: AppState,
    workspace: WorkspaceContext,
    message: Value,
) -> Result<Option<MCPResponse>, String> {
    let object = message
        .as_object()
        .ok_or_else(|| "Invalid JSON-RPC message: expected object or batch".to_string())?;
    match classify_message(object)? {
        IncomingKind::Request => {
            let request: MCPRequest =
                serde_json::from_value(Value::Object(object.clone())).map_err(|e| e.to_string())?;
            handle_mcp_request(state, workspace, request).await
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

async fn handle_mcp_request(
    state: AppState,
    workspace: WorkspaceContext,
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
        "initialize" => Some(MCPResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.unwrap_or(Value::Null),
            result: Some(
                serde_json::json!({ "protocolVersion": "2025-03-26", "capabilities": { "tools": {} }, "serverInfo": { "name": "xavier-memory", "version": env!("CARGO_PKG_VERSION") } }),
            ),
            error: None,
        }),
        "notifications/initialized" => None,
        "resources/list" => Some(MCPResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.unwrap_or(Value::Null),
            result: Some(serde_json::json!({ "resources": super::server::get_xavier_resources() })),
            error: None,
        }),
        "tools/list" => Some(MCPResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.unwrap_or(Value::Null),
            result: Some(serde_json::json!({ "tools": super::server::get_xavier_tools() })),
            error: None,
        }),
        "tools/call" => {
            let params = request.params.unwrap_or(serde_json::json!({}));
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            Some(
                match super::server::handle_tool_call(state, workspace, name, arguments).await {
                    Ok(result) => MCPResponse {
                        jsonrpc: "2.0".to_string(),
                        id: request.id.unwrap_or(Value::Null),
                        result: Some(result),
                        error: None,
                    },
                    Err(error) => MCPResponse {
                        jsonrpc: "2.0".to_string(),
                        id: request.id.unwrap_or(Value::Null),
                        result: None,
                        error: Some(classify_mcp_error(error)),
                    },
                },
            )
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
    let message = err.to_string();
    let code = if message.contains("Security policy violation")
        || message.contains("blocked by security policy")
    {
        XAVIER_ERROR_SECURITY
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
