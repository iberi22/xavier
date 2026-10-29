use axum::body::to_bytes;
use axum::{
    body::Body,
    http::{Method, Request},
};
use serde_json::Value;
use serial_test::serial;
use std::env;
use tempfile::TempDir;
use tower::ServiceExt;

xavier::isolate_test_process!();

struct EnvGuard {
    vars: Vec<(String, Option<String>)>,
    prev_dir: std::path::PathBuf,
}

impl EnvGuard {
    fn new(dir: &std::path::Path, vars: &[(&str, &str)]) -> Self {
        let prev_dir = env::current_dir().unwrap();
        let mut saved = Vec::new();
        for &(k, v) in vars {
            saved.push((k.to_string(), env::var(k).ok()));
            env::set_var(k, v);
        }
        env::set_current_dir(dir).unwrap();
        Self {
            vars: saved,
            prev_dir,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.prev_dir);
        for (k, v) in &self.vars {
            if let Some(v) = v {
                env::set_var(k, v);
            } else {
                env::remove_var(k);
            }
        }
    }
}

#[tokio::test]
#[serial]
async fn mcp_endpoints_expose_same_tool_list() {
    let temp = TempDir::new().expect("tempdir");
    let temp_str = temp.path().to_str().unwrap();
    let vec_path = temp.path().join("vec.sqlite3");
    let vec_path_str = vec_path.to_str().unwrap();
    let cg_path = temp.path().join("code_graph.db");
    let cg_path_str = cg_path.to_str().unwrap();

    let _guard = EnvGuard::new(
        temp.path(),
        &[
            ("TMPDIR", temp_str),
            ("XAVIER_DATA_DIR", temp_str),
            ("XAVIER_CONFIG_PATH", temp_str),
            ("XAVIER_RUNTIME_STATE_PATH", temp_str),
            ("HOME", temp_str),
            ("XAVIER_TOKEN", "test-token"),
            ("XAVIER_JWT_SECRET", "test-secret"),
            ("XAVIER_MEMORY_VEC_PATH", vec_path_str),
            ("XAVIER_CODE_GRAPH_DB_PATH", cg_path_str),
            ("XAVIER_EMBEDDER", "disabled"),
        ],
    );

    // Build state
    let (state, workspace) = xavier::cli::mcp::build_mcp_state()
        .await
        .expect("build_mcp_state failed");

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "tools/list",
        "id": 1
    });

    // 1. Simulate the JSON-RPC POST /mcp tools/list handler
    let app =
        xavier::server::mcp::transport::build_mcp_http_router(state.clone(), workspace.clone());

    let user = xavier::security::auth::User::new(
        "test-sub".to_string(),
        "test@example.com".to_string(),
        xavier::security::auth::UserRole::Admin,
    );
    let token = xavier::security::auth::generate_jwt(&user, b"test-secret").expect("jwt");

    let req = Request::builder()
        .uri("/mcp")
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .header("Origin", "http://localhost")
        .body(Body::from(payload.to_string()))
        .expect("request");

    let response = app.oneshot(req).await.expect("response");
    assert!(
        response.status().is_success(),
        "HTTP request failed: {:?}",
        response.status()
    );

    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json_rpc_response: Value = serde_json::from_slice(&bytes).expect("json");

    // 2. Simulate the stdio / xavier mcp tools/list path
    let stdio_response = xavier::server::mcp_stdio::dispatch_stdio_value(
        state.clone(),
        workspace.clone(),
        payload.clone(),
    )
    .await
    .expect("dispatch stdio")
    .expect("expected response");

    // Extract tool names
    fn extract_tool_names(response: &Value) -> Vec<String> {
        let tools_array = response
            .get("result")
            .expect("missing result")
            .get("tools")
            .expect("missing tools array")
            .as_array()
            .expect("tools is not an array");

        let mut names: Vec<String> = tools_array
            .iter()
            .map(|t| {
                t.get("name")
                    .expect("missing name")
                    .as_str()
                    .expect("name is not string")
                    .to_string()
            })
            .collect();
        assert!(!names.is_empty(), "Tool list is empty");

        let initial_len = names.len();
        names.sort();
        names.dedup();
        assert_eq!(initial_len, names.len(), "Tool list contains duplicates");

        names
    }

    let json_rpc_names = extract_tool_names(&json_rpc_response);
    let stdio_names = extract_tool_names(&stdio_response);

    // 3. Compare the sets
    let mut missing_in_rpc = Vec::new();
    for name in &stdio_names {
        if !json_rpc_names.contains(name) {
            missing_in_rpc.push(name.clone());
        }
    }

    let mut missing_in_stdio = Vec::new();
    for name in &json_rpc_names {
        if !stdio_names.contains(name) {
            missing_in_stdio.push(name.clone());
        }
    }

    let mut error_msg = String::new();
    if !missing_in_rpc.is_empty() {
        error_msg.push_str(&format!(
            "Tools missing from JSON-RPC MCP endpoint: {:?}\n",
            missing_in_rpc
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
