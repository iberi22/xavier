//! MCP adapter for the PageIndex tools. All tool logic, envelopes and schemas
//! live in `pageindex_glue::tools`; this module only maps them onto MCP types
//! and derives write access from the caller's role.

use std::sync::Arc;

use serde_json::Value;

use super::types::*;
use crate::pageindex_glue::{self, PageIndexSettings, PageIndexState, ToolContext};
use crate::security::auth::{Permission, UserRole};
use crate::workspace::WorkspaceContext;

fn shared_state() -> Arc<PageIndexState> {
    pageindex_glue::state::shared_state()
}

/// Tools exposed by the PageIndex module.
pub fn get_pageindex_tools() -> Vec<MCPTool> {
    pageindex_glue::tool_specs()
        .into_iter()
        .map(|s| MCPTool {
            name: s.name.to_string(),
            description: s.description.to_string(),
            input_schema: s.input_schema,
        })
        .collect()
}

/// True for the tools owned by this module.
pub fn is_pageindex_tool(name: &str) -> bool {
    name.starts_with("pageindex_")
}

/// Run a PageIndex tool with the shared state. `role` is `None` for a static
/// workspace token, which is trusted like it is for memory writes.
pub async fn handle_pageindex_tool(
    workspace: &WorkspaceContext,
    role: Option<&UserRole>,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    handle_with_state(&shared_state(), workspace, role, name, arguments).await
}

