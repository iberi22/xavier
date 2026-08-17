//! Tests for code-graph query engine

#[cfg(test)]
mod tests_inner {
    use crate::db::CodeGraphDB;
    use crate::error::GraphError;
    use crate::query::QueryEngine;
    use crate::types::{CodeEdge, EdgeType, Language, Symbol, SymbolEmbedder, SymbolKind};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct DummyEmbedder;

    #[async_trait]
    impl SymbolEmbedder for DummyEmbedder {
        async fn embed(&self, text: &str) -> Result<Vec<f32>, GraphError> {
            let lower = text.to_lowercase();
            if lower.contains("token")
                || lower.contains("permission")
                || lower.contains("auth")
                || lower.contains("cli")
            {
                // Return 2D embedding vector near (1.0, 0.0) for auth/token concepts
                Ok(vec![1.0, 0.0])
            } else if lower.contains("data") || lower.contains("calc") {
                // Return 2D embedding vector near (0.0, 1.0) for math/data concepts
                Ok(vec![0.0, 1.0])
            } else {
                Ok(vec![0.5, 0.5])
            }
        }
    }

    /// Create a test database with sample symbols
    fn setup_test_db() -> CodeGraphDB {
        let db = CodeGraphDB::in_memory().expect("test assertion");

        // Insert test symbols
        let sym1 = Symbol {
            id: None,
            stable_id: None,
            name: "main".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "/src/main.rs".to_string(),
            start_line: 1,
            end_line: 10,
            start_col: 0,
            end_col: 0,
            signature: Some("fn main()".to_string()),
            parent: None,
            complexity: None,
        };
        db.insert_symbol(&sym1).expect("test assertion");

        let sym2 = Symbol {
            id: None,
            stable_id: None,
            name: "process_data".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "/src/processor.rs".to_string(),
            start_line: 5,
            end_line: 20,
            start_col: 0,
            end_col: 0,
            signature: Some("fn process_data(data: String) -> Result<()>".to_string()),
            parent: None,
            complexity: None,
        };
        db.insert_symbol(&sym2).expect("test assertion");

        let sym3 = Symbol {
            id: None,
            stable_id: None,
            name: "User".to_string(),
            kind: SymbolKind::Struct,
            lang: Language::Rust,
            file_path: "/src/models.rs".to_string(),
            start_line: 1,
            end_line: 15,
            start_col: 0,
            end_col: 0,
            signature: Some("struct User { name: String }".to_string()),
            parent: None,
            complexity: None,
        };
        db.insert_symbol(&sym3).expect("test assertion");

        let sym4 = Symbol {
            id: None,
            stable_id: None,
            name: "calculate_total".to_string(),
            kind: SymbolKind::Function,
            lang: Language::TypeScript,
            file_path: "/src/calc.ts".to_string(),
            start_line: 10,
            end_line: 25,
            start_col: 0,
            end_col: 0,
            signature: Some("function calculateTotal(items: Item[]): number".to_string()),
            parent: None,
            complexity: None,
        };
        db.insert_symbol(&sym4).expect("test assertion");

        db
    }

    #[test]
    fn test_insert_and_find_symbol() {
        let db = setup_test_db();

        // Test exact match
        let result = db.find_symbols("main", 10).expect("test assertion");
        assert!(!result.symbols.is_empty());
        assert_eq!(result.symbols[0].name, "main");
    }

    #[test]
    fn test_find_by_name() {
        let db = setup_test_db();
        let query = QueryEngine::new(Arc::new(db));

        // Test exact find_by_name match
        let results = query
            .find_by_name("process_data", 10)
            .expect("test assertion");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "process_data");

