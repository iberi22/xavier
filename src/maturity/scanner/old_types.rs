// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! # Old Types — Backward-Compatible Type Definitions
//!
//! These types are used by the original scanner code and by consumers
//! that import from `crate::maturity::scanner`. They are re-exported
//! from `scanner/mod.rs`.

use std::collections::HashSet;
use std::process::Command;

/// Result of scanning the code-graph for a set of symbols.
#[derive(Debug, Clone)]
pub struct CodeGraphScan {
    /// Symbols that were found
    pub found: HashSet<String>,
    /// Symbols that were not found
    pub missing: HashSet<String>,
    /// Errors during scanning
    pub errors: Vec<String>,
}

/// Result of scanning tests via `cargo test --list`.
#[derive(Debug, Clone)]
pub struct TestListScan {
    /// All discovered test names (fully qualified)
    pub all_tests: Vec<String>,
    /// Tests that match a given pattern
    pub matching: HashSet<String>,
    /// Errors
    pub errors: Vec<String>,
}

/// Integrates with Xavier's `code-graph` crate to find symbols.
/// Falls back to filesystem grep if code-graph is unavailable.
pub fn scan_code_graph(root: &str, symbols: &[String]) -> CodeGraphScan {
    let mut found = HashSet::new();
    let mut missing = HashSet::new();
    let errors: Vec<String> = Vec::new();

    // Try code-graph db first
    let db_path = format!("{}/.xavier/codegraph.json", root);
    let _symbols_set: HashSet<&str> = symbols.iter().map(|s| s.as_str()).collect();

    if let Ok(content) = std::fs::read_to_string(&db_path) {
        for symbol in symbols {
            if content.contains(symbol) {
                found.insert(symbol.clone());
            } else {
                missing.insert(symbol.clone());
            }
        }
        return CodeGraphScan {
            found,
            missing,
            errors,
        };
    }

    // Fallback: grep .rs files
    let walker = walkdir::WalkDir::new(root)
        .max_depth(8)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "rs")
                && !e.path().to_string_lossy().contains("target")
        });

    for entry in walker {
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            for symbol in symbols {
                if !found.contains(symbol) && content.contains(symbol.as_str()) {
                    found.insert(symbol.clone());
                }
            }
        }
    }

    for symbol in symbols {
        if !found.contains(symbol.as_str()) {
            missing.insert(symbol.clone());
        }
    }

    CodeGraphScan {
        found,
        missing,
        errors,
    }
}

/// Run `cargo test --list` and return all discovered test names.
pub fn list_tests(root: &str, feature_gates: &[&str]) -> TestListScan {
    let mut all_tests = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    let mut cmd = Command::new("cargo");
    cmd.args(["test", "--list", "--workspace", "--message-format", "json"]);

    for gate in feature_gates {
        if !gate.is_empty() {
            cmd.args(["--features", gate]);
        }
    }

    let output = cmd.current_dir(root).output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);

            for line in stdout.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(test_name) = msg.get("name").and_then(|v| v.as_str()) {
                        if let Some(event) = msg.get("event").and_then(|v| v.as_str()) {
                            if event == "discovered" {
                                all_tests.push(test_name.to_string());
                            }
                        }
                    }
                }
            }

            if !stderr.is_empty() && all_tests.is_empty() {
                errors.push(format!("cargo test stderr: {}", stderr.trim()));
            }
        }
        Err(e) => {
            errors.push(format!("Failed to run cargo test: {}", e));
        }
    }

    let matching: HashSet<String> = all_tests.iter().cloned().collect();

    TestListScan {
        all_tests,
        matching,
        errors,
    }
}

/// Check if a specific test exists and passes by running it.
/// Returns (exists, passes).
pub fn check_test(root: &str, test_name: &str, features: &[&str]) -> (bool, bool) {
    let mut cmd = Command::new("cargo");
    cmd.args(["test", test_name, "--"]).current_dir(root);

    for feat in features {
        if !feat.is_empty() {
            cmd.args(["--features", feat]);
        }
    }

    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let exists = stdout.contains("test result");
            let passed = stdout.contains("test result: ok")
                || stdout.contains(&format!("test {} ... ok", test_name));
            (exists, passed)
        }
        Err(_) => (false, false),
    }
}

/// Scan Cargo.toml for a feature definition.
pub fn check_feature_in_cargo(root: &str, feature_name: &str) -> bool {
    let cargo_path = format!("{}/Cargo.toml", root);
    if let Ok(content) = std::fs::read_to_string(&cargo_path) {
        let in_features = content.contains(&format!("{} = [", feature_name));
        let in_deps = content.contains(&format!("\"{}\"", feature_name));
        return in_features || in_deps;
    }
    false
}

/// Count lines of code for files matching a pattern in the codebase.
pub fn count_loc_for_symbol(root: &str, pattern: &str) -> usize {
    let mut total = 0usize;
    let walker = walkdir::WalkDir::new(root)
        .max_depth(8)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "rs")
                && !e.path().to_string_lossy().contains("target")
        });

    for entry in walker {
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            if content.contains(pattern) {
                total += content.lines().count();
            }
        }
    }

    total
}