async fn handle_with_state(
    state: &Arc<PageIndexState>,
    workspace: &WorkspaceContext,
    role: Option<&UserRole>,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    let ctx = ToolContext {
        workspace: workspace.workspace_id.clone(),
        can_write: role.is_none_or(|r| r.can_add_memory()),
    };
    let envelope = pageindex_glue::call(state, name, &arguments, &ctx).await;
    let is_error = envelope.get("ok").and_then(Value::as_bool) != Some(true);
    Ok(serde_json::to_value(MCPToolResult::structured(
        envelope, is_error,
    ))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pageindex_glue::tools::{
        TOOL_BROWSE, TOOL_GET_DOCUMENT, TOOL_GET_PAGES, TOOL_GET_STRUCTURE, TOOL_INDEX, TOOL_SEARCH,
    };
    use crate::security::auth::Claims;
    use crate::server::mcp::server::{get_xavier_tools, handle_tool_call};
    use crate::server::mcp::tests::test_state;
    use serde_json::json;
    use xavier_pageindex::store::SqliteStore;

    const PI_TOOLS: [&str; 6] = [
        TOOL_BROWSE,
        TOOL_GET_DOCUMENT,
        TOOL_GET_STRUCTURE,
        TOOL_GET_PAGES,
        TOOL_SEARCH,
        TOOL_INDEX,
    ];

    /// Route `handle_tool_call` to an in-memory store (first caller wins; every
    /// test uses its own workspace so sharing is harmless).
    fn install_memory_state() {
        let st = PageIndexState::with_store(
            PageIndexSettings::default(),
            SqliteStore::open_in_memory().expect("in-memory store"),
        );
        let _ = pageindex_glue::state::install_shared_state(st);
    }

    fn claims(role: UserRole) -> Claims {
        Claims::new(
            "u".into(),
            "u@swal.dev".into(),
            role,
            chrono::Duration::hours(1),
        )
    }

    fn payload(res: &Value) -> &Value {
        &res["structuredContent"]
    }

    #[test]
    fn test_mcp_pageindex_tools_announced() {
        let tools = get_xavier_tools();
        for name in PI_TOOLS {
            let t = tools
                .iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("{name} missing from tools/list"));
            assert_eq!(t.input_schema["type"], "object", "{name}");
        }
        assert!(get_pageindex_tools()
            .iter()
            .all(|t| is_pageindex_tool(&t.name)));
    }

    #[tokio::test]
    async fn test_mcp_pageindex_search_tool_listed_and_callable() {
        assert!(get_xavier_tools().iter().any(|t| t.name == TOOL_SEARCH));
        install_memory_state();
        let (state, workspace) = test_state().await;
        let admin = claims(UserRole::Admin);
        let put = json!({
            "doc_name": "mcp-search",
            "content": "# One\nalpha text\n## Two\nzebra stripes\n",
            "format": "markdown"
        });
        handle_tool_call(
            state.clone(),
            workspace.clone(),
            Some(&admin),
            TOOL_INDEX,
            put,
        )
        .await
        .expect("ingest dispatch");
        // Read-only callers may search.
        let ro = claims(UserRole::Readonly);
        let res = handle_tool_call(
            state,
            workspace,
            Some(&ro),
            TOOL_SEARCH,
            json!({"doc_name": "mcp-search", "query": "zebra"}),
        )
        .await
        .expect("search dispatch");
        assert_eq!(res["isError"], false, "{res}");
        assert_eq!(payload(&res)["hits"][0]["page_no"], 1, "{res}");
    }

    #[test]
    fn test_mcp_existing_tool_names_unchanged() {
        let names: Vec<String> = get_xavier_tools().into_iter().map(|t| t.name).collect();
        for expected in [
            "secret_lend",
            "secret_exec",
            "xavier_run_command",
            "xavier_skill_list",
            "memory_save",
            "create_memory",
        ] {
            assert!(names.iter().any(|n| n == expected), "{expected} vanished");
        }
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "duplicate tool names");
    }

    #[tokio::test]
    async fn test_mcp_pageindex_roundtrip_ingest_structure_pages() {
        install_memory_state();
        let (state, workspace) = test_state().await;
        let admin = claims(UserRole::Admin);
        let put = json!({
            "doc_name": "mcp-guide",
            "content": "# One\nalpha text\n## Two\nbeta text\n",
            "format": "markdown"
        });
        let res = handle_tool_call(
            state.clone(),
            workspace.clone(),
            Some(&admin),
            TOOL_INDEX,
            put,
        )
        .await
        .expect("ingest dispatch");
        assert_eq!(res["isError"], false, "{res}");
        assert_eq!(payload(&res)["ok"], true, "{res}");

        let res = handle_tool_call(
            state.clone(),
            workspace.clone(),
            Some(&admin),
            TOOL_GET_STRUCTURE,
            json!({"doc_name": "mcp-guide"}),
        )
        .await
        .expect("structure dispatch");
        let text = res.to_string();
        assert_eq!(payload(&res)["ok"], true, "{res}");
        assert!(text.contains("One") && text.contains("Two"), "{text}");
        assert!(!text.contains("alpha text"), "structure leaked page text");

        let res = handle_tool_call(
            state,
            workspace,
            Some(&admin),
            TOOL_GET_PAGES,
            json!({"doc_name": "mcp-guide", "pages": "1"}),
        )
        .await
        .expect("pages dispatch");
        assert_eq!(payload(&res)["ok"], true, "{res}");
        assert!(res.to_string().contains("alpha text"), "{res}");
    }

    #[tokio::test]
    async fn test_mcp_pageindex_readonly_role_cannot_ingest() {
        install_memory_state();
        let (state, workspace) = test_state().await;
        let put = json!({
            "doc_name": "ro-doc",
            "content": "# A\ntext\n",
            "format": "markdown"
        });
        let ro = claims(UserRole::Readonly);
        let res = handle_tool_call(state.clone(), workspace.clone(), Some(&ro), TOOL_INDEX, put)
            .await
            .expect("envelope, not a transport error");
        assert_eq!(res["isError"], true, "{res}");
        assert_eq!(payload(&res)["ok"], false, "{res}");

        // Reads stay open to the read-only role and nothing was stored.
        let res = handle_tool_call(
            state,
            workspace,
            Some(&ro),
            TOOL_GET_DOCUMENT,
            json!({"doc_name": "ro-doc"}),
        )
        .await
        .expect("read dispatch");
        assert_eq!(payload(&res)["ok"], false, "doc must not exist: {res}");
    }
}
