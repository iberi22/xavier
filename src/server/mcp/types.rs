//! Type definitions for the Model Context Protocol (MCP).
//!
//! This module implements the core data structures for MCP communication,
//! including JSON-RPC request/response envelopes, tool definitions, and
//! resource schemas.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const XAVIER_ERROR_SECURITY: i32 = -32000;
pub const XAVIER_ERROR_VALIDATION: i32 = -32001;
pub const XAVIER_ERROR_NOT_FOUND: i32 = -32002;
pub const XAVIER_ERROR_INTERNAL: i32 = -32603;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MCPRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MCPResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<MCPError>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MCPError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MCPTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MCPResource {
    pub uri: String,
    pub name: String,
    pub mime_type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MCPToolResult {
    pub content: Vec<MCPContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MCPContent {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MCPSearchResult {
    pub id: String,
    pub path: String,
    pub score: f64,
    pub snippet: String,
    pub provenance: MCPProvenance,
    pub metadata: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MCPProvenance {
    pub source: String,
    pub retrieved_at: String,
    pub retrieval_method: String,
    pub embedding_model: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MCPContextResult {
    pub total_chars: usize,
    pub total_records: usize,
    pub truncated: bool,
    pub truncated_reason: Option<String>,
    pub content: String,
    pub sources: Vec<MCPSearchResult>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MCPHealthResult {
    pub status: String,
    pub tools_count: usize,
    pub handshake_ok: bool,
    pub memory_store_ok: bool,
    pub embedding_ok: bool,
    pub mcp_protocol: String,
}
