//! Type definitions for the Model Context Protocol (MCP).
//!
//! This module implements the core data structures for MCP communication,
//! including JSON-RPC request/response envelopes, tool definitions, and
//! resource schemas.

use serde::{Deserialize, Serialize};
use serde_json::Value;

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

#[derive(Debug, Serialize, Deserialize)]
pub struct MCPToolResult {
    pub content: Vec<MCPTextContent>,
    pub is_error: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MCPTextContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}
