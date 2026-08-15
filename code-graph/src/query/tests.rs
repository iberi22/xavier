//! Tests for code-graph query engine

#[cfg(test)]
mod tests_inner {
    use crate::db::CodeGraphDB;
    use crate::query::QueryEngine;
    use crate::types::{CodeEdge, EdgeType, Language, Symbol, SymbolKind};
    use std::sync::Arc;

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
        let depth1 = query.blast_radius("require_permission", 1).expect("blast radius d1");
        assert_eq!(depth1.len(), 3, "should find all 3 direct callers at depth 1");
        for (sym, d) in &depth1 {
            assert_eq!(*d, 1);
            assert!(
                sym.name == "delete_memory_handler"
                    || sym.name == "update_memory_handler"
                    || sym.name == "create_token_handler"
            );
        }

        // Test depth 2
        let depth2 = query.blast_radius("require_permission", 2).expect("blast radius d2");
        assert_eq!(depth2.len(), 4, "should find 3 direct callers + 1 transitive caller at depth 2");

        // Verify transitivity: blast_radius(X, 2) contains all elements of blast_radius(X, 1)
        for (d1_sym, _) in &depth1 {
            assert!(
                depth2.iter().any(|(d2_sym, _)| d2_sym.name == d1_sym.name),
                "depth2 must contain all depth1 symbols"
            );
        }

        // Verify depth 2 caller
        let router_entry = depth2.iter().find(|(s, _)| s.name == "server_router");
        assert!(router_entry.is_some(), "router should be in depth 2 blast radius");
        assert_eq!(router_entry.unwrap().1, 2, "router depth should be 2");
    }
}
