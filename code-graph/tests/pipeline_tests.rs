use code_graph::db::CodeGraphDB;
use code_graph::indexer::Indexer;
use code_graph::plugin::{
    health::{CircuitState, PluginHealthMonitor},
    manager::PluginManager,
    types::PluginDescriptor,
};
use code_graph::types::Language;
use code_graph::parser::parse_source;
use std::sync::Arc;

#[tokio::test]
async fn test_codegraph_pipeline() {
    // 1. Check if cc is available and compile a mock C plugin parser
    let temp_dir = tempfile::tempdir().unwrap();
    let c_src_path = temp_dir.path().join("mock_parser.c");
    let bin_path = temp_dir.path().join("mock_parser");

    let c_code = r#"
#include <stdio.h>
int main() {
    printf("{\n"
           "  \"symbols\": [\n"
           "    {\n"
           "      \"id\": null,\n"
           "      \"stable_id\": \"mock_c_func_id\",\n"
           "      \"name\": \"mock_c_func\",\n"
           "      \"kind\": \"Function\",\n"
           "      \"lang\": \"C\",\n"
           "      \"file_path\": \"test_c_file.c\",\n"
           "      \"start_line\": 1,\n"
           "      \"end_line\": 5,\n"
           "      \"start_col\": 0,\n"
           "      \"end_col\": 0,\n"
           "      \"signature\": \"void mock_c_func()\",\n"
           "      \"parent\": null,\n"
           "      \"complexity\": 1.0\n"
           "    }\n"
           "  ],\n"
           "  \"error\": null\n"
           "}\n");
    return 0;
}
"#;

    std::fs::write(&c_src_path, c_code).unwrap();

    let cc_available = std::process::Command::new("cc")
        .arg("-o")
        .arg(&bin_path)
        .arg(&c_src_path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !cc_available {
        println!("C compiler not available or failed to compile mock parser, skipping test_codegraph_pipeline.");
        return;
    }

    // 2. Setup the codebase and DB
    let db = Arc::new(CodeGraphDB::in_memory().unwrap());
    let manager = PluginManager::new();

    // 3. Register the C binary plugin for Language::C
    manager.register(PluginDescriptor {
        name: "mock-parser-c".to_string(),
        version: "1.0.0".to_string(),
        command: bin_path.to_string_lossy().to_string(),
        languages: vec![Language::C],
        extensions: vec!["c".to_string()],
        capabilities: vec!["parse".to_string()],
    });

    // 4. File parse
    let source_content = "void mock_c_func() {}";
    let file_path = "test_c_file.c";
    let lang = Language::C;

    let symbols = parse_source(source_content, &lang, file_path, Some(&manager))
        .await
        .unwrap();

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "mock_c_func");
    assert_eq!(symbols[0].stable_id.as_deref(), Some("mock_c_func_id"));

    // 5. Index / Insert into DB
    db.insert_symbols(&symbols).unwrap();

    // 6. Query symbols
    let results = db.find_symbols("mock_c_func", 10).unwrap();
    assert_eq!(results.symbols.len(), 1);
    assert_eq!(results.symbols[0].name, "mock_c_func");
    assert_eq!(results.symbols[0].lang, Language::C);
}

#[tokio::test]
async fn test_fallback_pipeline() {
    // 1. Setup DB and native file
    let db = Arc::new(CodeGraphDB::in_memory().unwrap());

    // We use standard Rust/Python parsing (native only)
    let source_content = "fn test_native_func() {\n    println!(\"hello\");\n}";
    let file_path = "src/main.rs";
    let lang = Language::Rust;

    // 2. Parse (using no plugin manager, so it goes to native parser)
    let symbols = parse_source(source_content, &lang, file_path, None)
        .await
        .unwrap();

    assert!(!symbols.is_empty(), "Native parser should find symbols");
    let found_func = symbols.iter().any(|s| s.name == "test_native_func");
    assert!(found_func, "Should find test_native_func symbol");

    // 3. Index / Insert into DB
    db.insert_symbols(&symbols).unwrap();

    // 4. Query symbols
    let results = db.find_symbols("test_native_func", 10).unwrap();
    assert_eq!(results.symbols.len(), 1);
    assert_eq!(results.symbols[0].name, "test_native_func");
}

