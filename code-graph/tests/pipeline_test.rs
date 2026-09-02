use code_graph::db::CodeGraphDB;
use code_graph::indexer::Indexer;
use code_graph::parser::parse_source;
use code_graph::plugin::types::{FallbackStep, PluginDescriptor};
use code_graph::plugin::PluginManager;
use code_graph::query::QueryEngine;
use code_graph::types::{Language, SymbolKind};
use std::path::Path;
use std::sync::Arc;

#[tokio::test]
async fn test_c_parser_pipeline() {
    let temp_dir = tempfile::tempdir().unwrap();
    let fixture_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("pipeline_test")
        .join("c_test.c");
    let dest = temp_dir.path().join("c_test.c");
    std::fs::copy(&fixture_src, &dest).unwrap();

    let db = Arc::new(CodeGraphDB::in_memory().unwrap());
    let indexer = Indexer::new(db.clone());

    let stats = indexer.index(temp_dir.path(), false).await.unwrap();
    assert_eq!(stats.total_files, 1);
    assert!(stats.total_symbols >= 1);

    // Verify symbols returned in DB
    let symbols = db.get_all_symbols().unwrap();
    assert!(symbols
        .iter()
        .any(|s| s.name == "calculate_area" && s.kind == SymbolKind::Function));
    assert!(symbols
        .iter()
        .any(|s| s.name == "Point" && s.kind == SymbolKind::Struct));
    assert!(symbols
        .iter()
        .any(|s| s.name == "LIMIT" && s.kind == SymbolKind::Constant));

    // Verify query runs on indexed symbols
    let query_engine = QueryEngine::new(db.clone());
    let res = query_engine.search("calculate_area", 10).unwrap();
    assert!(!res.symbols.is_empty());
    assert_eq!(res.symbols[0].name, "calculate_area");
}

#[tokio::test]
async fn test_rust_parser_pipeline() {
    let temp_dir = tempfile::tempdir().unwrap();
    let fixture_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("pipeline_test")
        .join("rust_test.rs");
    let dest = temp_dir.path().join("rust_test.rs");
    std::fs::copy(&fixture_src, &dest).unwrap();

    let db = Arc::new(CodeGraphDB::in_memory().unwrap());
    let indexer = Indexer::new(db.clone());

    let stats = indexer.index(temp_dir.path(), false).await.unwrap();
    assert_eq!(stats.total_files, 1);
    assert!(stats.total_symbols >= 1);

    // Verify symbols returned in DB
    let symbols = db.get_all_symbols().unwrap();
    assert!(symbols
        .iter()
        .any(|s| s.name == "start_server" && s.kind == SymbolKind::Function));
    assert!(symbols
        .iter()
        .any(|s| s.name == "Config" && s.kind == SymbolKind::Struct));

    // Verify query runs on indexed symbols
    let query_engine = QueryEngine::new(db.clone());
    let res = query_engine.search("start_server", 10).unwrap();
    assert!(!res.symbols.is_empty());
    assert_eq!(res.symbols[0].name, "start_server");
}

#[tokio::test]
async fn test_python_parser_pipeline() {
    let temp_dir = tempfile::tempdir().unwrap();
    let fixture_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("pipeline_test")
        .join("python_test.py");
    let dest = temp_dir.path().join("python_test.py");
    std::fs::copy(&fixture_src, &dest).unwrap();

    let db = Arc::new(CodeGraphDB::in_memory().unwrap());
    let indexer = Indexer::new(db.clone());

    let stats = indexer.index(temp_dir.path(), false).await.unwrap();
    assert_eq!(stats.total_files, 1);
    assert!(stats.total_symbols >= 1);

    // Verify symbols returned in DB
    let symbols = db.get_all_symbols().unwrap();
    assert!(symbols
        .iter()
        .any(|s| s.name == "Calculator" && s.kind == SymbolKind::Class));
    assert!(symbols
        .iter()
        .any(|s| s.name == "add" && s.kind == SymbolKind::Method));
    assert!(symbols
        .iter()
        .any(|s| s.name == "calculate_pi" && s.kind == SymbolKind::Function));

    // Verify query runs on indexed symbols
    let query_engine = QueryEngine::new(db.clone());
    let res = query_engine.search("Calculator", 10).unwrap();
    assert!(!res.symbols.is_empty());
    assert_eq!(res.symbols[0].name, "Calculator");
}

