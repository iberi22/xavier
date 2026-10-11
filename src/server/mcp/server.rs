//! MCP server core implementation
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use super::types::*;
use crate::ports::inbound::SecurityScanPort;
use crate::AppState;
use serde_json::Value;

tokio::task_local! {
    /// True only inside a call made by the node's ROOT credential. Admin
    /// claims alone (e.g. an Admin JWT) never set it.
    static ROOT_CREDENTIAL: bool;
}

/// Run `fut` with the root-credential marker set to `is_root`. The transports
/// set it from the `RootCredential` request extension, which only the root
/// token branch of the auth middleware inserts.
pub async fn with_root_credential<F: std::future::Future>(is_root: bool, fut: F) -> F::Output {
    ROOT_CREDENTIAL.scope(is_root, fut).await
}

/// Whether the current tool call carries the root credential.
pub(crate) fn root_credential_present() -> bool {
    ROOT_CREDENTIAL.try_with(|v| *v).unwrap_or(false)
}

/// Get xavier tools.
pub fn get_xavier_tools() -> Vec<MCPTool> {
    let mut tools = super::tools_core::get_xavier_core_tools();
    tools.extend(super::tools_secrets::get_xavier_secrets_tools());
    tools.extend(super::tools_memory::get_xavier_memory_tools());
    tools.extend(super::tools_context::get_xavier_context_tools());
    tools.extend(super::telecom_tools::register_telecom_mcp_tools());
    tools.extend(super::tools_expert::get_expert_tools());
    #[cfg(feature = "pageindex")]
    tools.extend(super::tools_pageindex::get_pageindex_tools());
    tools
}

/// Get xavier resources.
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

/// Handle tool call.
pub async fn handle_tool_call(
    state: AppState,
    workspace: crate::workspace::WorkspaceContext,
    claims: Option<&crate::security::auth::Claims>,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    if claims.is_none() && name != "health_check" {
        return Err(anyhow::anyhow!(
            "Forbidden: Insufficient permissions for anonymous role to execute tool '{name}'"
        ));
    }

    if let Some(claims) = claims {
        if claims.sub == crate::server::mcp_stdio::STDIO_LOCAL_SUBJECT
            && matches!(name, "secret_lend" | "secret_exec")
        {
            return Err(anyhow::anyhow!(
                "Forbidden: Insufficient permissions for local identity to execute tool '{name}'"
            ));
        }
        let role = &claims.role;
        use crate::security::auth::Permission;
        match name {
            // Addition / Creation tools
            "create_memory"
            | "save_fragment"
            | "memoryfragment_save"
            | "memory_save"
            | "ticket_create"
            | "sync_gitcore"
                if !role.can_add_memory() =>
            {
                return Err(anyhow::anyhow!(
                    "Forbidden: Insufficient permissions for role {:?} to execute tool '{}'",
                    role,
                    name
                ));
            }
            // Deletion / Pruning tools
            "memoryfragment_delete" | "memory_prune" if !role.can_delete_memory() => {
                return Err(anyhow::anyhow!(
                    "Forbidden: Insufficient permissions for role {:?} to execute tool '{}'",
                    role,
                    name
                ));
            }
            // Shell execution (arbitrary `sh -c` + caller-controlled cwd)
            // and secret access: admin-only. can_edit_config() is Admin-only
            // in the Permission trait; lesser JWT roles must never reach them.
            "xavier_run_command" | "secret_lend" | "secret_exec" if !role.can_edit_config() => {
                return Err(anyhow::anyhow!(
                    "Forbidden: Insufficient permissions for role {:?} to execute tool '{}'",
                    role,
                    name
                ));
            }
            _ => {}
        }
    }

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

    #[cfg(feature = "pageindex")]
    if super::tools_pageindex::is_pageindex_tool(name) {
        return super::tools_pageindex::handle_pageindex_tool(
            &workspace,
            claims.map(|c| &c.role),
            name,
            arguments,
        )
        .await;
    }

    if super::tools_core::is_core_tool(name) {
        super::tools_core::handle_core_tool(state, workspace, claims, name, arguments).await
    } else if super::tools_secrets::is_secrets_tool(name) {
        let ctx = super::tools_secrets::SecretToolContext::production();
        super::tools_secrets::handle_secrets_tool(&ctx, name, arguments).await
    } else if name.starts_with("xavier_context")
        || name == "xavier_token_savings"
        || name == "xavier_issue_context_package"
        || name == "xavier_run_command"
        || name == "xavier_dispatch_skill"
        || name == "xavier_skill_list"
    {
        super::tools_context::handle_context_tool(state, workspace, name, arguments).await
    } else if super::tools_expert::is_expert_tool(name) {
        super::tools_expert::handle_expert_tool(name, arguments).await
    } else if name.starts_with("telecom_") {
        super::telecom_tools::handle_telecom_tool(name, arguments, None).await
    } else {
        super::tools_memory::handle_memory_tool(state, workspace, name, arguments).await
    }
}

fn should_prescan_tool_argument(tool_name: &str, argument_name: &str) -> bool {
    let _ = tool_name;
    argument_name != "id"
}

/// Mcp text result.
pub fn mcp_text_result(text: impl Into<String>, is_error: bool) -> anyhow::Result<Value> {
    Ok(serde_json::to_value(MCPToolResult {
        content: vec![MCPContent::Text(MCPTextContent {
            content_type: "text".to_string(),
            text: text.into(),
        })],
        structured_content: None,
        is_error: Some(is_error),
    })?)
}
