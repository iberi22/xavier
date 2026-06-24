//! # CodeGraph Plugin — MCP-First Code Analysis
//!
//! Integrates the external `code-graph` MCP server to provide symbol discovery,
//! dependency analysis, and codebase understanding.

use async_trait::async_trait;
use crate::adapters::inbound::http::plugins::{Plugin, SyncDirection, SyncResult};
use crate::server::mcp::client::McpClient;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct CodeGraphPlugin {
    name: String,
    command: String,
    args: Vec<String>,
    client: Arc<Mutex<Option<McpClient>>>,
}

impl CodeGraphPlugin {
    pub fn new(name: &str, command: &str, args: &[String]) -> Self {
        Self {
            name: name.to_string(),
            command: command.to_string(),
            args: args.to_vec(),
            client: Arc::new(Mutex::new(None)),
        }
    }

    async fn ensure_client(&self) -> Result<Arc<Mutex<Option<McpClient>>>, String> {
        let mut client_lock = self.client.lock().await;
        if client_lock.is_none() {
            let client = McpClient::start(&self.command, &self.args)
                .map_err(|e| format!("Failed to start CodeGraph MCP client: {}", e))?;
            *client_lock = Some(client);
        }
        Ok(Arc::clone(&self.client))
    }

    /// Find symbols via the MCP tool.
    pub async fn find_symbols(&self, query: &str) -> Result<serde_json::Value, String> {
        let client_arc = self.ensure_client().await?;
        let client_lock = client_arc.lock().await;
        let client = client_lock.as_ref().unwrap();

        client.call_tool("find", json!({ "query": query }))
            .await
            .map_err(|e| format!("MCP tool call failed: {}", e))
    }
}

#[async_trait]
impl Plugin for CodeGraphPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    async fn health_check(&self) -> Result<(), String> {
        let client_arc = self.ensure_client().await?;
        let client_lock = client_arc.lock().await;
        let client = client_lock.as_ref().unwrap();

        match client.list_tools().await {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("MCP health check failed: {}", e)),
        }
    }

    async fn sync(&self, _direction: SyncDirection) -> Result<SyncResult, String> {
        // CodeGraph is a query-only plugin for now
        Ok(SyncResult::success(0))
    }

    fn version(&self) -> &str {
        "1.0.0"
    }
}
