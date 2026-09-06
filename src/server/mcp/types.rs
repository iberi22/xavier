//! Type definitions for the Model Context Protocol (MCP).
//!
//! This module implements the core data structures for MCP communication,
//! including JSON-RPC request/response envelopes, tool definitions, and
//! resource schemas.  MCP 2025-2026 best practices: structuredContent,
//! data provenance (W3C PROV), bounded context/output.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const XAVIER_ERROR_SECURITY: i32 = -32000;
pub const XAVIER_ERROR_VALIDATION: i32 = -32001;
pub const XAVIER_ERROR_NOT_FOUND: i32 = -32002;
pub const XAVIER_ERROR_INTERNAL: i32 = -32603;

// ── JSON-RPC envelopes ──────────────────────────────────────────────

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

// ── Tool / Resource definitions ─────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MCPTool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MCPResource {
    pub uri: String,
    pub name: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

// ── Content types (MCP 2025-2026 structuredContent support) ─────────

/// Unified MCP content variant — supports legacy text, structured JSON,
/// and resource URIs.
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MCPContent {
    Text(MCPTextContent),
    Structured(MCPStructuredContent),
    Resource(MCPResourceContent),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MCPTextContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

/// Structured content with an explicit output_schema (MCP spec 2025-06-18).
/// The `structuredContent` field carries the actual typed payload.
#[derive(Debug, Serialize, Deserialize)]
pub struct MCPStructuredContent {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(rename = "structuredContent")]
    pub structured_content: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MCPResourceContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub resource: MCPResourceRef,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MCPResourceRef {
    pub uri: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub text: Option<String>,
}

// ── Tool result envelope ────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct MCPToolResult {
    pub content: Vec<MCPContent>,
    #[serde(rename = "isError")]
    pub is_error: Option<bool>,
}

// ── Structured search result types ──────────────────────────────────

/// A single search result with full provenance and metadata.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MCPSearchResult {
    pub id: String,
    pub path: String,
    pub score: f64,
    pub snippet: String,
    pub provenance: MCPProvenance,
    pub metadata: Value,
}

/// Data provenance for each search result (W3C PROV-style).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MCPProvenance {
    pub source: String,
    #[serde(rename = "retrievedAt")]
    pub retrieved_at: String,
    #[serde(rename = "retrievalMethod")]
    pub retrieval_method: String,
    #[serde(rename = "embeddingModel")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Packaged context result with truncation awareness.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MCPContextResult {
    #[serde(rename = "totalChars")]
    pub total_chars: usize,
    #[serde(rename = "totalRecords")]
    pub total_records: usize,
    pub truncated: bool,
    #[serde(rename = "truncatedReason")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated_reason: Option<String>,
    pub content: String,
    pub sources: Vec<MCPSearchResult>,
    #[serde(rename = "estimatedTokens")]
    pub estimated_tokens: usize,
}

/// Structured health result for the health_check tool.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MCPHealthResult {
    pub status: String,
    #[serde(rename = "toolsCount")]
    pub tools_count: usize,
    #[serde(rename = "handshakeOk")]
    pub handshake_ok: bool,
    #[serde(rename = "memoryStoreOk")]
    pub memory_store_ok: bool,
    #[serde(rename = "embeddingOk")]
    pub embedding_ok: bool,
    #[serde(rename = "mcpProtocol")]
    pub mcp_protocol: String,
}

// ── Helper constructors ─────────────────────────────────────────────

impl MCPToolResult {
    /// Text.
    pub fn text(text: impl Into<String>, is_error: bool) -> Self {
        MCPToolResult {
            content: vec![MCPContent::Text(MCPTextContent {
                content_type: "text".to_string(),
                text: text.into(),
            })],
            is_error: Some(is_error),
        }
    }

    /// Structured.
    pub fn structured(payload: Value, is_error: bool) -> Self {
        let text_fallback = payload.to_string();
        MCPToolResult {
            content: vec![
                MCPContent::Text(MCPTextContent {
                    content_type: "text".to_string(),
                    text: text_fallback,
                }),
                MCPContent::Structured(MCPStructuredContent {
                    content_type: "structuredContent".to_string(),
                    structured_content: payload,
                }),
            ],
            is_error: Some(is_error),
        }
    }
}