#[tokio::test]
async fn test_plugin_fallback() {
    let manager = PluginManager::new();

    // 1. Register a mock crashing plugin for Rust
    // By pointing command to a non-existent binary, trying to execute it will result in std::io::Error/GraphError, causing ProcessEngine::parse to return Err
    manager.register(PluginDescriptor {
        name: "crashing-plugin".to_string(),
        version: "1.0.0".to_string(),
        command: "does_not_exist_and_will_fail_to_execute_anywhere".to_string(),
        languages: vec![Language::Rust],
        extensions: vec!["rs".to_string()],
        capabilities: vec!["parse".to_string()],
    });

    // Verify the fallback chain is indeed: Plugin("crashing-plugin") -> Native -> NoOp
    let chain = manager.chain_for(&Language::Rust);
    assert_eq!(chain.len(), 3);
    assert!(matches!(&chain[0], FallbackStep::Plugin(ref name) if name == "crashing-plugin"));
    assert!(matches!(&chain[1], FallbackStep::Native));
    assert!(matches!(&chain[2], FallbackStep::NoOp));

    // 2. Read the rust fixture file
    let fixture_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("pipeline_test")
        .join("rust_test.rs");
    let source = std::fs::read_to_string(&fixture_src).unwrap();

    // 3. Parse with parse_source, passing the plugin manager
    let symbols = parse_source(&source, &Language::Rust, "rust_test.rs", Some(&manager))
        .await
        .expect("should fall back gracefully and succeed");

    // 4. Assert that symbols are successfully returned by the native fallback parser
    assert!(!symbols.is_empty());
    assert!(symbols
        .iter()
        .any(|s| s.name == "start_server" && s.kind == SymbolKind::Function));
    assert!(symbols
        .iter()
        .any(|s| s.name == "Config" && s.kind == SymbolKind::Struct));
}

#[tokio::test]
async fn wal_stays_under_64mb_during_batch_writes() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("code_graph.db");
    let db = Arc::new(CodeGraphDB::new(&db_path).unwrap());

    // Generate WAL pressure exceeding 64 MB (67108864 bytes) bound using large symbol payloads
    let large_signature = "a".repeat(100_000); // 100 KB payload per symbol
    let symbols_per_batch = 700; // 700 * 100 KB = ~70 MB raw write volume
    let num_batches = 3;

    for batch in 0..num_batches {
        let symbols: Vec<code_graph::types::Symbol> = (0..symbols_per_batch)
            .map(|i| code_graph::types::Symbol {
                id: None,
                stable_id: None,
                name: format!("batch_{}_sym_{}", batch, i),
                kind: SymbolKind::Function,
                lang: Language::Rust,
                file_path: format!("src/batch_{}/file_{}.rs", batch, i % 10),
                start_line: i as u32,
                end_line: (i + 1) as u32,
                start_col: 0,
                end_col: 0,
                signature: Some(large_signature.clone()),
                parent: None,
                complexity: Some(1.0),
            })
            .collect();

        db.insert_symbols(&symbols).unwrap();
        db.checkpoint_wal().unwrap();

        // Assert journal_size_limit bounds WAL file size under 64 MB (67108864 bytes / 64 * 1024 * 1024)
        let wal_path = db_path.with_extension("db-wal");
        if wal_path.exists() {
            let wal_size = std::fs::metadata(&wal_path).unwrap().len();
            assert!(
                wal_size < 64 * 1024 * 1024,
                "WAL size {} bytes exceeded 64 MB bound (journal_size_limit = 67108864) at batch {}",
                wal_size,
                batch
            );
        }
    }
}