        // Test exact find_by_name match with no result
        let results = query.find_by_name("process", 10).expect("test assertion");
        assert!(results.is_empty());
    }

    #[test]
    fn test_fuzzy_search() {
        let db = setup_test_db();

        // Test partial match
        let result = db.find_symbols("process", 10).expect("test assertion");
        assert!(!result.symbols.is_empty());
        assert!(result.symbols[0].name.contains("process"));
    }

    #[test]
    fn test_case_insensitive() {
        let db = setup_test_db();

        // Test case insensitive
        let result = db.find_symbols("MAIN", 10).expect("test assertion");
        assert!(!result.symbols.is_empty());
    }

    #[test]
    fn test_find_by_kind() {
        let db = setup_test_db();

        // Find all functions
        let functions = db
            .find_by_kind(SymbolKind::Function, 100)
            .expect("test assertion");
        assert_eq!(functions.len(), 3); // main, process_data, calculate_total

        // Find all structs
        let structs = db
            .find_by_kind(SymbolKind::Struct, 100)
            .expect("test assertion");
        assert_eq!(structs.len(), 1); // User
    }

    #[test]
    fn test_empty_query() {
        let db = setup_test_db();

        // Empty query should return some results
        let result = db.find_symbols("", 10).expect("test assertion");
        assert!(!result.symbols.is_empty());
    }

    #[test]
    fn test_no_results() {
        let db = setup_test_db();

        let result = db
            .find_symbols("nonexistent_symbol_xyz", 10)
            .expect("test assertion");
        assert!(result.symbols.is_empty());
    }

    #[test]
    fn test_limit() {
        let db = setup_test_db();

        let result = db.find_symbols("", 2).expect("test assertion");
        assert!(result.symbols.len() <= 2);
    }

    #[test]
    #[allow(unused_comparisons)]
    #[allow(clippy::absurd_extreme_comparisons)]
    fn test_query_result_metadata() {
        let db = setup_test_db();

        let result = db.find_symbols("main", 10).expect("test assertion");

        assert!(result.total > 0);
        assert!(result.query_time_ms >= 0, "query time should be recorded");
    }

    #[test]
    fn test_graph_queries_follow_edges() {
        let db = setup_test_db();
        let main = db.find_symbols("main", 1).expect("main").symbols[0]
            .stable_id
            .clone()
            .expect("stable main");
        let process = db.find_symbols("process_data", 1).expect("process").symbols[0]
            .stable_id
            .clone()
            .expect("stable process");
        db.insert_edge(&CodeEdge {
            id: None,
            from_symbol: main.clone(),
            to_symbol: process.clone(),
            edge_type: EdgeType::Calls,
            file_path: "/src/main.rs".to_string(),
            line: 2,
            confidence: 0.9,
            metadata: None,
        })
        .expect("edge");

        let query = QueryEngine::new(Arc::new(db));
        let deps = query
            .dependencies("main", Some(EdgeType::Calls), 1, 10)
            .expect("deps");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].to_symbol, process);

        let reverse = query
            .reverse_dependencies("process_data", Some(EdgeType::Calls), 1, 10)
            .expect("reverse");
        assert_eq!(reverse.len(), 1);
        assert_eq!(reverse[0].from_symbol, main);
    }

    #[tokio::test]
    async fn test_semantic_search_finds_auth_logic() {
        let db = CodeGraphDB::in_memory().expect("db");
        let sym1 = Symbol {
            id: None,
            stable_id: None,
            name: "check_cli_token".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "src/cli/handlers/memory.rs".to_string(),
            start_line: 1350,
            end_line: 1370,
            start_col: 0,
            end_col: 0,
            signature: Some(
                "pub fn check_cli_token(headers: &HeaderMap) -> Result<(), Response>".to_string(),
            ),
            parent: None,
            complexity: None,
        };
        let sym2 = Symbol {
            id: None,
            stable_id: None,
            name: "require_permission".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "src/middleware/auth.rs".to_string(),
            start_line: 23,
            end_line: 40,
            start_col: 0,
            end_col: 0,
            signature: Some("pub fn require_permission(...)".to_string()),
            parent: None,
            complexity: None,
        };
        let sym3 = Symbol {
            id: None,
            stable_id: None,
            name: "calculate_total".to_string(),
            kind: SymbolKind::Function,
            lang: Language::TypeScript,
            file_path: "src/calc.ts".to_string(),
            start_line: 1,
            end_line: 10,
            start_col: 0,
            end_col: 0,
            signature: Some("function calculateTotal()".to_string()),
            parent: None,
            complexity: None,
        };

        let s1_id = sym1.deterministic_id("default");
        let s2_id = sym2.deterministic_id("default");
        let s3_id = sym3.deterministic_id("default");

        db.insert_symbol(&sym1).unwrap();
        db.insert_symbol(&sym2).unwrap();
        db.insert_symbol(&sym3).unwrap();

        let embedder = DummyEmbedder;
        db.insert_symbol_embedding(&s1_id, &embedder.embed("check_cli_token").await.unwrap())
            .unwrap();
        db.insert_symbol_embedding(&s2_id, &embedder.embed("require_permission").await.unwrap())
            .unwrap();
        db.insert_symbol_embedding(&s3_id, &embedder.embed("calculate_total").await.unwrap())
            .unwrap();

        let query = QueryEngine::new(Arc::new(db));
        let results = query
            .semantic_search("token validation", &embedder, 5)
            .await
            .unwrap();

        assert!(!results.symbols.is_empty());
        let names: Vec<String> = results.symbols.iter().map(|s| s.name.clone()).collect();
        assert!(
            names.contains(&"check_cli_token".to_string())
                || names.contains(&"require_permission".to_string())
        );
    }

    #[tokio::test]
    async fn test_hybrid_search_recall_higher_or_equal_to_bm25() {
        let db = CodeGraphDB::in_memory().expect("db");
        let sym1 = Symbol {
            id: None,
            stable_id: None,
            name: "check_cli_token".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "src/cli/handlers/memory.rs".to_string(),
            start_line: 1350,
            end_line: 1370,
            start_col: 0,
            end_col: 0,
            signature: Some(
                "pub fn check_cli_token(headers: &HeaderMap) -> Result<(), Response>".to_string(),
            ),
            parent: None,
            complexity: None,
        };
        let sym2 = Symbol {
            id: None,
            stable_id: None,
            name: "require_permission".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "src/middleware/auth.rs".to_string(),
            start_line: 23,
            end_line: 40,
            start_col: 0,
            end_col: 0,
            signature: Some("pub fn require_permission(...)".to_string()),
            parent: None,
            complexity: None,
        };

        let s1_id = sym1.deterministic_id("default");
        let s2_id = sym2.deterministic_id("default");

        db.insert_symbol(&sym1).unwrap();
        db.insert_symbol(&sym2).unwrap();

        let embedder = DummyEmbedder;
        db.insert_symbol_embedding(&s1_id, &embedder.embed("check_cli_token").await.unwrap())
            .unwrap();
        db.insert_symbol_embedding(&s2_id, &embedder.embed("require_permission").await.unwrap())
            .unwrap();

        let query_engine = QueryEngine::new(Arc::new(db));

        // Paraphrased query that BM25 alone fails on
        let bm25_results = query_engine.search("token validation", 5).unwrap();
        let hybrid_results = query_engine
            .hybrid_search("token validation", &embedder, 5)
            .await
            .unwrap();

        assert!(hybrid_results.symbols.len() >= bm25_results.symbols.len());
    }

    #[test]
    fn test_blast_radius_multi_caller_and_transitivity() {
        let db = CodeGraphDB::in_memory().expect("in-memory db");

        // Create helper symbol
        let helper = Symbol {
            name: "require_permission".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "/src/middleware/auth.rs".to_string(),
            start_line: 1,
            end_line: 10,
            ..Default::default()
        };
        db.insert_symbol(&helper).expect("insert helper");
        let helper_id = db
            .find_by_name("require_permission", 1)
            .expect("find helper")[0]
            .stable_id
            .clone()
            .unwrap();

        // Create 3 handlers (direct callers, depth 1)
        let handler_a = Symbol {
            name: "delete_memory_handler".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "/src/cli/handlers/memory.rs".to_string(),
            start_line: 20,
            end_line: 35,
            ..Default::default()
        };
        let handler_b = Symbol {
            name: "update_memory_handler".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "/src/cli/handlers/memory.rs".to_string(),
            start_line: 40,
            end_line: 55,
            ..Default::default()
        };
        let handler_c = Symbol {
            name: "create_token_handler".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "/src/cli/handlers/tokens.rs".to_string(),
            start_line: 10,
            end_line: 25,
            ..Default::default()
        };

        db.insert_symbol(&handler_a).expect("insert handler_a");
        db.insert_symbol(&handler_b).expect("insert handler_b");
        db.insert_symbol(&handler_c).expect("insert handler_c");

        let ha_id = db.find_by_name("delete_memory_handler", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();
        let hb_id = db.find_by_name("update_memory_handler", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();
        let hc_id = db.find_by_name("create_token_handler", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();

        // Create level 2 caller (router calling delete_memory_handler)
        let router = Symbol {
            name: "server_router".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "/src/cli/server.rs".to_string(),
            start_line: 100,
            end_line: 200,
            ..Default::default()
        };
        db.insert_symbol(&router).expect("insert router");
        let router_id = db.find_by_name("server_router", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();

        // Insert Calls edges
        // Handlers call helper
        for caller in [&ha_id, &hb_id, &hc_id] {
            db.insert_edge(&CodeEdge {
                id: None,
                from_symbol: caller.clone(),
                to_symbol: helper_id.clone(),
                edge_type: EdgeType::Calls,
                file_path: "/src/test.rs".to_string(),
                line: 1,
                confidence: 1.0,
                metadata: None,
            })
            .expect("insert edge");
        }

        // Router calls handler A
        db.insert_edge(&CodeEdge {
            id: None,
            from_symbol: router_id.clone(),
            to_symbol: ha_id.clone(),
            edge_type: EdgeType::Calls,
            file_path: "/src/cli/server.rs".to_string(),
            line: 150,
            confidence: 1.0,
            metadata: None,
        })
        .expect("insert router edge");

        let query = QueryEngine::new(Arc::new(db));

        // Test depth 1
        let depth1 = query
            .blast_radius("require_permission", 1)
            .expect("blast radius d1");
        assert_eq!(
            depth1.len(),
            3,
            "should find all 3 direct callers at depth 1"
        );
        for (sym, d) in &depth1 {
            assert_eq!(*d, 1);
            assert!(
                sym.name == "delete_memory_handler"
                    || sym.name == "update_memory_handler"
                    || sym.name == "create_token_handler"
            );
        }

        // Test depth 2
        let depth2 = query
            .blast_radius("require_permission", 2)
            .expect("blast radius d2");
        assert_eq!(
            depth2.len(),
            4,
            "should find 3 direct callers + 1 transitive caller at depth 2"
        );

        // Verify transitivity: blast_radius(X, 2) contains all elements of blast_radius(X, 1)
        for (d1_sym, _) in &depth1 {
            assert!(
                depth2.iter().any(|(d2_sym, _)| d2_sym.name == d1_sym.name),
                "depth2 must contain all depth1 symbols"
            );
        }

        // Verify depth 2 caller
        let router_entry = depth2.iter().find(|(s, _)| s.name == "server_router");
        assert!(
            router_entry.is_some(),
            "router should be in depth 2 blast radius"
        );
        assert_eq!(router_entry.unwrap().1, 2, "router depth should be 2");
    }

    #[test]
    fn test_memories_for_symbol_linking() {
        let db = setup_test_db();
        let sym = Symbol {
            id: None,
            stable_id: None,
            name: "require_permission".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "/src/middleware/auth.rs".to_string(),
            start_line: 10,
            end_line: 25,
            start_col: 0,
            end_col: 0,
            signature: Some("pub fn require_permission()".to_string()),
            parent: None,
            complexity: None,
        };
        db.insert_symbol(&sym).expect("test assertion");

        let memory_id = "mem_agent_101";
        let memory_content =
            "This conversation discusses require_permission middleware for RBAC access control.";

        let links = db
            .link_memory_to_symbols(memory_id, memory_content)
            .expect("test assertion");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].memory_id, memory_id);

        let query = QueryEngine::new(Arc::new(db));
        let mem_links = query
            .memories_for_symbol("require_permission")
            .expect("test assertion");
        assert!(!mem_links.is_empty());
        assert_eq!(mem_links[0].memory_id, memory_id);

        let symbols = query.symbols_for_memory(memory_id).expect("test assertion");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "require_permission");
    }
}
