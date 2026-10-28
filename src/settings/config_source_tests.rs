use super::*;
use crate::settings::tests::TempEnv;

struct Fixture {
    root: tempfile::TempDir,
    _env: TempEnv,
}

impl Fixture {
    fn new() -> Self {
        crate::test_support::isolate_user_dirs();
        let env = TempEnv::new();
        for (key, _) in std::env::vars().filter(|(k, _)| k.starts_with("XAVIER_")) {
            std::env::remove_var(key);
        }
        let root = tempfile::tempdir().unwrap();
        std::env::set_var("XAVIER_CONFIG_PATH", root.path().join("config.json"));
        std::env::set_var(
            "XAVIER_RUNTIME_STATE_PATH",
            root.path().join("runtime.json"),
        );
        std::env::set_var("XAVIER_ENV_FILE", root.path().join("env"));
        std::fs::write(root.path().join("env"), "").unwrap();
        Self { root, _env: env }
    }

    fn config(&self, value: Value) {
        std::fs::write(self.root.path().join("config.json"), value.to_string()).unwrap();
    }
}

#[test]
fn config_source_relative_paths_use_effective_data_dir() {
    let f = Fixture::new();
    let data = f.root.path().join("store");
    let absolute = f.root.path().join("external.sqlite3");
    f.config(serde_json::json!({
        "memory": {"data_dir": data, "file_path": "data/memory.json",
            "sqlite_path": "memory-store.sqlite3", "vec_path": absolute,
            "workspace_dir": "workspaces"},
        "embedding": {"cache_db_path": "data/cache.db"},
        "server": {"code_graph_db_path": "data/code_graph.db"},
        "enterprise": {"db_path": "enterprise.db"},
        "data_commons": {"data_dir": "data/commons"},
        "router": {"policy_path": ""}
    }));
    let s = load().unwrap().unwrap();
    assert_eq!(
        Path::new(&s.memory.sqlite_path),
        data.join("memory-store.sqlite3")
    );
    assert_eq!(Path::new(&s.memory.file_path), data.join("memory.json"));
    assert_eq!(Path::new(&s.memory.vec_path), absolute);
    assert_eq!(Path::new(&s.memory.workspace_dir), data.join("workspaces"));
    assert_eq!(
        Path::new(&s.server.code_graph_db_path),
        data.join("code_graph.db")
    );
    assert_eq!(Path::new(&s.embedding.cache_db_path), data.join("cache.db"));
    assert_eq!(Path::new(&s.enterprise.db_path), data.join("enterprise.db"));
    assert_eq!(Path::new(&s.data_commons.data_dir), data.join("commons"));
    assert!(s.router.policy_path.is_empty());
}

#[test]
fn config_source_env_file_overrides_runtime_and_process_env_wins() {
    let f = Fixture::new();
    let runtime = f.root.path().join("runtime-store");
    let contract = f.root.path().join("contract-store");
    let process = f.root.path().join("process-store");
    f.config(serde_json::json!({"memory": {"data_dir": f.root.path().join("defaults")}}));
    std::fs::write(
        f.root.path().join("runtime.json"),
        serde_json::json!({"memory": {"data_dir": runtime}}).to_string(),
    )
    .unwrap();
    std::fs::write(f.root.path().join("env"), format!(
        "# shared contract\nXAVIER_DATA_DIR='{}'\nXAVIER_CODE_GRAPH_DB_PATH=graphs/contract.db\nIGNORED_CONFIG_SOURCE_KEY=ignored\n",
        contract.display())).unwrap();
    let s = load().unwrap().unwrap();
    assert_eq!(Path::new(&s.memory.data_dir), contract);
    assert_eq!(
        Path::new(&s.server.code_graph_db_path),
        contract.join("graphs/contract.db")
    );
    assert!(std::env::var_os("IGNORED_CONFIG_SOURCE_KEY").is_none());
    std::env::set_var("XAVIER_DATA_DIR", &process);
    std::env::set_var("XAVIER_CODE_GRAPH_DB_PATH", "data/process.db");
    let s = load().unwrap().unwrap();
    assert_eq!(Path::new(&s.memory.data_dir), &process);
    assert_eq!(
        Path::new(&s.server.code_graph_db_path),
        process.join("process.db")
    );
}

