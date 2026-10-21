//! Integration test for per-repo memory store isolation and registry opt-in.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use xavier::codebase::repo_package_crypto::derive_repo_record_key_from_master;
use xavier::memory::connection_provider::InMemoryProvider;
use xavier::memory::repo_store_registry::{RepoRecordKeySource, RepoStoreError, RepoStoreRegistry};
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::memory::store::{MemoryRecord, MemoryStore};

xavier::isolate_test_process!();

const TEST_MASTER: [u8; 32] = [0x3C; 32];
const OPT_IN_CONFIG: &str = "memory_store = \"repo\"\n";

struct TestKeys;

impl RepoRecordKeySource for TestKeys {
    fn record_key(&self, project_id: &str) -> anyhow::Result<[u8; 32]> {
        derive_repo_record_key_from_master(&TEST_MASTER, project_id)
    }
}

fn create_repo(base: &Path, name: &str, config: Option<&str>) -> PathBuf {
    let root = base.join(name);
    std::fs::create_dir_all(root.join(".git")).expect("mkdir .git");
    if let Some(body) = config {
        std::fs::create_dir_all(root.join(".xavier")).expect("mkdir .xavier");
        std::fs::write(root.join(".xavier/config.toml"), body).expect("write config.toml");
    }
    root.canonicalize().expect("canonical repo root")
}

fn create_registry(allowed: &[&Path]) -> RepoStoreRegistry {
    let roots: Vec<PathBuf> = allowed.iter().map(|p| p.to_path_buf()).collect();
    RepoStoreRegistry::new(
        &roots,
        Arc::new(InMemoryProvider::new()),
        Arc::new(TestKeys),
    )
}

fn sample_record(id: &str, content: &str) -> MemoryRecord {
    MemoryRecord {
        id: id.to_string(),
        workspace_id: "repo".to_string(),
        path: format!("notes/{id}"),
        content: content.to_string(),
        ..Default::default()
    }
}

#[tokio::test]
async fn test_opted_in_repo_saved_via_registry_absent_from_global_store() {
    let tmp = TempDir::new().expect("tempdir");
    let repo_a = create_repo(tmp.path(), "repo-alpha", Some(OPT_IN_CONFIG));
    let reg = create_registry(&[tmp.path()]);

    let global_store_path = tmp.path().join("global_vec_store.sqlite3");
    let global_config = VecSqliteStoreConfig {
        path: global_store_path,
        embedding_dimensions: 1536,
    };
    let global_store = VecSqliteMemoryStore::new(global_config)
        .await
        .expect("create global store");

    let repo_store_a = reg.open(&repo_a).await.expect("open repo a store");
    assert!(repo_a.join(".xavier/memory.sqlite3").is_file());

    let rec_a = sample_record("record-a", "alpha isolated knowledge");
    repo_store_a
        .put(rec_a.clone())
        .await
        .expect("put record into repo a store");

    // Row is present in the repo store
    let retrieved_a = repo_store_a
        .get("repo", "record-a")
        .await
        .expect("get from repo store")
        .expect("row must exist in repo store");
    assert_eq!(retrieved_a.content, "alpha isolated knowledge");

    // Row is strictly absent from the global store
    assert!(
        global_store
            .get("repo", "record-a")
            .await
            .expect("get from global store")
            .is_none(),
        "repo record must be absent from global store"
    );
    assert!(
        global_store
            .get("default", "record-a")
            .await
            .expect("get default from global store")
            .is_none(),
        "repo record must not leak into default workspace of global store"
    );
}

#[tokio::test]
async fn test_two_opted_in_repos_are_isolated() {
    let tmp = TempDir::new().expect("tempdir");
    let repo_a = create_repo(tmp.path(), "repo-alpha", Some(OPT_IN_CONFIG));
    let repo_b = create_repo(tmp.path(), "repo-beta", Some(OPT_IN_CONFIG));
    let reg = create_registry(&[tmp.path()]);

    let store_a = reg.open(&repo_a).await.expect("open repo a");
    let store_b = reg.open(&repo_b).await.expect("open repo b");

    assert_ne!(
        store_a.connection_project_id(),
        store_b.connection_project_id()
    );

    let rec_a = sample_record("rec-alpha", "confidential to repo A");
    let rec_b = sample_record("rec-beta", "confidential to repo B");

    store_a.put(rec_a).await.expect("put a");
    store_b.put(rec_b).await.expect("put b");

    // Store A has A but not B
    assert!(store_a.get("repo", "rec-alpha").await.unwrap().is_some());
    assert!(
        store_a.get("repo", "rec-beta").await.unwrap().is_none(),
        "repo A must not see repo B records"
    );

    // Store B has B but not A
    assert!(store_b.get("repo", "rec-beta").await.unwrap().is_some());
    assert!(
        store_b.get("repo", "rec-alpha").await.unwrap().is_none(),
        "repo B must not see repo A records"
    );
}

#[tokio::test]
async fn test_non_opted_repo_is_refused() {
    let tmp = TempDir::new().expect("tempdir");
    let unconfigured_repo = create_repo(tmp.path(), "repo-no-config", None);
    let non_opted_repo = create_repo(
        tmp.path(),
        "repo-not-opted",
        Some("memory_store = \"global\"\n"),
    );
    let reg = create_registry(&[tmp.path()]);

    let err_unconfigured = match reg.open(&unconfigured_repo).await {
        Ok(_) => panic!("unconfigured repo must be refused"),
        Err(err) => err,
    };
    assert!(
        matches!(err_unconfigured, RepoStoreError::NotOptedIn { .. }),
        "expected NotOptedIn error, got: {err_unconfigured:?}"
    );
    assert!(!unconfigured_repo.join(".xavier/memory.sqlite3").exists());

    let err_non_opted = match reg.open(&non_opted_repo).await {
        Ok(_) => panic!("non-opted repo must be refused"),
        Err(err) => err,
    };
    assert!(
        matches!(err_non_opted, RepoStoreError::NotOptedIn { .. }),
        "expected NotOptedIn error, got: {err_non_opted:?}"
    );
    assert!(!non_opted_repo.join(".xavier/memory.sqlite3").exists());
}

#[tokio::test]
async fn test_outside_allowlist_repo_is_refused() {
    let tmp = TempDir::new().expect("tempdir");
    let allowed_dir = tmp.path().join("allowed");
    let outside_dir = tmp.path().join("outside");
    std::fs::create_dir_all(&allowed_dir).expect("mkdir allowed");
    std::fs::create_dir_all(&outside_dir).expect("mkdir outside");

    let repo = create_repo(&outside_dir, "outside-repo", Some(OPT_IN_CONFIG));
    let reg = create_registry(&[&allowed_dir]);

    let err = match reg.open(&repo).await {
        Ok(_) => panic!("outside allowlist must be refused"),
        Err(err) => err,
    };
    assert!(
        matches!(err, RepoStoreError::OutsideAllowlist { .. }),
        "expected OutsideAllowlist error, got: {err:?}"
    );
    assert!(!repo.join(".xavier/memory.sqlite3").exists());
}
