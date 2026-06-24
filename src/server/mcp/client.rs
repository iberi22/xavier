//! MCP (Model Context Protocol) Client implementation.
//!
//! Provides a client to interact with external MCP servers via stdio transport.
//! Supports tool listing and tool calling as per the MCP specification.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use std::sync::Arc;
use crate::server::mcp::types::{MCPRequest, MCPResponse};

/// A client for an external MCP server running as a subprocess.
pub struct McpClient {
    child: Arc<Mutex<Child>>,
    next_id: Arc<Mutex<u64>>,
}

impl McpClient {
    /// Start a new MCP client by spawning an external process.
    pub fn start(command: &str, args: &[String]) -> Result<Self> {
        let child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("Failed to spawn MCP server: {} {:?}", command, args))?;

        Ok(Self {
            child: Arc::new(Mutex::new(child)),
            next_id: Arc::new(Mutex::new(1)),
        })
    }

    /// Call a tool on the external MCP server.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        let id = {
            let mut id_lock = self.next_id.lock().await;
            let id = *id_lock;
            *id_lock += 1;
            id
        };

        let request = MCPRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(id)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": name,
                "arguments": arguments,
            })),
        };

        let response = self.send_request(request).await?;

        if let Some(error) = response.error {
            anyhow::bail!("MCP error {}: {}", error.code, error.message);
        }

        response.result.context("MCP response missing result")
    }

    /// List available tools on the external MCP server.
    pub async fn list_tools(&self) -> Result<Value> {
        let id = {
            let mut id_lock = self.next_id.lock().await;
            let id = *id_lock;
            *id_lock += 1;
            id
        };

        let request = MCPRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(id)),
            method: "tools/list".to_string(),
            params: None,
        };

        let response = self.send_request(request).await?;

        if let Some(error) = response.error {
            anyhow::bail!("MCP error {}: {}", error.code, error.message);
        }

        response.result.context("MCP response missing result")
    }

    async fn send_request(&self, request: MCPRequest) -> Result<MCPResponse> {
        let mut child = self.child.lock().await;
        let stdin = child.stdin.as_mut().context("Failed to open stdin")?;
        let stdout = child.stdout.as_mut().context("Failed to open stdout")?;
        let mut reader = BufReader::new(stdout);

        let request_json = serde_json::to_string(&request)?;
        stdin.write_all(request_json.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;

        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let response: MCPResponse = serde_json::from_str(&line)
            .with_context(|| format!("Failed to parse MCP response: {}", line))?;

        Ok(response)
    }
}
