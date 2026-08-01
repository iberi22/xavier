#[cfg(test)]
mod tests {
    use crate::server::mcp::tests::{get_json_body, post_json, test_router, test_state};
    use serde_json::json;

    #[tokio::test]
    async fn regression_fat_search_token_savings() {
        let (state, workspace) = test_state().await;
        let router = test_router(state, workspace);

        // 1. Seed a large memory with realistic markdown content
        let long_content = "# Xavier Architecture\n\n## Core Philosophy\n\nXavier is built on a **Hexagonal Architecture** (Ports & Adapters) to ensure the core logic remains isolated from external dependencies like database drivers, LLM providers, and transport protocols (HTTP/CLI/MCP).\n\n## Memory Stores\n\n### Primary Backend: SQLite-Vec\n\nAs of v0.6+, Xavier has transitioned to **SQLite-Vec** as the primary storage engine.\nThis provides a zero-infrastructure, ACID-compliant vector database.\n\n### Semantic Layer\n\n- **Belief Graph**: Maps semantic relationships between memories (L0-L1-L2 hierarchy).\n- **Hybrid Retrieval**: Uses Reciprocal Rank Fusion (RRF) to combine keyword (FTS5) and semantic (Vector) search.\n- **Threat Detection**: Integrated SecurityScanner for prompt injection and leak monitoring.\n\n## System Components\n\n### 1. Inbound Ports (Entry Points)\n\n- **HTTP API**: High-performance Axum-based REST API with token authentication.\n- **CLI**: Command-line interface for local memory operations.\n- **MCP**: Model Context Protocol for native AI agent integration.\n\n### 2. Domain Core\n\n- **ProxyUseCase**: The central orchestrator coordinating security, embeddings, and persistence.\n- **SecurityService**: Multi-layer scanner (Aho-Corasick, Entropy, Regex) for input validation.\n\n### 3. Outbound Ports\n\n- **MemoryBackend**: Trait defining persistence operations.\n- **EmbeddingPort**: Interface for vector generation.\n\n## Release Status\n\n| Feature | Status | Verified |\n|---------|--------|----------|\n| Hierarchical Memory | Stable | ✅ |\n| Belief Graph | Stable | ✅ |\n| Security Scanner | Stable | ✅ |\n| TUI Installer | Stable | ✅ |\n| Public Export | Stable | ✅ |\n\n## Development Ecosystem\n\nXavier uses autonomous agents for continuous improvement:\n- **Jules**: Background execution agent for refactoring and clippy fixes.\n- **Antigravity**: Strategic architect and integration manager.\n\n---\n\n*This document describes the architecture as of v0.6.1-beta.*".to_string();
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
        )
        .await;

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
        )
        .await;
        let body_fat = get_json_body(resp_fat).await;
        let candidates_fat = body_fat["result"]["content"][0]["structuredContent"]["candidates"]
            .as_array()
            .unwrap();
        assert!(!candidates_fat.is_empty());
        let snippet_len = candidates_fat[0]["snippet"].as_str().unwrap_or("").len();

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
        )
        .await;
        let body_full = get_json_body(resp_full).await;
        let candidates_full = body_full["result"]["content"][0]["structuredContent"]["candidates"]
            .as_array()
            .unwrap();
        assert!(!candidates_full.is_empty());
        let content_len = candidates_full[0]["content"].as_str().unwrap_or("").len();

        println!("Fat search snippet length: {}", snippet_len);
        println!("Full search content length: {}", content_len);
        println!(
            "Fat search saved {} bytes (ratio: {:.2}%)",
            content_len.saturating_sub(snippet_len),
            if content_len > 0 {
                (content_len.saturating_sub(snippet_len)) as f64 / content_len as f64 * 100.0
            } else {
                0.0
            }
        );

        // Regression: Fat search should be significantly smaller than full search
        assert!(
            snippet_len < 1000,
            "Fat search snippet should be small, got {snippet_len}"
        );
        assert!(content_len >= long_content.len() / 2, "Full search content should contain substantial content, got {content_len} vs original {}", long_content.len());
        assert!(snippet_len < content_len / 5, "Fat search snippet ({snippet_len}B) should be at least 5x smaller than full ({content_len}B)");
    }

    #[tokio::test]
    async fn regression_memory_context_targeted_page_in() {
        let (state, workspace) = test_state().await;
        let router = test_router(state, workspace);

        // Seed two memories with realistic content
        let content1 = "## Chapter 1: Introduction\n\nThis chapter covers the fundamental concepts of Rust programming including ownership, borrowing, and lifetimes. These concepts form the foundation of memory safety without a garbage collector. Rust's type system guarantees memory safety at compile time, making it ideal for systems programming."
            .to_string();
        let content2 = "## Chapter 2: Advanced Features\n\nThis chapter explores advanced Rust features: traits, generics, closures, iterators, and pattern matching. These features enable expressive, zero-cost abstractions while maintaining the strict safety guarantees that Rust provides."
            .to_string();

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
        let resp_search = post_json(
            router.clone(),
            json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": { "name": "mem_search", "arguments": { "query": "ownership" }}
            }),
        )
        .await;
        let body_search = get_json_body(resp_search).await;
        let candidates_search = body_search["result"]["content"][0]["structuredContent"]
            ["candidates"]
            .as_array()
            .unwrap();
        assert!(!candidates_search.is_empty());
        let id1 = candidates_search[0]["id"].as_str().unwrap();

        // memory_context with ID (targeted page-in)
        let resp_context = post_json(
            router.clone(),
            json!({
                "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                "params": { "name": "memory_context", "arguments": { "ids": [id1] }}
            }),
        )
        .await;
        let body_context = get_json_body(resp_context).await;
        let sc = &body_context["result"]["content"][0]["structuredContent"];
        let content = sc["content"].as_str().unwrap();

        assert!(
            content.contains("Chapter 1"),
            "Context should contain requested doc (Chapter 1)"
        );
        assert!(
            !content.contains("Chapter 2"),
            "Context should NOT contain unrequested doc (Chapter 2)"
        );
    }

    #[tokio::test]
    async fn regression_token_estimation_honest_reporting() {
        let (state, workspace) = test_state().await;
        let router = test_router(state, workspace.clone());

        let session_id = "test-session";
        let long_content = "Word ".repeat(100); // ~500 chars

        // 1. Create a message via create_checkpoint (mocking history)
        // Actually, let's use record_session_exchange if available or directly inject into conversations_db
        let _: String = workspace
            .workspace
            .record_session_exchange(session_id, "test", "hello", &long_content)
            .await
            .unwrap();

        // 2. Restore context
        let resp_restore = post_json(
            router.clone(),
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {
                    "name": "xavier_context_restore",
                    "arguments": { "session_id": session_id, "depth": "medium" }
                }
            }),
        )
        .await;
        let body_restore = get_json_body(resp_restore).await;
        let text = body_restore["result"]["content"][0]["text"]
            .as_str()
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(text).unwrap();

        let optimized_tokens = result["token_usage"]["optimized"].as_u64().unwrap();

        let expected_tokens =
            crate::context::estimate_tokens(result["context"].as_str().unwrap()) as u64;

        assert_eq!(
            optimized_tokens, expected_tokens,
            "Token estimation should be honest (chars / 4)"
        );
    }
}
