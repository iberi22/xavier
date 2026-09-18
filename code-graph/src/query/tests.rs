//! Tests for code-graph query engine

#[cfg(test)]
mod tests_inner {
    use crate::db::CodeGraphDB;
    use crate::error::GraphError;
    use crate::query::QueryEngine;
    use crate::query::RouteMode;
    use crate::query::{
        compare_contract, ContractStatus, DeleteRisk, RationaleKind, VerifyVerdict,
    };
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
    fn test_blast_radius_unknown_symbol_refuses_with_suggestions() {
        // O1 (US-101): an unknown selector must refuse with suggestions,
        // never return a silent empty vec (ripwire R1 refuse-vs-empty).
        let db = CodeGraphDB::in_memory().expect("in-memory db");
        let known = Symbol {
            name: "require_permission".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "/src/middleware/auth.rs".to_string(),
            start_line: 1,
            end_line: 10,
            ..Default::default()
        };
        db.insert_symbol(&known).expect("insert known");

        let query = QueryEngine::new(Arc::new(db));
        let err = query
            .blast_radius("require_permisson", 2)
            .expect_err("unknown symbol must refuse, not return empty");
        match err {
            GraphError::UnknownSymbol { name, suggestions } => {
                assert_eq!(name, "require_permisson");
                assert!(
                    suggestions.contains(&"require_permission".to_string()),
                    "should suggest the known symbol, got {suggestions:?}"
                );
            }
            other => panic!("expected UnknownSymbol refusal, got {other:?}"),
        }
    }

    fn o3_fixture_db() -> CodeGraphDB {
        // target_fn <- caller_fn (Calls), <- importer_mod (Imports),
        // <- reader_fn (References); test_it (tests/) and user_fn call target_fn.
        let db = CodeGraphDB::in_memory().expect("in-memory db");
        let mk = |name: &str, file: &str| Symbol {
            name: name.to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: file.to_string(),
            start_line: 1,
            end_line: 5,
            ..Default::default()
        };
        for (n, f) in [
            ("target_fn", "/src/lib.rs"),
            ("caller_fn", "/src/main.rs"),
            ("importer_mod", "/src/other.rs"),
            ("reader_fn", "/src/reader.rs"),
            ("test_it", "/tests/lib_test.rs"),
            ("user_fn", "/src/app.rs"),
            ("lonely_fn", "/src/lonely.rs"),
        ] {
            db.insert_symbol(&mk(n, f)).expect("insert symbol");
        }
        let id = |n: &str| db.find_by_name(n, 1).unwrap()[0].stable_id.clone().unwrap();
        let edge = |from: &str, to: &str, ty: EdgeType| {
            db.insert_edge(&CodeEdge {
                id: None,
                from_symbol: from.to_string(),
                to_symbol: to.to_string(),
                edge_type: ty,
                file_path: "/src/main.rs".to_string(),
                line: 1,
                confidence: 1.0,
                metadata: None,
            })
            .expect("insert edge")
        };
        let (t, c, i, r, test, user, lonely) = (
            id("target_fn"),
            id("caller_fn"),
            id("importer_mod"),
            id("reader_fn"),
            id("test_it"),
            id("user_fn"),
            id("lonely_fn"),
        );
        edge(&c, &t, EdgeType::Calls);
        edge(&i, &t, EdgeType::Imports);
        edge(&r, &t, EdgeType::References);
        edge(&test, &t, EdgeType::Calls);
        edge(&user, &t, EdgeType::Calls);
        edge(&user, &lonely, EdgeType::Calls);
        db
    }

