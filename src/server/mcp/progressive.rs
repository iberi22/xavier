#[cfg(test)]
mod tests {
    use crate::server::mcp::tests::{get_json_body, post_json, test_router, test_state};
    use serde_json::json;

    #[tokio::test]
    async fn mem_search_progressive_disclosure() {
        let (state, workspace) = test_state().await;
        let router = test_router(state, workspace);

        // 1. Seed a large memory document
        let document_body = "PROD_SPEC_999: " .to_string() + &"X".repeat(4000) + " END_SPEC";
        let create_resp = post_json(
            router.clone(),
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "create_memory",
                    "arguments": {
                        "path": "specs/prod-999.txt",
                        "content": document_body
                    }
                }
            }),
        )
        .await;
        assert_eq!(create_resp.status(), axum::http::StatusCode::OK);

        // 2. Perform a search using `mem_search` (default: include_content = false)
        let search_resp_default = post_json(
            router.clone(),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "mem_search",
                    "arguments": {
                        "query": "PROD_SPEC_999"
                    }
                }
            }),
        )
        .await;
        assert_eq!(search_resp_default.status(), axum::http::StatusCode::OK);

        let body_default = get_json_body(search_resp_default).await;
        assert!(body_default["result"].is_object(), "Search result should be an object");

        let content_array_default = body_default["result"]["content"]
            .as_array()
            .expect("content should be an array");
        assert!(!content_array_default.is_empty(), "Should return at least one search result");

        let sc_default = &content_array_default[0]["structuredContent"];
        let candidates_default = sc_default["candidates"].as_array().expect("candidates should be an array");
        assert!(!candidates_default.is_empty(), "Candidates must not be empty");

        let first_cand = &candidates_default[0];
        let id_default = first_cand["id"].as_str().expect("ID should be a string");
        let path_default = first_cand["path"].as_str().expect("Path should be a string");
        let snippet_default = first_cand["snippet"].as_str().expect("Snippet should be a string");

        // Assert no full content is included by default (progressive disclosure)
        assert!(!id_default.is_empty(), "ID should not be empty");
        assert_eq!(path_default, "specs/prod-999.txt");
        assert!(snippet_default.contains("PROD_SPEC_999"));
        assert!(first_cand.get("content").is_none(), "Search result must NOT contain full body content by default");

        // 3. Perform a search using `mem_search` with include_content = true
        let search_resp_full = post_json(
            router.clone(),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "mem_search",
                    "arguments": {
                        "query": "PROD_SPEC_999",
                        "include_content": true
                    }
                }
            }),
        )
        .await;
        assert_eq!(search_resp_full.status(), axum::http::StatusCode::OK);

        let body_full = get_json_body(search_resp_full).await;
        let content_array_full = body_full["result"]["content"]
            .as_array()
            .expect("content should be an array");
        let sc_full = &content_array_full[0]["structuredContent"];
        let candidates_full = sc_full["candidates"].as_array().expect("candidates array");
        let content_full = candidates_full[0]["content"].as_str().expect("Content must be disclosed");

        // Assert full content is disclosed when requested
        assert!(content_full.contains("PROD_SPEC_999"), "Full search result must contain prefix");
        assert!(content_full.contains("END_SPEC"), "Full search result must contain the full body ending");
        assert!(content_full.len() > 4000, "Disclosed full search response should be large");
    }

    #[tokio::test]
    async fn memory_context_targeted_ids() {
        let (state, workspace) = test_state().await;
        let router = test_router(state, workspace);

        // 1. Seed two unique memories
        let content1 = "Rust is a systems programming language focusing on safety and speed.";
        let content2 = "Python is an interpreted programming language focusing on readability.";

        // Ingest first document
        post_json(
            router.clone(),
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "create_memory",
                    "arguments": {
                        "path": "lang/rust.txt",
                        "content": content1
                    }
                }
            }),
        )
        .await;

        // Ingest second document
        post_json(
            router.clone(),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "create_memory",
                    "arguments": {
                        "path": "lang/python.txt",
                        "content": content2
                    }
                }
            }),
        )
        .await;

        // 2. Use search to get IDs
        let search_rust_resp = post_json(
            router.clone(),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "mem_search",
                    "arguments": {
                        "query": "Rust"
                    }
                }
            }),
        )
        .await;
        let body_rust = get_json_body(search_rust_resp).await;
        let id_rust = body_rust["result"]["content"][0]["structuredContent"]["candidates"][0]["id"].as_str().unwrap();

        let search_python_resp = post_json(
            router.clone(),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "mem_search",
                    "arguments": {
                        "query": "Python"
                    }
                }
            }),
        )
        .await;
        let body_python = get_json_body(search_python_resp).await;
        let id_python = body_python["result"]["content"][0]["structuredContent"]["candidates"][0]["id"].as_str().unwrap();

        // Ensure we retrieved two different valid IDs
        assert_ne!(id_rust, id_python, "The two documents should have unique IDs");

        // 3. Request memory_context passing ONLY the Rust ID
        let context_resp_rust = post_json(
            router.clone(),
            json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {
                    "name": "memory_context",
                    "arguments": {
                        "ids": [id_rust]
                    }
                }
            }),
        )
        .await;
        assert_eq!(context_resp_rust.status(), axum::http::StatusCode::OK);

        let context_body_rust = get_json_body(context_resp_rust).await;
        let sc_rust = &context_body_rust["result"]["content"][0]["structuredContent"];
        let content_rust_out = sc_rust["content"].as_str().unwrap();

        // Assert the returned context block has only the requested doc
        assert!(content_rust_out.contains("Rust is a systems"), "Context should contain requested Rust document");
        assert!(!content_rust_out.contains("Python is an interpreted"), "Context must NOT contain Python document");
        assert_eq!(sc_rust["totalRecords"].as_u64().unwrap(), 1, "Only 1 record should be in context");

        let sources_rust = sc_rust["sources"].as_array().unwrap();
        assert_eq!(sources_rust.len(), 1, "There should be exactly one source record");
        assert_eq!(sources_rust[0]["id"].as_str().unwrap(), id_rust, "The source record ID must match the requested Rust ID");

        // 4. Request memory_context passing ONLY the Python ID
        let context_resp_python = post_json(
            router.clone(),
            json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": {
                    "name": "memory_context",
                    "arguments": {
                        "ids": [id_python]
                    }
                }
            }),
        )
        .await;
        assert_eq!(context_resp_python.status(), axum::http::StatusCode::OK);

        let context_body_python = get_json_body(context_resp_python).await;
        let sc_python = &context_body_python["result"]["content"][0]["structuredContent"];
        let content_python_out = sc_python["content"].as_str().unwrap();

        // Assert the returned context block has only the requested doc
        assert!(content_python_out.contains("Python is an interpreted"), "Context should contain requested Python document");
        assert!(!content_python_out.contains("Rust is a systems"), "Context must NOT contain Rust document");
        assert_eq!(sc_python["totalRecords"].as_u64().unwrap(), 1, "Only 1 record should be in context");

        let sources_python = sc_python["sources"].as_array().unwrap();
        assert_eq!(sources_python.len(), 1, "There should be exactly one source record");
        assert_eq!(sources_python[0]["id"].as_str().unwrap(), id_python, "The source record ID must match the requested Python ID");
    }
}
