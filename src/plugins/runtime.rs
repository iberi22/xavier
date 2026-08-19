//! Plugin runtime environment for Xavier.
//!
//! Provides APIs for registering, loading, and executing plugins that are compatible with `code-graph`.

use anyhow::{Context, Result};
use code_graph::plugin::types::{FileToParse, PluginDescriptor};
use code_graph::plugin::PluginManager;
use code_graph::types::{Language, Symbol};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The runtime environment for managing Xavier plugins.
pub struct XavierPluginRuntime {
    /// Reference to the underlying PluginManager from code-graph
    pub manager: Arc<PluginManager>,
}

impl XavierPluginRuntime {
    /// Create a new plugin runtime wrapping an existing `PluginManager`.
    pub fn new(manager: Arc<PluginManager>) -> Self {
        Self { manager }
    }

    /// Explicitly register a pre-constructed `PluginDescriptor` into the manager.
    pub fn register_plugin(&self, descriptor: PluginDescriptor) {
        self.manager.register(descriptor);
    }

    /// Load and register a plugin from a specific file path.
    ///
    /// This sets appropriate executable permissions on Unix platforms before registration.
    pub fn load_plugin(
        &self,
        name: &str,
        path: &Path,
        languages: Vec<Language>,
        extensions: Vec<String>,
    ) -> Result<PluginDescriptor> {
        let abs_path = fs::canonicalize(path)
            .with_context(|| format!("Failed to canonicalize plugin path: {}", path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = fs::metadata(&abs_path) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&abs_path, perms).with_context(|| {
                    format!(
                        "Failed to set executable permissions on {}",
                        abs_path.display()
                    )
                })?;
            }
        }

        let descriptor = PluginDescriptor {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            command: abs_path.to_string_lossy().to_string(),
            languages,
            extensions,
            capabilities: vec!["parse".to_string()],
        };

