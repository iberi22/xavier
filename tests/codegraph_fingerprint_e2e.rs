//! e2e test for code-graph fingerprint contract.

use code_graph::db::CodeGraphDB;
use code_graph::indexer::Indexer;
use code_graph::query::QueryEngine;
use std::fs;
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
    let code_content = fs::read_to_string(&path_str).unwrap();
    let fp = code_graph::fingerprint::fingerprint_source(
        &code_content,
        code_graph::types::Language::Rust,
        &path_str,
        &[],
    );

    // stable
    state.indexer.index(&root, true).await.unwrap();

    let fp2 = code_graph::fingerprint::fingerprint_source(
        &code_content,
        code_graph::types::Language::Rust,
        &path_str,
        &[],
    );

    assert_eq!(fp.functions.len(), 1);
    assert_eq!(
        code_graph::fingerprint::to_json_sorted(&fp),
        code_graph::fingerprint::to_json_sorted(&fp2)
    );
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

    let fp1 = {
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
        let code_content = fs::read_to_string(&path_str).unwrap();

        let fp_doc = code_graph::fingerprint::fingerprint_source(
            &code_content,
            code_graph::types::Language::Rust,
            &path_str,
            &[],
        );
        code_graph::fingerprint::to_json_sorted(&fp_doc)
    };

    // change the source
    fs::write(&lib_rs, code2).unwrap();

    let fp2 = {
        let db2 = Arc::new(CodeGraphDB::in_memory().unwrap());
        let indexer2 = Arc::new(Indexer::new(Arc::clone(&db2)));
        let query2 = Arc::new(QueryEngine::new(Arc::clone(&db2)));
        let state2 = CodeGraphState {
            db: Arc::clone(&db2),
            indexer: indexer2,
            query: query2,
        };

        state2.indexer.index(&root, true).await.unwrap();

        let path_str = lib_rs.to_str().unwrap().to_string();
        let code_content = fs::read_to_string(&path_str).unwrap();

        let fp_doc = code_graph::fingerprint::fingerprint_source(
            &code_content,
            code_graph::types::Language::Rust,
            &path_str,
            &[],
        );
        code_graph::fingerprint::to_json_sorted(&fp_doc)
    };

    assert_ne!(fp1, fp2);
}
