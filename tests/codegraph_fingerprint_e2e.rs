//! e2e test for code-graph fingerprint contract.

use code_graph::db::CodeGraphDB;
use code_graph::indexer::Indexer;
use code_graph::query::QueryEngine;
use std::fs;
use std::process::Command;
use std::sync::Arc;
use tempfile::tempdir;
use xavier::cli::state::CodeGraphState;
use xavier::isolate_test_process;

isolate_test_process!();

struct EnvRestorer {
    saved: Vec<(String, Option<std::ffi::OsString>)>,
}

impl EnvRestorer {
    fn set(key: &str, val: &std::path::Path) -> Self {
        let prev = std::env::var_os(key);
        std::env::set_var(key, val);
        Self {
            saved: vec![(key.to_string(), prev)],
        }
    }

    fn append(&mut self, key: &str, val: &std::path::Path) {
        let prev = std::env::var_os(key);
        std::env::set_var(key, val);
        self.saved.push((key.to_string(), prev));
    }
}

impl Drop for EnvRestorer {
    fn drop(&mut self) {
        for (k, v) in self.saved.drain(..) {
            if let Some(val) = v {
                std::env::set_var(&k, val);
            } else {
                std::env::remove_var(&k);
            }
        }
    }
}

fn get_cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

#[tokio::test]
#[serial_test::serial]
async fn test_fingerprint_stable_unchanged() {
    let tmp = tempdir().unwrap();

    let mut _temp_env = EnvRestorer::set("HOME", tmp.path());
    _temp_env.append("XAVIER_CONFIG_PATH", &tmp.path().join(".config"));
    _temp_env.append("XAVIER_RUNTIME_STATE_PATH", &tmp.path().join(".run"));
    _temp_env.append("XAVIER_DATA_DIR", &tmp.path().join(".data"));

    let root = tmp.path().join("repo");
    fs::create_dir_all(&root).unwrap();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let code = "pub fn add(a: i32, b: i32) -> i32 { a + b }";
    fs::write(src_dir.join("lib.rs"), code).unwrap();

    let db = Arc::new(CodeGraphDB::in_memory().unwrap());
    let indexer = Arc::new(Indexer::new(Arc::clone(&db)));
    let query = Arc::new(QueryEngine::new(Arc::clone(&db)));
    let state = CodeGraphState {
        db: Arc::clone(&db),
        indexer,
        query,
    };

    state.indexer.index(&root, true).await.unwrap();

    let path_str = src_dir.join("lib.rs").to_str().unwrap().to_string();

    let output1 = Command::new(get_cargo())
        .args(&[
            "run",
            "-p",
            "code-graph",
            "--",
            "fingerprint",
            "--lang",
            "rust",
            &path_str,
        ])
        .output()
        .expect("failed to execute cargo run");
    assert!(output1.status.success());
    let fp1_json = String::from_utf8_lossy(&output1.stdout);
    let mut fp1: serde_json::Value = serde_json::from_str(&fp1_json).unwrap();

    // stable
    state.indexer.index(&root, true).await.unwrap();

    let output2 = Command::new(get_cargo())
        .args(&[
            "run",
            "-p",
            "code-graph",
            "--",
            "fingerprint",
            "--lang",
            "rust",
            &path_str,
        ])
        .output()
        .expect("failed to execute cargo run");
    assert!(output2.status.success());
    let fp2_json = String::from_utf8_lossy(&output2.stdout);
    let mut fp2: serde_json::Value = serde_json::from_str(&fp2_json).unwrap();

    let functions = fp1.get("functions").unwrap().as_array().unwrap();
    assert_eq!(functions.len(), 1);

    // Sort functions arrays by name if they exist so we can compare
    if let Some(f1) = fp1.get_mut("functions").and_then(|v| v.as_array_mut()) {
        f1.sort_by_key(|f| f.get("name").unwrap().as_str().unwrap().to_string());
    }
    if let Some(f2) = fp2.get_mut("functions").and_then(|v| v.as_array_mut()) {
        f2.sort_by_key(|f| f.get("name").unwrap().as_str().unwrap().to_string());
    }
    assert_eq!(fp1, fp2);
}

#[tokio::test]
#[serial_test::serial]
async fn test_fingerprint_changes() {
    let tmp = tempdir().unwrap();

    let mut _temp_env = EnvRestorer::set("HOME", tmp.path());
    _temp_env.append("XAVIER_CONFIG_PATH", &tmp.path().join(".config"));
    _temp_env.append("XAVIER_RUNTIME_STATE_PATH", &tmp.path().join(".run"));
    _temp_env.append("XAVIER_DATA_DIR", &tmp.path().join(".data"));

    let root = tmp.path().join("repo");
    fs::create_dir_all(&root).unwrap();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let code1 = "pub fn add(a: i32, b: i32) -> i32 { a + b }";
    let code2 = "pub fn add(a: i32, b: i32) -> i32 { a - b }";

    let lib_rs = src_dir.join("lib.rs");

    fs::write(&lib_rs, code1).unwrap();

    let db = Arc::new(CodeGraphDB::in_memory().unwrap());
    let indexer = Arc::new(Indexer::new(Arc::clone(&db)));
    let query = Arc::new(QueryEngine::new(Arc::clone(&db)));
    let state = CodeGraphState {
        db: Arc::clone(&db),
        indexer,
        query,
    };

    state.indexer.index(&root, true).await.unwrap();

    let path_str = lib_rs.to_str().unwrap().to_string();

    let output1 = Command::new(get_cargo())
        .args(&[
            "run",
            "-p",
            "code-graph",
            "--",
            "fingerprint",
            "--lang",
            "rust",
            &path_str,
        ])
        .output()
        .expect("failed to execute cargo run");
    assert!(output1.status.success());
    let fp1_json = String::from_utf8_lossy(&output1.stdout);
    let mut fp1: serde_json::Value = serde_json::from_str(&fp1_json).unwrap();

    // change the source
    fs::write(&lib_rs, code2).unwrap();

    state.indexer.index(&root, true).await.unwrap();

    let output2 = Command::new(get_cargo())
        .args(&[
            "run",
            "-p",
            "code-graph",
            "--",
            "fingerprint",
            "--lang",
            "rust",
            &path_str,
        ])
        .output()
        .expect("failed to execute cargo run");
    assert!(output2.status.success());
    let fp2_json = String::from_utf8_lossy(&output2.stdout);
    let mut fp2: serde_json::Value = serde_json::from_str(&fp2_json).unwrap();

    if let Some(f1) = fp1.get_mut("functions").and_then(|v| v.as_array_mut()) {
        f1.sort_by_key(|f| f.get("name").unwrap().as_str().unwrap().to_string());
    }
    if let Some(f2) = fp2.get_mut("functions").and_then(|v| v.as_array_mut()) {
        f2.sort_by_key(|f| f.get("name").unwrap().as_str().unwrap().to_string());
    }

    assert_ne!(fp1, fp2);
}
