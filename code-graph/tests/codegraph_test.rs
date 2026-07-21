use code_graph::plugin::types::FileToParse;
use code_graph::plugin::PluginManager;
use code_graph::types::{Language, SymbolKind};
use std::env;
use std::fs::{self, File};
use std::io::Write;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn test_codegraph_plugin_auto_detection_and_execution() {
    // 1. Create a temp directory for the mock codegraph executable
    let temp_dir = tempfile::tempdir().unwrap();
    let bin_path = temp_dir.path().join("codegraph");

    // 2. Write the mock codegraph script that reads stdin and outputs a valid JSON PluginResponse
    let script_content = r#"#!/bin/sh
cat > /dev/null
echo '{"symbols": [{"name": "test_func", "kind": "Function", "lang": "Rust", "file_path": "test.rs", "start_line": 1, "end_line": 2, "start_col": 0, "end_col": 0, "signature": "fn test_func()", "parent": null, "complexity": 1.0}], "error": null}'
"#;

    {
        let mut file = File::create(&bin_path).unwrap();
        file.write_all(script_content.as_bytes()).unwrap();
    }

    // 3. Make the script executable on Unix
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&bin_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&bin_path, perms).unwrap();
    }

    // 4. Save the original PATH and prepend our temp directory to the PATH
    let original_path = env::var_os("PATH").unwrap_or_default();
    let mut new_paths = vec![temp_dir.path().to_path_buf()];
    new_paths.extend(env::split_paths(&original_path));
    let new_path_os = env::join_paths(new_paths).unwrap();
    env::set_var("PATH", &new_path_os);

    // 5. Initialize the PluginManager (it should auto-detect and register our mock codegraph)
    let manager = PluginManager::new();

    // 6. Verify that "codegraph" is registered as a plugin for the 7 languages
    let test_langs = vec![
        Language::Rust,
        Language::TypeScript,
        Language::Python,
        Language::Go,
        Language::Java,
        Language::C,
        Language::Cpp,
    ];

    for lang in &test_langs {
        let desc = manager.descriptor_for(lang);
        assert!(
            desc.is_some(),
            "codegraph plugin should be registered for {:?}",
            lang
        );
        let desc = desc.unwrap();
        assert_eq!(desc.name, "codegraph");
        assert_eq!(desc.command, "codegraph");
        assert_eq!(desc.version, "1.4.1");
        assert!(desc.capabilities.contains(&"parse".to_string()));
        assert!(desc.capabilities.contains(&"index".to_string()));
        assert!(desc.capabilities.contains(&"query".to_string()));
    }

    // 7. Test the actual parse execution via ProcessEngine to verify stdin/stdout protocol integration
    let files = vec![FileToParse {
        path: "test.rs".to_string(),
        source: "fn test_func() {}".to_string(),
    }];

    let symbols = manager
        .parse_with_plugin("codegraph", Language::Rust, files)
        .await
        .expect("should execute codegraph parse successfully");

    // Restore the original PATH to prevent contaminating subsequent tests
    env::set_var("PATH", original_path);

    assert_eq!(symbols.len(), 1);
    let sym = &symbols[0];
    assert_eq!(sym.name, "test_func");
    assert_eq!(sym.kind, SymbolKind::Function);
    assert_eq!(sym.lang, Language::Rust);
    assert_eq!(sym.file_path, "test.rs");
    assert_eq!(sym.start_line, 1);
    assert_eq!(sym.end_line, 2);
    assert_eq!(sym.complexity, Some(1.0));
}