        self.register_plugin(descriptor.clone());
        Ok(descriptor)
    }

    /// Execute a registered plugin for a set of source files.
    pub async fn execute_plugin(
        &self,
        name: &str,
        lang: Language,
        files: Vec<FileToParse>,
    ) -> Result<Vec<Symbol>> {
        let symbols = self
            .manager
            .parse_with_plugin(name, lang, files)
            .await
            .map_err(|e| anyhow::anyhow!("Plugin execution failed: {}", e))?;
        Ok(symbols)
    }

    /// Load and register a python plugin from string content.
    ///
    /// Writes the script to the target temp directory, ensures it is executable,
    /// and registers it with the plugin manager for the Python language.
    pub async fn load_and_register_python_plugin(
        &self,
        name: &str,
        script_content: &str,
        temp_dir: &Path,
    ) -> Result<PluginDescriptor> {
        let script_path = temp_dir.join(format!("{}.py", name));
        fs::write(&script_path, script_content).with_context(|| {
            format!("Failed to write python script to {}", script_path.display())
        })?;

        self.load_plugin(
            name,
            &script_path,
            vec![Language::Python],
            vec!["py".to_string()],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_graph::plugin::types::FallbackStep;
    use code_graph::types::SymbolKind;
    use std::io::Write;

    // 1. Test registration
    #[test]
    fn test_registration() {
        let manager = Arc::new(PluginManager::new());
        let runtime = XavierPluginRuntime::new(manager.clone());

        let descriptor = PluginDescriptor {
            name: "test-plugin".to_string(),
            version: "0.1.0".to_string(),
            command: "test-cmd".to_string(),
            languages: vec![Language::Python],
            extensions: vec!["py".to_string()],
            capabilities: vec!["parse".to_string()],
        };

        runtime.register_plugin(descriptor.clone());

        let registered = manager.descriptor_for(&Language::Python);
        assert!(registered.is_some());
        assert_eq!(registered.unwrap().name, "test-plugin");
    }

    // 2. Test loading from disk
    #[test]
    fn test_loading() {
        let manager = Arc::new(PluginManager::new());
        let runtime = XavierPluginRuntime::new(manager);

        let temp_dir = tempfile::tempdir().unwrap();
        let script_path = temp_dir.path().join("mock-plugin.sh");
        fs::write(&script_path, "#!/bin/sh\necho '{}'\n").unwrap();

        let desc = runtime
            .load_plugin(
                "loaded-plugin",
                &script_path,
                vec![Language::TypeScript],
                vec!["ts".to_string()],
            )
            .expect("Failed to load plugin");

        assert_eq!(desc.name, "loaded-plugin");
        assert!(desc.command.contains("mock-plugin.sh"));
        assert_eq!(desc.languages, vec![Language::TypeScript]);
        assert_eq!(desc.extensions, vec!["ts".to_string()]);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&script_path).unwrap();
            let mode = metadata.permissions().mode();
            assert_ne!(mode & 0o111, 0, "File should be executable");
        }
    }

    // 3. Test execution of a mock plugin process
    #[tokio::test]
    async fn test_execution() {
        let manager = Arc::new(PluginManager::new());
        let runtime = XavierPluginRuntime::new(manager);

        let temp_dir = tempfile::tempdir().unwrap();
        let script_path = temp_dir.path().join("mock-executor");

        // Write a script that parses inputs and produces JSON PluginResponse
        let script_content = r#"#!/bin/sh
cat > /dev/null
echo '{"symbols": [{"name": "execute_test", "kind": "Function", "lang": "Python", "file_path": "dummy.py", "start_line": 10, "end_line": 12, "start_col": 4, "end_col": 8, "signature": "def execute_test()", "parent": null, "complexity": 1.0}], "error": null}'
"#;
        fs::write(&script_path, script_content).unwrap();

        let _desc = runtime
            .load_plugin(
                "executor-plugin",
                &script_path,
                vec![Language::Python],
                vec!["py".to_string()],
            )
            .expect("Failed to load plugin");

        let files = vec![FileToParse {
            path: "dummy.py".to_string(),
            source: "def execute_test(): pass".to_string(),
        }];

        let symbols = runtime
            .execute_plugin("executor-plugin", Language::Python, files)
            .await
            .expect("Failed to execute plugin");

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "execute_test");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
        assert_eq!(symbols[0].lang, Language::Python);
    }

    // 4. Test fallback chain resolver interaction
    #[test]
    fn test_fallback() {
        let manager = Arc::new(PluginManager::new());
        let runtime = XavierPluginRuntime::new(manager.clone());

        // Initially Python has default fallback: Native, NoOp
        let initial_chain = manager.chain_for(&Language::Python);
        assert_eq!(
            initial_chain,
            vec![FallbackStep::Native, FallbackStep::NoOp]
        );

        // Register custom plugin
        let descriptor = PluginDescriptor {
            name: "python-super-parser".to_string(),
            version: "0.1.0".to_string(),
            command: "super-py".to_string(),
            languages: vec![Language::Python],
            extensions: vec!["py".to_string()],
            capabilities: vec!["parse".to_string()],
        };
        runtime.register_plugin(descriptor);

        // Python now prefers python-super-parser first, then Native, then NoOp
        let updated_chain = manager.chain_for(&Language::Python);
        assert_eq!(
            updated_chain,
            vec![
                FallbackStep::Plugin("python-super-parser".to_string()),
                FallbackStep::Native,
                FallbackStep::NoOp
            ]
        );
    }

    // 5. Test error / negative execution scenarios
    #[tokio::test]
    async fn test_errors() {
        let manager = Arc::new(PluginManager::new());
        let runtime = XavierPluginRuntime::new(manager);

        let temp_dir = tempfile::tempdir().unwrap();
        let script_path = temp_dir.path().join("failing-executor");

        // Write a script that exits with non-zero status
        let script_content = r#"#!/bin/sh
echo "Some stderr error msg" >&2
exit 1
"#;
        fs::write(&script_path, script_content).unwrap();

        let _desc = runtime
            .load_plugin(
                "failing-plugin",
                &script_path,
                vec![Language::Python],
                vec!["py".to_string()],
            )
            .expect("Failed to load plugin");

        let files = vec![FileToParse {
            path: "dummy.py".to_string(),
            source: "def execute_test(): pass".to_string(),
        }];

        let result = runtime
            .execute_plugin("failing-plugin", Language::Python, files)
            .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Plugin execution failed") || err_msg.contains("exited with status")
        );
    }

    // 6. Test Live E2E Python parser plugin using a real Python script loaded via `include_str!`
    #[tokio::test]
    async fn test_live_e2e_python_plugin() {
        let manager = Arc::new(PluginManager::new());
        let runtime = XavierPluginRuntime::new(manager);

        // Load the REAL python parser plugin content from plugins/parser-python/plugin.py
        let real_python_plugin_src = include_str!("../../plugins/parser-python/plugin.py");

        let temp_dir = tempfile::tempdir().unwrap();
        let desc = runtime
            .load_and_register_python_plugin(
                "parser-python",
                real_python_plugin_src,
                temp_dir.path(),
            )
            .await
            .expect("Failed to load and register real Python parser plugin");

        assert_eq!(desc.name, "parser-python");
        assert_eq!(desc.languages, vec![Language::Python]);

        // Real python code to parse
        let py_code = r#"
import os
import sys

class Calculator:
    """A simple calculator class"""
    def add(self, a, b):
        if a > 0:
            return a + b
        return b

def compute_everything():
    c = Calculator()
    return c.add(1, 2)
"#;

        let files = vec![FileToParse {
            path: "calc.py".to_string(),
            source: py_code.to_string(),
        }];

        // Execute the real python plugin via the runtime/manager
        let symbols = runtime
            .execute_plugin("parser-python", Language::Python, files)
            .await
            .expect("Failed to run E2E Python parser plugin");

        assert!(!symbols.is_empty(), "Parsed symbols should not be empty");

        // Find the imported modules
        let has_os_import = symbols
            .iter()
            .any(|s| s.name == "os" && s.kind == SymbolKind::Import);
        let has_sys_import = symbols
            .iter()
            .any(|s| s.name == "sys" && s.kind == SymbolKind::Import);
        assert!(has_os_import, "Should extract 'os' import");
        assert!(has_sys_import, "Should extract 'sys' import");

        // Find Calculator class
        let calc_class = symbols
            .iter()
            .find(|s| s.name == "Calculator" && s.kind == SymbolKind::Class)
            .expect("Should find 'Calculator' class symbol");
        assert_eq!(calc_class.lang, Language::Python);
        assert_eq!(calc_class.file_path, "calc.py");

        // Find add method
        let add_method = symbols
            .iter()
            .find(|s| s.name == "add" && s.kind == SymbolKind::Method)
            .expect("Should find 'add' method symbol");
        assert_eq!(add_method.parent.as_deref(), Some("Calculator"));
        // Cyclomatic complexity check:
        // base complexity (1) + "if a > 0" (1) = 2.0
        assert_eq!(add_method.complexity, Some(2.0));

        // Find compute_everything function
        let compute_fn = symbols
            .iter()
            .find(|s| s.name == "compute_everything" && s.kind == SymbolKind::Function)
            .expect("Should find 'compute_everything' function symbol");
        assert_eq!(compute_fn.parent, None);
        assert_eq!(compute_fn.complexity, Some(1.0));
    }
}
