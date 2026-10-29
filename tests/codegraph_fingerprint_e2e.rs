//! e2e test for code-graph fingerprint contract.

#![allow(dead_code)]
use std::fs;
use tempfile::tempdir;
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

    // Environment overrides as per rules
    let mut _temp_env = EnvRestorer::set("HOME", tmp.path());
    _temp_env.append("XAVIER_CONFIG_PATH", &tmp.path().join(".config"));
    _temp_env.append("XAVIER_RUNTIME_STATE_PATH", &tmp.path().join(".run"));
    _temp_env.append("XAVIER_DATA_DIR", &tmp.path().join(".data"));

    let root = tmp.path().join("repo");
    fs::create_dir_all(&root).unwrap();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let code = "pub fn add(a: i32, b: i32) -> i32 { a + b }";
    let lib_rs = src_dir.join("lib.rs");
    fs::write(&lib_rs, code).unwrap();

    let file_path = lib_rs.to_str().unwrap();

    let fp1 = code_graph::fingerprint::fingerprint_source(
        code,
        code_graph::types::Language::Rust,
        file_path,
        &[],
    );
    let fp2 = code_graph::fingerprint::fingerprint_source(
        code,
        code_graph::types::Language::Rust,
        file_path,
        &[],
    );

    assert_eq!(fp1.functions.len(), 1);
    assert_eq!(
        code_graph::fingerprint::to_json_sorted(&fp1),
        code_graph::fingerprint::to_json_sorted(&fp2)
    );
}

#[tokio::test]
#[serial_test::serial]
async fn test_fingerprint_changes() {
    let tmp = tempdir().unwrap();

    // Environment overrides as per rules
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
    let file_path = lib_rs.to_str().unwrap();

    let fp1 = code_graph::fingerprint::fingerprint_source(
        code1,
        code_graph::types::Language::Rust,
        file_path,
        &[],
    );

    fs::write(&lib_rs, code2).unwrap();
    let fp2 = code_graph::fingerprint::fingerprint_source(
        code2,
        code_graph::types::Language::Rust,
        file_path,
        &[],
    );

    assert_ne!(
        code_graph::fingerprint::to_json_sorted(&fp1),
        code_graph::fingerprint::to_json_sorted(&fp2)
    );
}
