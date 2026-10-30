use axum::response::IntoResponse;
use std::collections::HashSet;
use std::fs;
use xavier::cli::handlers::workspace::mcp_tools_handler;
use xavier::isolate_test_process;
use xavier::server::mcp::server::get_xavier_tools;

isolate_test_process!();

#[tokio::test]
async fn mcp_surface_split_drift_test() {
    // 1. Parse the Markdown table from docs/MCP_CONTRACT.md
    let doc_content =
        fs::read_to_string("docs/MCP_CONTRACT.md").expect("failed to read MCP_CONTRACT.md");
    let mut table_started = false;

    let mut doc_tools_8100 = HashSet::new();
    let mut doc_tools_8006 = HashSet::new();

    for line in doc_content.lines() {
        if line.starts_with("| Tool Name | :8006 | :8100 | Purpose |") {
            table_started = true;
            continue;
        }
        if table_started && line.starts_with("|---") {
            continue;
        }
        if table_started && line.starts_with('|') {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 4 {
                let name = parts[1].trim().replace('`', "");
                let on_8006 = parts[2].trim().eq_ignore_ascii_case("yes");
                let on_8100 = parts[3].trim().eq_ignore_ascii_case("yes");

                if on_8100 {
                    doc_tools_8100.insert(name.clone());
                }
                if on_8006 {
                    doc_tools_8006.insert(name);
                }
            }
        } else if table_started {
            break; // end of table
        }
    }

    assert!(
        !doc_tools_8100.is_empty(),
        "failed to parse tools from markdown"
    );

    // PageIndex tools are only registered on :8100 when the `pageindex` feature is
    // compiled in (src/server/mcp/server.rs); drop them from the documented
    // expectation otherwise so the drift test holds for every feature set.
    if !cfg!(feature = "pageindex") {
        doc_tools_8100.retain(|name| !name.starts_with("pageindex_"));
    }

    // 2. Fetch the actual tools for :8100
    // The table lists all tools, including telecom tools, so we pass has_telecom = true
    let actual_tools_8100: HashSet<String> =
        get_xavier_tools(true).into_iter().map(|t| t.name).collect();

    // 3. Fetch actual tools for :8006
    let resp = mcp_tools_handler().await.into_response();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let mut actual_tools_8006 = HashSet::new();
    if let Some(tools) = json.get("tools").and_then(|t| t.as_array()) {
        for t in tools {
            if let Some(name) = t.get("name").and_then(|n| n.as_str()) {
                actual_tools_8006.insert(name.to_string());
            }
        }
    }

    assert!(
        !actual_tools_8006.is_empty(),
        "failed to parse tools from mcp_tools_handler"
    );

    // 4. Assert exact parity
    let missing_from_doc_8100: Vec<_> = actual_tools_8100.difference(&doc_tools_8100).collect();
    let extra_in_doc_8100: Vec<_> = doc_tools_8100.difference(&actual_tools_8100).collect();

    assert!(
        missing_from_doc_8100.is_empty(),
        "Tools missing from 8100 list in docs/MCP_CONTRACT.md: {:?}",
        missing_from_doc_8100
    );
    assert!(
        extra_in_doc_8100.is_empty(),
        "Extra tools in 8100 list in docs/MCP_CONTRACT.md that do not exist: {:?}",
        extra_in_doc_8100
    );

    let missing_from_doc_8006: Vec<_> = actual_tools_8006.difference(&doc_tools_8006).collect();
    let extra_in_doc_8006: Vec<_> = doc_tools_8006.difference(&actual_tools_8006).collect();

    assert!(
        missing_from_doc_8006.is_empty(),
        "Tools missing from 8006 list in docs/MCP_CONTRACT.md: {:?}",
        missing_from_doc_8006
    );
    assert!(
        extra_in_doc_8006.is_empty(),
        "Extra tools in 8006 list in docs/MCP_CONTRACT.md that do not exist: {:?}",
        extra_in_doc_8006
    );
}