#[test]
fn config_source_relative_data_dir_is_home_relative_in_any_cwd() {
    const MARKER: &str = "XAVIER_CONFIG_SOURCE_CHILD";
    if let Some(root) = std::env::var_os(MARKER) {
        let home = PathBuf::from(root);
        for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("XAVIER_")) {
            std::env::remove_var(key);
        }
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
        std::env::set_var("XAVIER_CONFIG_PATH", home.join("config.json"));
        std::env::set_var("XAVIER_RUNTIME_STATE_PATH", home.join("runtime.json"));
        let s = load().unwrap().unwrap();
        assert_eq!(Path::new(&s.memory.data_dir), home.join("node-data"));
        assert_eq!(
            Path::new(&s.server.code_graph_db_path),
            home.join("node-data/code_graph.db")
        );
        assert_eq!(
            Path::new(&s.memory.sqlite_path),
            home.join("node-data/memory-store.sqlite3")
        );
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config.json");
    let env_dir = root.path().join(".config/xavier");
    std::fs::create_dir_all(&env_dir).unwrap();
    std::fs::write(env_dir.join("env"), "XAVIER_DATA_DIR=node-data\n").unwrap();
    std::fs::write(&config, r#"{"memory":{"data_dir":"other-data"}}"#).unwrap();
    for cwd in [root.path().join("first"), root.path().join("second")] {
        std::fs::create_dir_all(&cwd).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "settings::serialization::config_source_tests::config_source_relative_data_dir_is_home_relative_in_any_cwd", "--nocapture"])
            .env_clear()
            .env("HOME", root.path())
            .env("XAVIER_CONFIG_PATH", &config)
            .env("XAVIER_RUNTIME_STATE_PATH", root.path().join("runtime.json"))
            .env("XDG_CONFIG_HOME", root.path().join(".config"))
            .env(MARKER, root.path())
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn config_source_invalid_env_file_reports_line_without_value() {
    let f = Fixture::new();
    f.config(serde_json::json!({}));
    let private_value = "fixture-private-value";
    std::fs::write(
        f.root.path().join("env"),
        format!("XAVIER_TEST='{private_value}\n"),
    )
    .unwrap();
    let message = match load() {
        Err(error) => error.to_string(),
        Ok(_) => panic!("invalid quoting must fail closed"),
    };
    assert!(message.contains("line 1"), "{message}");
    assert!(!message.contains(private_value));
}

#[test]
fn config_source_path_env_overrides_are_exported_as_absolute_paths() {
    let f = Fixture::new();
    let data = f.root.path().join("store");
    f.config(serde_json::json!({"memory": {"data_dir": data}}));
    std::env::set_var("XAVIER_MEMORY_VEC_PATH", "data/vec.db");
    std::env::set_var("XAVIER_EMBEDDING_CACHE_DB_PATH", "cache.db");
    let s = load().unwrap().unwrap();
    s.apply_to_env();
    assert_eq!(
        PathBuf::from(std::env::var_os("XAVIER_MEMORY_VEC_PATH").unwrap()),
        data.join("vec.db")
    );
    assert_eq!(
        PathBuf::from(std::env::var_os("XAVIER_EMBEDDING_CACHE_DB_PATH").unwrap()),
        data.join("cache.db")
    );
    assert_eq!(
        crate::codebase::codegraph_paths::code_graph_db_path_for(Path::new(".")),
        data.join("code_graph.db")
    );
    assert_eq!(
        crate::memory::sqlite_vec_store::at_rest::record_key_path(),
        data.join("node/record.key")
    );
}

#[test]
fn config_source_runtime_overrides_defaults_without_contract_values() {
    let f = Fixture::new();
    let data = f.root.path().join("runtime-store");
    f.config(serde_json::json!({"memory": {"data_dir": f.root.path().join("defaults")}}));
    std::fs::write(
        f.root.path().join("runtime.json"),
        serde_json::json!({"memory": {"data_dir": data}}).to_string(),
    )
    .unwrap();
    let s = load().unwrap().unwrap();
    assert_eq!(Path::new(&s.memory.data_dir), data);
    assert_eq!(
        Path::new(&s.memory.sqlite_path),
        data.join("memory-store.sqlite3")
    );
}

#[test]
fn config_source_contract_is_literal_and_only_imports_xavier_keys() {
    let f = Fixture::new();
    f.config(serde_json::json!({}));
    std::fs::write(
        f.root.path().join("env"),
        "PATH=ignored\nXAVIER_CONTRACT_LITERAL=\"$(echo never-run) $HOME # literal\"\n",
    )
    .unwrap();
    let path = std::env::var_os("PATH");
    load().unwrap();
    assert_eq!(std::env::var_os("PATH"), path);
    assert_eq!(
        std::env::var("XAVIER_CONTRACT_LITERAL").unwrap(),
        "$(echo never-run) $HOME # literal"
    );
}
