//! Integration tests for repo memory instance pack/unpack and cross-repo isolation.

use std::path::{Path, PathBuf};
use tempfile::TempDir;
use xavier::cli::commands::repo_pack::{pack_with_kek, unpack_with_kek};
use xavier::codebase::codegraph_paths::code_graph_db_path_for;
use xavier::codebase::repo_package_crypto::derive_package_kek_from_master;

xavier::isolate_test_process!();

const TEST_MASTER_KEY: [u8; 32] = [0x5A; 32];

fn setup_repo(base: &Path, name: &str, project_id: &str, symbol: &str) -> PathBuf {
    let root = base.join(name);
    std::fs::create_dir_all(root.join(".git")).expect("mkdir .git");
    std::fs::create_dir_all(root.join(".xavier")).expect("mkdir .xavier");

    let config = format!("project_id = \"{project_id}\"\n");
    std::fs::write(root.join(".xavier/config.toml"), config).expect("write config.toml");

    let db_path = code_graph_db_path_for(&root);
    let conn = rusqlite::Connection::open(&db_path).expect("open sqlite db");
    conn.execute_batch(&format!(
        "CREATE TABLE symbols (id INTEGER PRIMARY KEY, name TEXT);
         INSERT INTO symbols (name) VALUES ('{symbol}');"
    ))
    .expect("execute ddl");

    root.canonicalize().expect("canonicalize repo root")
}

fn query_symbol(root: &Path) -> Result<String, rusqlite::Error> {
    let db_path = code_graph_db_path_for(root);
    let conn = rusqlite::Connection::open(db_path)?;
    conn.query_row("SELECT name FROM symbols LIMIT 1", [], |r| r.get(0))
}

#[test]
fn test_pack_unpack_roundtrip() {
    let tmp = TempDir::new().expect("tempdir");
    let root = setup_repo(tmp.path(), "repo-alpha", "alpha-proj", "alpha_symbol_42");
    let kek = derive_package_kek_from_master(&TEST_MASTER_KEY, "alpha-proj").expect("derive kek");

    let packed = pack_with_kek(&root, "alpha-proj", &kek).expect("pack_with_kek");
    assert!(
        packed.enc_path.is_file(),
        "encrypted package file must exist"
    );

    let db_path = code_graph_db_path_for(&root);
    std::fs::remove_file(&db_path).expect("remove original db");
    assert!(!db_path.exists());

    let restored = unpack_with_kek(&root, &kek).expect("unpack_with_kek");
    assert_eq!(restored.project_id, "alpha-proj");
    assert!(db_path.is_file(), "code_graph.db must be restored");

    let sym = query_symbol(&root).expect("query restored symbol");
    assert_eq!(sym, "alpha_symbol_42");
}

#[test]
fn test_unpack_with_wrong_kek_fails() {
    let tmp = TempDir::new().expect("tempdir");
    let root = setup_repo(
        tmp.path(),
        "repo-wrong-key",
        "wrong-key-proj",
        "secret_symbol",
    );
    let valid_kek = derive_package_kek_from_master(&TEST_MASTER_KEY, "wrong-key-proj")
        .expect("derive valid kek");
    let wrong_kek = [0x99u8; 32];

    pack_with_kek(&root, "wrong-key-proj", &valid_kek).expect("pack_with_kek");

    let db_path = code_graph_db_path_for(&root);
    std::fs::remove_file(&db_path).expect("remove original db");

    let err = unpack_with_kek(&root, &wrong_kek).expect_err("wrong KEK must fail");
    let err_msg = format!("{err:#}");
    assert!(
        err_msg.contains("authentication failed")
            || err_msg.contains("unwrap failed")
            || err_msg.contains("no se pudo descifrar"),
        "expected decryption failure message, got: {err_msg}"
    );
    assert!(
        !db_path.exists(),
        "code_graph.db must not be created on failed unpack"
    );
}

#[test]
fn test_two_repos_stay_isolated() {
    let tmp = TempDir::new().expect("tempdir");
    let repo_a = setup_repo(tmp.path(), "repo-a", "proj-a", "symbol_a");
    let repo_b = setup_repo(tmp.path(), "repo-b", "proj-b", "symbol_b");

    let kek_a = derive_package_kek_from_master(&TEST_MASTER_KEY, "proj-a").expect("kek a");
    let kek_b = derive_package_kek_from_master(&TEST_MASTER_KEY, "proj-b").expect("kek b");
    assert_ne!(
        kek_a, kek_b,
        "different project IDs must produce different KEKs"
    );

    let packed_a = pack_with_kek(&repo_a, "proj-a", &kek_a).expect("pack a");
    let packed_b = pack_with_kek(&repo_b, "proj-b", &kek_b).expect("pack b");

    assert_ne!(
        std::fs::read(&packed_a.enc_path).expect("read a enc"),
        std::fs::read(&packed_b.enc_path).expect("read b enc"),
        "ciphertexts under different repo KEKs must differ"
    );

    let db_path_a = code_graph_db_path_for(&repo_a);
    std::fs::remove_file(&db_path_a).expect("remove a db");
    assert!(
        unpack_with_kek(&repo_a, &kek_b).is_err(),
        "cross-repo KEK unpack must fail"
    );
    assert!(
        !db_path_a.exists(),
        "repo A db must remain absent after failed unpack"
    );

    let sym_b = query_symbol(&repo_b).expect("query b symbol");
    assert_eq!(sym_b, "symbol_b", "repo B data must be untouched");

    let db_path_b = code_graph_db_path_for(&repo_b);
    std::fs::remove_file(&db_path_b).expect("remove b db");
    assert!(
        unpack_with_kek(&repo_b, &kek_a).is_err(),
        "unpacking repo B with repo A's KEK must fail"
    );
    assert!(!db_path_b.exists());

    unpack_with_kek(&repo_b, &kek_b).expect("unpack b with kek b");
    assert_eq!(query_symbol(&repo_b).expect("query restored b"), "symbol_b");
}
