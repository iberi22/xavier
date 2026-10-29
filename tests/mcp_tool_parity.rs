use serde_json::Value;
use std::collections::HashSet;

xavier::isolate_test_process!();

#[tokio::test]
async fn mcp_endpoints_expose_same_tool_list() {
    // Both endpoints (stdio and HTTP JSON-RPC) share the same build_mcp_state fn,
    // which builds the AppState and WorkspaceContext for them.
    // They both dispatch requests to `dispatch_mcp_value` internally.
    let (state, workspace) = xavier::cli::mcp::build_mcp_state()
        .await
        .expect("build_mcp_state failed");

    // Build the request payload for tools/list
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "tools/list",
        "id": 1
    });

    // 1. Simulate the HTTP /mcp tools/list handler (HTTP transport uses the workspace authenticated user)
    // The HTTP transport delegates to `dispatch_mcp_value` with some claims.
    let claims = xavier::security::auth::Claims::new(
        "test-sub".to_string(),
        "test@example.com".to_string(),
        xavier::security::auth::UserRole::Admin,
        chrono::Duration::hours(1),
    );
    let http_response = xavier::server::mcp::session::dispatch_mcp_value(
        state.clone(),
        workspace.clone(),
        Some(&claims),
        payload.clone(),
    )
    .await
    .expect("dispatch http")
    .expect("expected response");

    // 2. Simulate the stdio / xavier mcp tools/list path
    // The stdio transport overrides some caller constraints before calling dispatch_mcp_value.
    let stdio_response = xavier::server::mcp_stdio::dispatch_stdio_value(
        state.clone(),
        workspace.clone(),
        payload.clone(),
    )
    .await
    .expect("dispatch stdio")
    .expect("expected response");

    // Extract tool names
    fn extract_tool_names(response: &Value) -> HashSet<String> {
        let tools_array = response
            .get("result")
            .expect("missing result")
            .get("tools")
            .expect("missing tools array")
            .as_array()
            .expect("tools is not an array");

        let mut names = HashSet::new();
        for tool in tools_array {
            let name = tool
                .get("name")
                .expect("missing name")
                .as_str()
                .expect("name is not string")
                .to_string();
            names.insert(name);
        }
        names
    }

    let http_names = extract_tool_names(&http_response);
    let stdio_names = extract_tool_names(&stdio_response);

    // 3. Compare the sets
    let missing_in_http: Vec<_> = stdio_names.difference(&http_names).collect();
    let missing_in_stdio: Vec<_> = http_names.difference(&stdio_names).collect();

    let mut error_msg = String::new();
    if !missing_in_http.is_empty() {
        error_msg.push_str(&format!(
            "Tools missing from HTTP MCP endpoint: {:?}\n",
            missing_in_http
        ));
    }
    if !missing_in_stdio.is_empty() {
        error_msg.push_str(&format!(
            "Tools missing from stdio MCP endpoint: {:?}\n",
            missing_in_stdio
        ));
    }

    assert!(
        error_msg.is_empty(),
        "Tool lists differ between endpoints!\n{}",
        error_msg
    );
}
