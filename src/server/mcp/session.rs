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
use serde_json::Value;
use tracing::info;

pub const MCP_PROTOCOL_VERSION: &str = "2026-07-28";
pub const ERROR_HEADER_MISMATCH: i32 = -32020;

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

    // Spec 2026-07-28: Request Metadata Validation
    if let Err(mismatch) = validate_request_headers(&headers, &payload) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error_response(None, ERROR_HEADER_MISMATCH, mismatch)),
        )
            .into_response();
    }

    match dispatch_mcp_value(state, workspace, payload).await {
        Ok(Some(response)) => (StatusCode::OK, Json(response)).into_response(),
        Ok(None) => StatusCode::ACCEPTED.into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error).into_response(),
    }
}

fn validate_request_headers(headers: &HeaderMap, body: &Value) -> Result<(), String> {
    // 1. Protocol Version
    let header_version = headers
        .get("mcp-protocol-version")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| "Missing MCP-Protocol-Version header".to_string())?;

    if header_version != MCP_PROTOCOL_VERSION {
        return Err(format!(
            "Protocol version mismatch: header='{header_version}', expected='{MCP_PROTOCOL_VERSION}'"
        ));
    }

    // 2. Method
    let header_method = headers
        .get("mcp-method")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| "Missing Mcp-Method header".to_string())?;

    let body_method = body
        .get("method")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing 'method' in request body".to_string())?;

    if header_method != body_method {
        return Err(format!(
            "Method mismatch: header='{header_method}', body='{body_method}'"
        ));
    }

    // 3. Name (required for tools/call, resources/read, prompts/get)
    if ["tools/call", "resources/read", "prompts/get"].contains(&body_method) {
        let header_name = headers
            .get("mcp-name")
            .and_then(|v| v.to_str().ok())
            .map(decode_mcp_header_value)
            .ok_or_else(|| "Missing Mcp-Name header".to_string())?;

        let body_name = match body_method {
            "tools/call" => body.get("params").and_then(|p| p.get("name")).and_then(|n| n.as_str()),
            "resources/read" => body.get("params").and_then(|p| p.get("uri")).and_then(|n| n.as_str()),
            "prompts/get" => body.get("params").and_then(|p| p.get("name")).and_then(|n| n.as_str()),
            _ => None,
        }.ok_or_else(|| "Missing name/uri in request params".to_string())?;

        if header_name != body_name {
            return Err(format!(
                "Name mismatch: header='{header_name}', body='{body_name}'"
            ));
        }
    }

    Ok(())
}

fn decode_mcp_header_value(value: &str) -> String {
    if value.starts_with("=?base64?") && value.ends_with("?=") {
        let b64 = &value[9..value.len() - 2];
        if let Ok(decoded) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64) {
            if let Ok(utf8) = String::from_utf8(decoded) {
                return utf8;
            }
        }
    }
    value.to_string()
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

fn validate_tool_call_args(name: &str, arguments: &Value) -> Result<(), MCPError> {
    let tools = super::server::get_xavier_tools();
    let tool = tools.iter().find(|t| t.name == name);

    if let Some(tool) = tool {
        if let Some(required) = tool.input_schema.get("required").and_then(|r| r.as_array()) {
            for field in required {
                if let Some(field_name) = field.as_str() {
                    if arguments.get(field_name).is_none()
                        || arguments.get(field_name) == Some(&Value::Null)
                    {
                        return Err(MCPError {
                            code: XAVIER_ERROR_VALIDATION,
                            message: format!("Missing required parameter: {}", field_name),
                            data: None,
                        });
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
                serde_json::json!({
                    "protocolVersion": "2026-07-28",
                    "capabilities": {
                        "tools": { "listChanged": false },
                        "resources": { "listChanged": false }
                    },
                    "serverInfo": { "name": "xavier-memory", "version": env!("CARGO_PKG_VERSION") }
                }),
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
        "resources/read" => {
            let params = request.params.clone().unwrap_or_else(|| serde_json::json!({}));
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
                Err(error) => error_response(
                    request_id.clone(),
                    XAVIER_ERROR_INTERNAL,
                    error,
                ),
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
        "tools/call" => {
            let params = request.params.unwrap_or(serde_json::json!({}));
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));

            if let Err(error) = validate_tool_call_args(name, &arguments) {
                Some(MCPResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.unwrap_or(Value::Null),
                    result: None,
                    error: Some(error),
                })
            } else {
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
