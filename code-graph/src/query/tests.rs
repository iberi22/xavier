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
    fn test_c_symbol_and_relation_mapping() {
        use crate::query::{CSymbol, CRelation};

        let c_sym = CSymbol {
            uuid: "c-uuid-1".to_string(),
            name: "calculate_area".to_string(),
            c_kind: "function_definition".to_string(),
            filepath: "geometry.c".to_string(),
            start_line: 12,
            end_line: 25,
            start_col: 4,
            end_col: 20,
            signature_text: Some("double calculate_area(double r)".to_string()),
            parent_scope: Some("geometry".to_string()),
            cyclomatic_complexity: Some(4.0),
        };

        let rust_sym = c_sym.to_rust_symbol();
        assert_eq!(rust_sym.stable_id, Some("c-uuid-1".to_string()));
        assert_eq!(rust_sym.name, "calculate_area");
        assert_eq!(rust_sym.kind, SymbolKind::Function);
        assert_eq!(rust_sym.lang, Language::C);
        assert_eq!(rust_sym.file_path, "geometry.c");
        assert_eq!(rust_sym.start_line, 12);
        assert_eq!(rust_sym.end_line, 25);
        assert_eq!(rust_sym.start_col, 4);
        assert_eq!(rust_sym.end_col, 20);
        assert_eq!(rust_sym.signature, Some("double calculate_area(double r)".to_string()));
        assert_eq!(rust_sym.parent, Some("geometry".to_string()));
        assert_eq!(rust_sym.complexity, Some(4.0));

        let c_rel = CRelation {
            from_uuid: "c-uuid-1".to_string(),
            to_uuid: "c-uuid-2".to_string(),
            rel_type: "PointsTo".to_string(),
            file_path: "geometry.c".to_string(),
            line_num: 15,
            weight: 0.85,
        };

        let rust_edge = c_rel.to_rust_edge();
        assert_eq!(rust_edge.from_symbol, "c-uuid-1");
        assert_eq!(rust_edge.to_symbol, "c-uuid-2");
        assert_eq!(rust_edge.edge_type, EdgeType::PointsTo);
        assert_eq!(rust_edge.file_path, "geometry.c");
        assert_eq!(rust_edge.line, 15);
        assert_eq!(rust_edge.confidence, 0.85);
    }

    #[test]
    fn test_c_query_bridge_and_engine() {
        use crate::query::{CSymbol, CRelation, CQueryBridge, QueryEngine};

        let c_symbols = vec![
            CSymbol {
                uuid: "sym-1".to_string(),
                name: "init_system".to_string(),
                c_kind: "function_definition".to_string(),
                filepath: "main.c".to_string(),
                start_line: 1,
                end_line: 10,
                start_col: 0,
                end_col: 0,
                signature_text: Some("void init_system()".to_string()),
                parent_scope: None,
                cyclomatic_complexity: Some(2.0),
            },
            CSymbol {
                uuid: "sym-2".to_string(),
                name: "CONFIG_MAX".to_string(),
                c_kind: "preproc_def".to_string(),
                filepath: "config.h".to_string(),
                start_line: 5,
                end_line: 5,
                start_col: 0,
                end_col: 0,
                signature_text: None,
                parent_scope: None,
                cyclomatic_complexity: None,
            },
        ];

        let c_relations = vec![
            CRelation {
                from_uuid: "sym-1".to_string(),
                to_uuid: "sym-2".to_string(),
                rel_type: "MacroExpansion".to_string(),
                file_path: "main.c".to_string(),
                line_num: 3,
                weight: 1.0,
            }
        ];

        let c_bridge = CQueryBridge::new(c_symbols, c_relations);
        let dummy_db = Arc::new(CodeGraphDB::in_memory().unwrap());
        let engine = QueryEngine::with_bridge(dummy_db, Box::new(c_bridge));

        // Test search
        let res = engine.search("init", 10).expect("search");
        assert_eq!(res.total, 1);
        assert_eq!(res.symbols[0].stable_id.as_deref(), Some("sym-1"));

        // Test in_file
        let file_symbols = engine.in_file("main.c").expect("in_file");
        assert_eq!(file_symbols.len(), 1);
        assert_eq!(file_symbols[0].name, "init_system");

        // Test dependencies/relations
        let deps = engine.dependencies("init_system", Some(EdgeType::MacroExpansion), 1, 10).expect("deps");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].to_symbol, "sym-2");
        assert_eq!(deps[0].edge_type, EdgeType::MacroExpansion);

        // Test stats
        let stats = engine.stats().expect("stats");
        assert_eq!(stats.total_symbols, 2);
        assert_eq!(stats.total_files, 2); // main.c and config.h

        // Test hubs
        let hubs = engine.hubs(1, 10).expect("hubs");
        assert_eq!(hubs.len(), 2);
    }
}