#[tokio::test]
async fn test_circuit_breaker() {
    // 1. Setup a crashing C binary plugin
    let temp_dir = tempfile::tempdir().unwrap();
    let c_src_path = temp_dir.path().join("crash_parser.c");
    let bin_path = temp_dir.path().join("crash_parser");

    let c_code = r#"
int main() {
    return 1;
}
"#;

    std::fs::write(&c_src_path, c_code).unwrap();

    let cc_available = std::process::Command::new("cc")
        .arg("-o")
        .arg(&bin_path)
        .arg(&c_src_path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !cc_available {
        println!("C compiler not available or failed to compile crash parser, skipping test_circuit_breaker crashing plugin part.");
        return;
    }

    let manager = PluginManager::new();
    let plugin_name = "crashy-plugin";
    manager.register(PluginDescriptor {
        name: plugin_name.to_string(),
        version: "1.0.0".to_string(),
        command: bin_path.to_string_lossy().to_string(),
        languages: vec![Language::Python],
        extensions: vec!["py".to_string()],
        capabilities: vec!["parse".to_string()],
    });

    let source_content = "def test_native_python():\n    pass";
    let file_path = "test.py";
    let lang = Language::Python;

    // Call 1: fails, falls back to native parser which successfully returns the native symbol!
    let symbols1 = parse_source(source_content, &lang, file_path, Some(&manager)).await.unwrap();
    assert!(!symbols1.is_empty(), "Should fallback to native python parser and return symbols");
    assert!(symbols1.iter().any(|s| s.name == "test_native_python"));

    // Call 2: fails
    let _ = parse_source(source_content, &lang, file_path, Some(&manager)).await;
    // Call 3: fails (circuit should be open after this)
    let _ = parse_source(source_content, &lang, file_path, Some(&manager)).await;

    // Check health monitor state
    let health = manager.health().expect("should have health monitor");
    assert_eq!(health.circuit_state(plugin_name), CircuitState::Open);
    assert!(health.is_open(plugin_name));

    let metrics = health.metrics(plugin_name).expect("should have metrics");
    assert_eq!(metrics.failure_count, 3);

    // Call 4: should skip plugin because circuit is Open, falling back directly to native parser.
    // Since it's skipped, failure count should NOT increase.
    let symbols4 = parse_source(source_content, &lang, file_path, Some(&manager)).await.unwrap();
    assert!(!symbols4.is_empty(), "Should fallback directly to native python parser and return symbols");
    assert!(symbols4.iter().any(|s| s.name == "test_native_python"));

    let metrics2 = health.metrics(plugin_name).expect("should have metrics");
    assert_eq!(metrics2.failure_count, 3, "Failure count should stay 3 because plugin was skipped by circuit breaker!");

    // 2. Test recovery using a custom health monitor with 50ms check interval
    let custom_health = Arc::new(PluginHealthMonitor::new(std::time::Duration::from_millis(50)));
    let custom_plugin = "custom-plugin";

    // Trigger 3 failures
    for _ in 0..3 {
        custom_health.record(custom_plugin, 0, false, Some("crash".to_string()));
    }
    assert_eq!(custom_health.circuit_state(custom_plugin), CircuitState::Open);

    // Sleep to exceed the 50ms interval and transition to HalfOpen
    tokio::time::sleep(std::time::Duration::from_millis(70)).await;
    assert_eq!(custom_health.circuit_state(custom_plugin), CircuitState::HalfOpen);

    // Record a success -> transitions to Closed (recovery!)
    custom_health.record(custom_plugin, 0, true, None);
    assert_eq!(custom_health.circuit_state(custom_plugin), CircuitState::Closed);
    assert!(!custom_health.is_open(custom_plugin));
}

#[tokio::test]
async fn test_empty_codebase() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db = Arc::new(CodeGraphDB::in_memory().unwrap());
    let indexer = Indexer::new(db.clone());

    // Index the empty directory
    let stats = indexer.index(temp_dir.path(), false).await.unwrap();

    assert_eq!(stats.total_files, 0);
    assert_eq!(stats.total_symbols, 0);
    assert_eq!(stats.total_imports, 0);
    assert!(stats.languages.is_empty());
}
