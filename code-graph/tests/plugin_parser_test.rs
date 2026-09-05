use code_graph::plugin::types::{FileToParse, PluginConfig, PluginDescriptor, PluginEngine};
use code_graph::plugin::ProcessEngine;
use code_graph::plugin::PluginManager;
use code_graph::types::{Language, SymbolKind};
use std::fs::{self, File};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn test_plugin_parser_mock_process_protocol() {
    // 1. Create a mock executable that accepts PluginRequest on stdin and outputs PluginResponse on stdout
    let temp_dir = tempfile::tempdir().unwrap();
    let mock_bin = temp_dir.path().join("mock-parser");

    let script = r#"#!/bin/sh
# Read stdin JSON
cat > /dev/null

# Respond with valid PluginResponse JSON
cat << 'ENDJSON'
{
  "symbols": [
    {
      "name": "mock_process_symbol",
      "kind": "Function",
      "lang": "Rust",
      "file_path": "mock.rs",
      "start_line": 10,
      "end_line": 20,
      "start_col": 0,
      "end_col": 0,
      "signature": "fn mock_process_symbol()",
      "parent": null,
      "complexity": 2.5
    },
    {
      "name": "MockStruct",
      "kind": "Struct",
      "lang": "Rust",
      "file_path": "mock.rs",
      "start_line": 25,
      "end_line": 35,
      "start_col": 0,
      "end_col": 0,
      "signature": "struct MockStruct",
      "parent": null,
      "complexity": 1.0
    }
  ],
  "error": null
}
ENDJSON
"#;

    {
        let mut f = File::create(&mock_bin).unwrap();
        f.write_all(script.as_bytes()).unwrap();
    }

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&mock_bin).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&mock_bin, perms).unwrap();
    }

    // 2. Direct ProcessEngine test
    let engine = ProcessEngine::new();
    let config = PluginConfig {
        name: "mock-parser".to_string(),
        command: mock_bin.to_string_lossy().to_string(),
        version: "1.0.0".to_string(),
        languages: vec![Language::Rust],
        extensions: Some(vec!["rs".to_string()]),
        capabilities: vec!["parse".to_string()],
    };

    let files = vec![FileToParse {
        path: "mock.rs".to_string(),
        source: "fn mock_process_symbol() {}\nstruct MockStruct {}".to_string(),
    }];

    let symbols = engine
        .parse(&config, Language::Rust, files)
        .await
        .expect("ProcessEngine should parse successfully with mock-parser");

    assert_eq!(symbols.len(), 2);
    assert_eq!(symbols[0].name, "mock_process_symbol");
    assert_eq!(symbols[0].kind, SymbolKind::Function);
    assert_eq!(symbols[0].complexity, Some(2.5));
    assert_eq!(symbols[1].name, "MockStruct");
    assert_eq!(symbols[1].kind, SymbolKind::Struct);

    // 3. PluginManager registration and execution test
    let manager = PluginManager::new();
    let descriptor = PluginDescriptor {
        name: "mock-parser".to_string(),
        version: "1.0.0".to_string(),
        command: mock_bin.to_string_lossy().to_string(),
        languages: vec![Language::Rust],
        extensions: vec!["rs".to_string()],
        capabilities: vec!["parse".to_string()],
    };
    manager.register(descriptor);

    let parse_files = vec![FileToParse {
        path: "mock.rs".to_string(),
        source: "// some code".to_string(),
    }];

    let mgr_symbols = manager
        .parse_with_plugin("mock-parser", Language::Rust, parse_files)
        .await
        .expect("PluginManager should execute registered mock-parser successfully");

    assert_eq!(mgr_symbols.len(), 2);
    assert_eq!(mgr_symbols[0].name, "mock_process_symbol");
    assert_eq!(mgr_symbols[1].name, "MockStruct");
}

#[tokio::test]
async fn test_plugin_parser_error_handling() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mock_failing_bin = temp_dir.path().join("failing-parser");

    let script = r#"#!/bin/sh
cat > /dev/null
echo "fatal: syntax error in source" >&2
exit 1
"#;

    {
        let mut f = File::create(&mock_failing_bin).unwrap();
        f.write_all(script.as_bytes()).unwrap();
    }

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&mock_failing_bin).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&mock_failing_bin, perms).unwrap();
    }

    let engine = ProcessEngine::new();
    let config = PluginConfig {
        name: "failing-parser".to_string(),
        command: mock_failing_bin.to_string_lossy().to_string(),
        version: "1.0.0".to_string(),
        languages: vec![Language::Rust],
        extensions: Some(vec!["rs".to_string()]),
        capabilities: vec!["parse".to_string()],
    };

    let files = vec![FileToParse {
        path: "fail.rs".to_string(),
        source: "bad syntax".to_string(),
    }];

    let result = engine.parse(&config, Language::Rust, files).await;
    assert!(result.is_err(), "engine.parse should return Err when process exits non-zero");
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("fatal: syntax error in source") || err_msg.contains("exited with status"));
}
