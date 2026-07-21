// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! MCP server core implementation
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use super::types::*;
use crate::ports::inbound::SecurityScanPort;
use crate::AppState;
use serde_json::Value;

pub fn get_xavier_tools() -> Vec<MCPTool> {
    let mut tools = super::tools_core::get_xavier_core_tools();
    tools.extend(super::tools_memory::get_xavier_memory_tools());
    tools.extend(super::tools_context::get_xavier_context_tools());
    tools
}

pub fn get_xavier_resources() -> Vec<MCPResource> {
    vec![
        MCPResource {
            uri: "xavier://memory".to_string(),
            name: "Memory Store".to_string(),
            mime_type: "application/json".to_string(),
        },
        MCPResource {
            uri: "xavier://projects".to_string(),
            name: "Projects List".to_string(),
            mime_type: "application/json".to_string(),
        },
        MCPResource {
            uri: "xavier://health".to_string(),
            name: "System Health".to_string(),
            mime_type: "application/json".to_string(),
        },
    ]
}

pub async fn handle_tool_call(
    state: AppState,
    workspace: crate::workspace::WorkspaceContext,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    for (key, value) in arguments.as_object().unwrap_or(&serde_json::Map::new()) {
        if !should_prescan_tool_argument(name, key) {
            continue;
        }
        if let Some(text) = value.as_str() {
            let scan_result = state.security_service.scan(text, None).await?;
            if !scan_result.threats.is_empty() {
                return Err(anyhow::anyhow!(
                    "Security policy violation detected in argument '{}': {}",
                    key,
                    scan_result.threats[0].description
                ));
            }
        }
    }

    if super::tools_core::is_core_tool(name) {
        super::tools_core::handle_core_tool(state, workspace, name, arguments).await
    } else if name.starts_with("xavier_context") || name == "xavier_token_savings" {
        super::tools_context::handle_context_tool(state, workspace, name, arguments).await
    } else {
        super::tools_memory::handle_memory_tool(state, workspace, name, arguments).await
    }
}

fn should_prescan_tool_argument(tool_name: &str, argument_name: &str) -> bool {
    let _ = tool_name;
    argument_name != "id"
}

pub fn mcp_text_result(text: impl Into<String>, is_error: bool) -> anyhow::Result<Value> {
    Ok(serde_json::to_value(MCPToolResult {
        content: vec![MCPContent::Text(MCPTextContent {
            content_type: "text".to_string(),
            text: text.into(),
        })],
        is_error: Some(is_error),
    })?)
}
