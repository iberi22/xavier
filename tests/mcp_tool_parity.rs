use std::collections::HashSet;
use axum::body::to_bytes;
use axum::response::IntoResponse;
use serde_json::Value;

xavier::isolate_test_process!();

#[tokio::test]
async fn mcp_endpoints_expose_same_tool_list() {
    // 1. Get the list of tools from the JSON-RPC MCP endpoint
    let mcp_tools = xavier::server::mcp::server::get_xavier_tools();
    let mcp_tool_names: HashSet<String> = mcp_tools.into_iter().map(|t| t.name).collect();

    // 2. Get the list of tools from the legacy REST /mcp/tools endpoint
    let response = xavier::cli::handlers::workspace::mcp_tools_handler().await.into_response();

    // We expect the handler to return an OK status (or 200). Wait, it's just a raw tuple in the handler: (headers, Json(...))
    // We can extract the body.
    let bytes = to_bytes(response.into_body(), usize::MAX).await.expect("Failed to read body");
    let json_body: Value = serde_json::from_slice(&bytes).expect("Failed to parse JSON body");

    let rest_tools_array = json_body.get("tools")
        .expect("Missing 'tools' array in REST response")
        .as_array()
        .expect("'tools' is not an array");

    let mut rest_tool_names: HashSet<String> = HashSet::new();
    for tool in rest_tools_array {
        let name = tool.get("name")
            .expect("Tool missing 'name' field")
            .as_str()
            .expect("'name' is not a string")
            .to_string();
        rest_tool_names.insert(name);
    }

    // 3. Compare the sets
    let missing_in_rest: Vec<_> = mcp_tool_names.difference(&rest_tool_names).collect();
    let missing_in_mcp: Vec<_> = rest_tool_names.difference(&mcp_tool_names).collect();

    let mut error_msg = String::new();
    if !missing_in_rest.is_empty() {
        error_msg.push_str(&format!("Tools missing from REST endpoint (/mcp/tools): {:?}\n", missing_in_rest));
    }
    if !missing_in_mcp.is_empty() {
        error_msg.push_str(&format!("Tools missing from JSON-RPC MCP endpoint (get_xavier_tools): {:?}\n", missing_in_mcp));
    }

    assert!(error_msg.is_empty(), "Tool lists differ between endpoints!\n{}", error_msg);
}
