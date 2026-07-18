#[cfg(test)]
mod tests {
    use crate::server::mcp::tests::{test_state, test_router, post_json, get_json_body};
    use serde_json::json;

    #[tokio::test]
    async fn regression_fat_search_token_savings() {
        let (state, workspace) = test_state().await;
        let router = test_router(state, workspace);

        // 1. Seed a large memory
        let long_content = "A".repeat(5000);
        post_json(
            router.clone(),
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {
                    "name": "create_memory",
                    "arguments": {
                        "path": "fat-search-test/doc1",
                        "content": long_content
                    }
                }
            }),
        ).await;

        // 2. Search without content (Fat Search - Default)
        let resp_fat = post_json(
            router.clone(),
            json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {
                    "name": "mem_search",
                    "arguments": { "query": "fat" }
                }
            }),
        ).await;
        let body_fat = get_json_body(resp_fat).await;
        let text_fat = body_fat["result"]["content"][0]["text"].as_str().unwrap();

        // 3. Search with content
        let resp_full = post_json(
            router.clone(),
            json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {
                    "name": "mem_search",
                    "arguments": { "query": "fat", "include_content": true }
                }
            }),
        ).await;
        let body_full = get_json_body(resp_full).await;
        let text_full = body_full["result"]["content"][0]["text"].as_str().unwrap();

        println!("Fat search result length: {}", text_fat.len());
        println!("Full search result length: {}", text_full.len());

        // Regression: Fat search should be significantly smaller than full search
        assert!(text_fat.len() < 1000, "Fat search should not contain full content");
        assert!(text_full.len() > 5000, "Full search should contain full content");
        assert!(text_fat.len() < text_full.len() / 5, "Fat search should be at least 5x smaller");
    }

    #[tokio::test]
    async fn regression_memory_context_targeted_page_in() {
        let (state, workspace) = test_state().await;
        let router = test_router(state, workspace);

        let content1 = "Content one " .to_string() + &"A".repeat(1000);
        let content2 = "Content two " .to_string() + &"B".repeat(1000);

        // Seed memories
        post_json(router.clone(), json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "create_memory", "arguments": { "path": "p/1", "content": content1 }}
        })).await;
        post_json(router.clone(), json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": "create_memory", "arguments": { "path": "p/2", "content": content2 }}
        })).await;

        // Get ID for p/1
        let resp_search = post_json(router.clone(), json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "mem_search", "arguments": { "query": "one" }}
        })).await;
        let body_search = get_json_body(resp_search).await;
        let search_text = body_search["result"]["content"][0]["text"].as_str().unwrap();
        let id1 = search_text.split('\n').next().unwrap().strip_prefix("Id: ").unwrap();

        // memory_context with ID (targeted page-in)
        let resp_context = post_json(router.clone(), json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "memory_context", "arguments": { "ids": [id1] }}
        })).await;
        let body_context = get_json_body(resp_context).await;
        let sc = &body_context["result"]["content"][0]["structuredContent"];
        let content = sc["content"].as_str().unwrap();

        assert!(content.contains("Content one"), "Context should contain requested doc");
        assert!(!content.contains("Content two"), "Context should NOT contain unrequested doc");
    }

    #[tokio::test]
    async fn regression_token_estimation_honest_reporting() {
        let (state, workspace) = test_state().await;
        let router = test_router(state, workspace.clone());

        let session_id = "test-session";
        let long_content = "Word ".repeat(100); // ~500 chars

        // 1. Create a message via create_checkpoint (mocking history)
        // Actually, let's use record_session_exchange if available or directly inject into conversations_db
        let _: String = workspace.workspace.record_session_exchange(session_id, "test", "hello", &long_content).await.unwrap();

        // 2. Restore context
        let resp_restore = post_json(router.clone(), json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {
                "name": "xavier_context_restore",
                "arguments": { "session_id": session_id, "depth": "medium" }
            }
        })).await;
        let body_restore = get_json_body(resp_restore).await;
        let text = body_restore["result"]["content"][0]["text"].as_str().unwrap();
        let result: serde_json::Value = serde_json::from_str(text).unwrap();

        let optimized_tokens = result["token_usage"]["optimized"].as_u64().unwrap();

        let expected_tokens = crate::context::estimate_tokens(result["context"].as_str().unwrap()) as u64;

        assert_eq!(optimized_tokens, expected_tokens, "Token estimation should be honest (chars / 4)");
    }
}