    #[test]
    fn test_impact_roles_distinguish_call_import_reference() {
        // O3 (US-105, ripwire R2 --uses): every reach names its role.
        let db = o3_fixture_db();
        let query = QueryEngine::new(Arc::new(db));
        let report = query.blast_radius_ex("target_fn", 2).expect("report");
        let role_of = |n: &str| {
            report
                .hits
                .iter()
                .find(|h| h.symbol.name == n)
                .map(|h| h.edge.clone())
        };
        assert_eq!(role_of("caller_fn"), Some(EdgeType::Calls));
        assert_eq!(role_of("importer_mod"), Some(EdgeType::Imports));
        assert_eq!(role_of("reader_fn"), Some(EdgeType::References));
        // Call-graph answers are floors (O1): dynamic dispatch is invisible.
        assert!(report.meta.floor);
        assert!(report.meta.invariant_holds());
        // Edge confidence rides along classified (O1 rubric).
        let call = report
            .hits
            .iter()
            .find(|h| h.symbol.name == "caller_fn")
            .unwrap();
        assert_eq!(call.confidence.tag(), "EXTRACTED");
    }

    #[test]
    fn test_tests_for_finds_covering_tests() {
        // O3 (US-105, ripwire R2 --affected): only test files count.
        let db = o3_fixture_db();
        let query = QueryEngine::new(Arc::new(db));
        let tests = query.tests_for("target_fn", 2).expect("tests");
        let names: Vec<_> = tests.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["test_it"]);
    }

    #[test]
    fn test_testgate_fails_on_untested_radius() {
        // O3 (US-106, ripwire R2 --test-gate): obligation iff radius
        // contains a non-test symbol no test reaches.
        let db = o3_fixture_db();
        let query = QueryEngine::new(Arc::new(db));
        let gate = query
            .test_gate(&["target_fn", "lonely_fn"], 2)
            .expect("gate");
        assert!(gate.tests_to_run.iter().any(|s| s.name == "test_it"));
        // user_fn reaches lonely_fn but no test reaches user_fn.
        assert!(gate.untested.iter().any(|h| h.symbol.name == "user_fn"));
        assert!(gate.has_obligation());

        let clean = query.test_gate(&["target_fn"], 1).expect("gate");
        assert!(!clean.has_obligation());
    }

    #[test]
    fn test_gate_tests_carry_real_file_paths() {
        // O3 honesty: the gate names real test files; runner commands are
        // CLI-level and never invented here (ripwire `run=` rule).
        let db = o3_fixture_db();
        let query = QueryEngine::new(Arc::new(db));
        let gate = query.test_gate(&["target_fn"], 2).expect("gate");
        assert!(!gate.tests_to_run.is_empty());
        for t in &gate.tests_to_run {
            assert!(
                t.file_path.contains("/tests/") || t.file_path.contains("/test/"),
                "test must carry its real file path, got {}",
                t.file_path
            );
        }
    }

    #[test]
    fn test_blast_report_declares_est_tokens() {
        // O5: every answer declares its wire cost (never silently zero).
        let db = o3_fixture_db();
        let query = QueryEngine::new(Arc::new(db));
        let report = query.blast_radius_ex("target_fn", 2).expect("report");
        let est = report.meta.est_tokens.expect("est_tokens declared");
        assert!(est > 0);
    }

    fn o6_star_db() -> CodeGraphDB {
        // Hub called by 4 leaves; leaves live in distinct files.
        let db = CodeGraphDB::in_memory().expect("in-memory db");
        let mk = |name: &str, file: &str| Symbol {
            name: name.to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: file.to_string(),
            start_line: 1,
            end_line: 5,
            ..Default::default()
        };
        db.insert_symbol(&mk("hub_core", "/src/hub.rs"))
            .expect("hub");
        for leaf in ["leaf_a", "leaf_b", "leaf_c", "leaf_d"] {
            db.insert_symbol(&mk(leaf, &format!("/src/{}.rs", leaf)))
                .expect("leaf");
        }
        let hub_id = db.find_by_name("hub_core", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();
        for leaf in ["leaf_a", "leaf_b", "leaf_c", "leaf_d"] {
            let leaf_id = db.find_by_name(leaf, 1).unwrap()[0]
                .stable_id
                .clone()
                .unwrap();
            db.insert_edge(&CodeEdge {
                id: None,
                from_symbol: leaf_id,
                to_symbol: hub_id.clone(),
                edge_type: EdgeType::Calls,
                file_path: format!("/src/{}.rs", leaf),
                line: 2,
                confidence: 1.0,
                metadata: None,
            })
            .expect("edge");
        }
        db
    }

    #[test]
    fn test_route_query_exact_name_is_confident() {
        // O6 router: an exact name hit answers directly (confidence 100).
        let db = o6_star_db();
        let query = QueryEngine::new(Arc::new(db));
        let report = query.route_query("hub_core", 10).expect("route");
        assert_eq!(report.mode, RouteMode::ExactName);
        assert!(report.confident);
        assert!(report.hits.iter().any(|h| h.symbol.name == "hub_core"));
        assert!(report.meta.est_tokens.expect("cost declared") > 0);
    }

    #[test]
    fn test_route_query_conceptual_ranks_by_graph() {
        // O6 router: no exact hit → FTS candidates ordered by centrality.
        let db = o6_star_db();
        let query = QueryEngine::new(Arc::new(db));
        let report = query.route_query("leaf", 10).expect("route");
        assert_eq!(report.mode, RouteMode::GraphRanked);
        assert!(!report.hits.is_empty());
        // Scores descend with the hits.
        let scores: Vec<f64> = report.hits.iter().map(|h| h.score).collect();
        let mut sorted = scores.clone();
        sorted.sort_by(|a, b| b.partial_cmp(a).unwrap());
        assert_eq!(scores, sorted);
    }

    #[test]
    fn test_route_query_unknown_refuses() {
        // O6 + O1: unresolvable queries refuse, never return noise.
        let db = o6_star_db();
        let query = QueryEngine::new(Arc::new(db));
        let err = query
            .route_query("zzz_no_such_symbol", 10)
            .expect_err("unknown must refuse");
        assert!(
            matches!(err, GraphError::UnknownSymbol { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn test_top_central_symbols_names_the_hub() {
        // O6: structural centrality finds the hub (feeds O7 god-nodes).
        let db = o6_star_db();
        let query = QueryEngine::new(Arc::new(db));
        let top = query.top_central_symbols(2).expect("top");
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].symbol.name, "hub_core");
        assert!(top[0].score >= top[1].score);
    }

    // ---- O7 (feat-cg-contracts-arch) ----

    fn o7_sym(name: &str, file: &str, sig: Option<&str>, start: u32) -> Symbol {
        Symbol {
            name: name.to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: file.to_string(),
            start_line: start,
            end_line: start + 5,
            signature: sig.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn test_edit_check_contract_change_detects_arity_break() {
        // O7 (US-113, ripwire R5 --edit-check): fingerprint covers arity.
        let old = o7_sym("run", "/src/a.rs", Some("pub fn run(a: i32)"), 1);
        let same = o7_sym("run", "/src/a.rs", Some("pub fn run(a : i32 )"), 1);
        let broke = o7_sym("run", "/src/a.rs", Some("pub fn run(a: i32, b: i32)"), 1);
        assert_eq!(compare_contract(&old, &same), ContractStatus::Unchanged);
        assert!(matches!(
            compare_contract(&old, &broke),
            ContractStatus::ContractChange { .. }
        ));
        assert_eq!(
            compare_contract(&old, &o7_sym("new_fn", "/src/a.rs", None, 20)),
            ContractStatus::NewSymbol
        );
    }

    #[test]
    fn test_safe_delete_names_risk_never_verdict() {
        // O7 (ripwire R5 --safe-delete): risk is named, never a go/no-go.
        let db = CodeGraphDB::in_memory().expect("in-memory db");
        db.insert_symbol(&o7_sym("victim", "/src/a.rs", None, 1))
            .expect("victim");
        db.insert_symbol(&o7_sym("user_fn", "/src/b.rs", None, 1))
            .expect("user");
        db.insert_symbol(&o7_sym("lonely_fn", "/src/c.rs", None, 1))
            .expect("lonely");
        let vid = db.find_by_name("victim", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();
        let uid = db.find_by_name("user_fn", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();
        db.insert_edge(&CodeEdge {
            id: None,
            from_symbol: uid,
            to_symbol: vid,
            edge_type: EdgeType::Calls,
            file_path: "/src/b.rs".to_string(),
            line: 2,
            confidence: 1.0,
            metadata: None,
        })
        .expect("edge");
        let query = QueryEngine::new(Arc::new(db));
        let used = query.safe_delete("victim", 3).expect("report");
        assert_eq!(used.impact_reaches, 1);
        assert!(matches!(
            used.risk,
            DeleteRisk::UntestedRadius | DeleteRisk::UsesExist
        ));
        let lonely = query.safe_delete("lonely_fn", 3).expect("report");
        assert_eq!(lonely.risk, DeleteRisk::NoneFound);
        assert_eq!(lonely.impact_reaches, 0);
    }

    #[test]
    fn test_verify_three_valued_with_evidence() {
        // O7 (ripwire R5 --verify): confirmed / refuted-complete /
        // not-established. Zero graph evidence never refutes.
        let db = CodeGraphDB::in_memory().expect("in-memory db");
        db.insert_symbol(&o7_sym("caller_a", "/src/a.rs", None, 1))
            .expect("a");
        db.insert_symbol(&o7_sym("callee_b", "/src/a.rs", None, 10))
            .expect("b");
        db.insert_symbol(&o7_sym("other_c", "/src/a.rs", None, 20))
            .expect("c");
        let aid = db.find_by_name("caller_a", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();
        let bid = db.find_by_name("callee_b", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();
        db.insert_edge(&CodeEdge {
            id: None,
            from_symbol: aid,
            to_symbol: bid,
            edge_type: EdgeType::Calls,
            file_path: "/src/a.rs".to_string(),
            line: 2,
            confidence: 0.9,
            metadata: None,
        })
        .expect("edge");
        db.batch_upsert_file_metadata([("/src/a.rs".to_string(), 1i64)].into())
            .expect("meta");
        let query = QueryEngine::new(Arc::new(db));
        match query.verify_calls("caller_a", "callee_b").expect("v") {
            VerifyVerdict::Confirmed { evidence } => {
                assert!(evidence.contains("/src/a.rs"), "got {evidence}")
            }
            other => panic!("expected Confirmed, got {other:?}"),
        }
        match query.verify_calls("caller_a", "other_c").expect("v") {
            VerifyVerdict::NotEstablished { limit } => {
                assert!(limit.contains("floor"), "got {limit}")
            }
            other => panic!("zero graph evidence must not refute, got {other:?}"),
        }
        match query.verify_defines("/src/a.rs", "callee_b").expect("v") {
            VerifyVerdict::Confirmed { .. } => {}
            other => panic!("expected Confirmed, got {other:?}"),
        }
        match query.verify_defines("/src/a.rs", "ghost_fn").expect("v") {
            VerifyVerdict::Refuted { complete } => assert!(complete),
            other => panic!("expected complete Refuted, got {other:?}"),
        }
        match query
            .verify_defines("/src/never_indexed.rs", "x")
            .expect("v")
        {
            VerifyVerdict::NotEstablished { .. } => {}
            other => panic!("expected NotEstablished, got {other:?}"),
        }
    }

    #[test]
    fn test_god_nodes_exclude_builtins() {
        // O7 (G7 + O2): globals are never architecture signals.
        let db = CodeGraphDB::in_memory().expect("in-memory db");
        let mut hub = o7_sym("hub_fn", "/src/hub.rs", None, 1);
        hub.lang = Language::Rust;
        let mut print_sym = o7_sym("print", "/src/app.py", None, 1);
        print_sym.lang = Language::Python;
        db.insert_symbol(&hub).expect("hub");
        db.insert_symbol(&print_sym).expect("print");
        let hid = db.find_by_name("hub_fn", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();
        let pid = db.find_by_name("print", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();
        for i in 0..3 {
            let c = o7_sym(&format!("caller_{}", i), "/src/c.rs", None, 1);
            db.insert_symbol(&c).expect("caller");
            let cid = db.find_by_name(&format!("caller_{}", i), 1).unwrap()[0]
                .stable_id
                .clone()
                .unwrap();
            db.insert_edge(&CodeEdge {
                id: None,
                from_symbol: cid,
                to_symbol: hid.clone(),
                edge_type: EdgeType::Calls,
                file_path: "/src/c.rs".to_string(),
                line: 2,
                confidence: 1.0,
                metadata: None,
            })
            .expect("edge");
        }
        for i in 0..5 {
            let c = o7_sym(&format!("pcaller_{}", i), "/src/p.py", None, 1);
            db.insert_symbol(&c).expect("pcaller");
            let cid = db.find_by_name(&format!("pcaller_{}", i), 1).unwrap()[0]
                .stable_id
                .clone()
                .unwrap();
            db.insert_edge(&CodeEdge {
                id: None,
                from_symbol: cid,
                to_symbol: pid.clone(),
                edge_type: EdgeType::Calls,
                file_path: "/src/p.py".to_string(),
                line: 2,
                confidence: 1.0,
                metadata: None,
            })
            .expect("edge");
        }
        let query = QueryEngine::new(Arc::new(db));
        let gods = query.god_nodes(5).expect("gods");
        assert!(!gods.is_empty());
        assert_eq!(gods[0].symbol.name, "hub_fn");
        assert!(gods.iter().all(|g| g.symbol.name != "print"));
    }

    #[test]
    fn test_call_cycles_found_length_bounded() {
        // O7 (G7 import-cycles, honest file-level form): a↔b reported,
        // self-calls excluded, bound respected.
        let db = CodeGraphDB::in_memory().expect("in-memory db");
        db.insert_symbol(&o7_sym("fa", "/src/a.rs", None, 1))
            .expect("fa");
        db.insert_symbol(&o7_sym("fb", "/src/b.rs", None, 1))
            .expect("fb");
        db.insert_symbol(&o7_sym("fc", "/src/c.rs", None, 1))
            .expect("fc");
        let aid = db.find_by_name("fa", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();
        let bid = db.find_by_name("fb", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();
        let cid = db.find_by_name("fc", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();
        let edge = |from: &str, to: &str, file: &str| {
            db.insert_edge(&CodeEdge {
                id: None,
                from_symbol: from.to_string(),
                to_symbol: to.to_string(),
                edge_type: EdgeType::Calls,
                file_path: file.to_string(),
                line: 2,
                confidence: 1.0,
                metadata: None,
            })
            .expect("edge")
        };
        edge(&aid, &bid, "/src/a.rs");
        edge(&bid, &aid, "/src/b.rs");
        edge(&aid, &aid, "/src/a.rs");
        edge(&bid, &cid, "/src/b.rs");
        let query = QueryEngine::new(Arc::new(db));
        let cycles = query.call_cycles(4).expect("cycles");
        assert!(
            cycles
                .iter()
                .any(|c| c.contains(&"/src/a.rs".to_string())
                    && c.contains(&"/src/b.rs".to_string())),
            "a↔b cycle missing: {cycles:?}"
        );
        assert!(
            cycles.iter().all(|c| c.len() <= 4),
            "bound respected: {cycles:?}"
        );
    }

    #[test]
    fn test_rationale_note_linked_to_symbol() {
        // O7 (G7 rationale): NOTE/WHY/HACK comments link to enclosing symbol.
        let dir = tempfile::TempDir::new().expect("temp dir");
        let content = "fn guarded() {\n    // NOTE: retry is intentional here\n    run();\n}\n";
        std::fs::write(dir.path().join("lib.rs"), content).expect("write");
        let db = CodeGraphDB::in_memory().expect("in-memory db");
        db.insert_symbol(&o7_sym("guarded", "lib.rs", None, 1))
            .expect("sym");
        db.insert_symbol(&o7_sym("run", "lib.rs", None, 10))
            .expect("run");
        let query = QueryEngine::new(Arc::new(db));
        let notes = query
            .rationale_for(dir.path(), "guarded")
            .expect("rationale");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].tag, RationaleKind::Note);
        assert_eq!(notes[0].symbol.as_deref(), Some("guarded"));
        assert!(notes[0].text.contains("retry"));
    }

    #[test]
    fn test_call_cycles_cap_total_output() {
        // Dogfood fix: dense file graphs explode combinatorially (219k
        // cycles seen live). Total output is capped, shortest first.
        let db = CodeGraphDB::in_memory().expect("in-memory db");
        for i in 0..8 {
            db.insert_symbol(&o7_sym(
                &format!("kf{}", i),
                &format!("/src/k{}.rs", i),
                None,
                1,
            ))
            .expect("sym");
        }
        let ids: Vec<String> = (0..8)
            .map(|i| {
                db.find_by_name(&format!("kf{}", i), 1).unwrap()[0]
                    .stable_id
                    .clone()
                    .unwrap()
            })
            .collect();
        for (a, aid) in ids.iter().enumerate() {
            for (b, bid) in ids.iter().enumerate() {
                if a == b {
                    continue;
                }
                db.insert_edge(&CodeEdge {
                    id: None,
                    from_symbol: aid.clone(),
                    to_symbol: bid.clone(),
                    edge_type: EdgeType::Calls,
                    file_path: format!("/src/k{}.rs", a),
                    line: 2,
                    confidence: 1.0,
                    metadata: None,
                })
                .expect("edge");
            }
        }
        let query = QueryEngine::new(Arc::new(db));
        let cycles = query.call_cycles(6).expect("cycles");
        assert!(!cycles.is_empty());
        assert!(
            cycles.len() <= 512,
            "total cycles capped, got {}",
            cycles.len()
        );
        assert!(cycles.iter().all(|c| c.len() <= 6));
        // Shortest first: a 2-cycle leads.
        assert_eq!(cycles[0].len(), 2);
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

    #[test]
    fn test_search_narrow_returns_only_matching_names() {
        let db = CodeGraphDB::in_memory().expect("in-memory db");

        let sym1 = Symbol {
            name: "useXavierMemory".to_string(),
            kind: SymbolKind::Function,
            lang: Language::TypeScript,
            file_path: "src/hooks/useXavierMemory.ts".to_string(),
            start_line: 1,
            end_line: 20,
            ..Default::default()
        };
        let sym2 = Symbol {
            name: "useAuth".to_string(),
            kind: SymbolKind::Function,
            lang: Language::TypeScript,
            file_path: "src/hooks/useAuth.ts".to_string(),
            start_line: 1,
            end_line: 15,
            ..Default::default()
        };
        let sym3 = Symbol {
            name: "fs/promises".to_string(),
            kind: SymbolKind::Import,
            lang: Language::TypeScript,
            file_path: "src/file.ts".to_string(),
            start_line: 1,
            end_line: 1,
            ..Default::default()
        };

        db.insert_symbol(&sym1).expect("insert sym1");
        db.insert_symbol(&sym2).expect("insert sym2");
        db.insert_symbol(&sym3).expect("insert sym3");

        let query = QueryEngine::new(Arc::new(db));
        let res = query.search("useXavier", 10).expect("search useXavier");

        assert_eq!(res.symbols.len(), 1);
        assert_eq!(res.symbols[0].name, "useXavierMemory");

        let lang_res = query
            .by_language(Language::TypeScript, 10)
            .expect("by_language");
        assert_eq!(lang_res.len(), 3);
    }
}
