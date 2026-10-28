use serde_json::{json, Value};
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

xavier::isolate_test_process!();

struct Fixture(TempDir);
impl Fixture {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }
    fn run(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        let root = self.0.path();
        let mut command = Command::new(env!("CARGO_BIN_EXE_xavier"));
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", root)
            .env("XDG_CONFIG_HOME", root.join("xdg-config"))
            .env("XDG_STATE_HOME", root.join("xdg-state"))
            .env("XAVIER_CONFIG_PATH", root.join("config.json"))
            .env("XAVIER_RUNTIME_STATE_PATH", root.join("runtime.json"))
            .env("XAVIER_DATA_DIR", root.join("data"))
            .current_dir(root)
            .stdin(Stdio::null())
            .args(args);
        for (key, value) in extra {
            command.env(key, value);
        }
        command.output().unwrap()
    }
    fn json(&self, args: &[&str], extra: &[(&str, &str)]) -> Value {
        let output = self.run(args, extra);
        assert!(
            output.status.success(),
            "status {:?}, stderr {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value =
            serde_json::from_slice(&output.stdout).expect("stdout is one JSON document");
        assert_eq!(value["schema_version"], 1);
        value
    }
    fn write(&self, name: &str, value: Value) {
        std::fs::write(
            self.0.path().join(name),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn config_provenance_per_layer_and_env_wins() {
    let f = Fixture::new();
    let args = ["config", "get", "server.port", "--json"];
    assert_eq!(f.json(&args, &[])["entry"]["source"], "default");
    f.write("config.json", json!({"server":{"port":8201}}));
    let value = f.json(&args, &[]);
    assert_eq!(value["entry"]["value"], 8201);
    assert_eq!(value["entry"]["source"], "config-file");
    f.write("runtime.json", json!({"server":{"port":8202}}));
    assert_eq!(f.json(&args, &[])["entry"]["source"], "runtime-state");
    let value = f.json(&args, &[("XAVIER_PORT", "8203")]);
    assert_eq!(value["entry"]["source"], "env");
    assert_eq!(value["entry"]["env_var"], "XAVIER_PORT");
    assert_eq!(value["entry"]["value"], 8203);
}

#[test]
fn config_set_unset_round_trip_preserves_other_layers_and_mode() {
    let f = Fixture::new();
    let config = json!({"server":{"port":8201}});
    f.write("config.json", config.clone());
    f.write("runtime.json", json!({"sync":{"max_retries":7}}));
    let set = f.json(&["config", "set", "server.port", "8202", "--json"], &[]);
    assert_eq!(set["restart_required"], true);
    assert_eq!(
        f.json(&["config", "get", "server.port", "--json"], &[])["entry"]["value"],
        8202
    );
    assert_eq!(
        f.json(&["config", "get", "sync.max_retries", "--json"], &[])["entry"]["value"],
        7
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(f.0.path().join("runtime.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    f.json(&["config", "unset", "server.port", "--json"], &[]);
    let entry = &f.json(&["config", "get", "server.port", "--json"], &[])["entry"];
    assert_eq!(entry["value"], 8201);
    assert_eq!(entry["source"], "config-file");
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(f.0.path().join("config.json")).unwrap())
            .unwrap(),
        config
    );
}

#[test]
fn config_rejects_unknown_keys_and_invalid_types() {
    let f = Fixture::new();
    for args in [
        vec!["config", "get", "server.typo", "--json"],
        vec!["config", "set", "server.port", "wrong", "--json"],
        vec!["config", "set", "server.port", "65536", "--json"],
        vec!["config", "unset", "server.typo", "--json"],
    ] {
        let output = f.run(&args, &[]);
        assert_eq!(output.status.code(), Some(2));
        let body: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(body["ok"], false);
    }
    assert!(!f.0.path().join("runtime.json").exists());
}

#[test]
fn config_secrets_require_file_and_are_never_echoed() {
    let f = Fixture::new();
    let secret = "fixture-private-license-value";
    std::fs::write(f.0.path().join("value.txt"), secret).unwrap();
    let rejected = f.run(
        &["config", "set", "license.commercial_key", secret, "--json"],
        &[],
    );
    assert_eq!(rejected.status.code(), Some(2));
    for output in [
        rejected,
        f.run(
            &[
                "config",
                "set",
                "license.commercial_key",
                "--value-file",
                "value.txt",
                "--json",
            ],
            &[],
        ),
        f.run(&["config", "list", "--json"], &[]),
        f.run(
            &["config", "explain", "license.commercial_key", "--json"],
            &[],
        ),
    ] {
        assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
    }
    assert_eq!(
        f.json(&["config", "get", "license.commercial_key", "--json"], &[])["entry"]["value"],
        "set"
    );
}

#[test]
fn config_validate_reports_unknown_types_and_relative_paths_without_values() {
    let f = Fixture::new();
    f.write("config.json", json!({"server":{"port":"private-invalid-port", "typo":42}, "memory":{"data_dir":"relative"}}));
    let output = f.run(&["config", "validate", "--json"], &[]);
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let problems = value["problems"].as_array().unwrap();
    assert!(problems.iter().any(|p| p["key"] == "server.typo"));
    assert!(problems.iter().any(|p| p["key"] == "server.port"));
    assert!(problems.iter().any(|p| p["key"] == "memory.data_dir"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("private-invalid-port"));
}

#[test]
fn config_explain_paths_and_json_on_every_command() {
    let f = Fixture::new();
    let paths = f.json(&["config", "path", "--json"], &[]);
    assert_eq!(
        paths["runtime_state_path"],
        f.0.path().join("runtime.json").to_str().unwrap()
    );
    let explain = f.json(
        &["config", "explain", "models.embedding_model", "--json"],
        &[],
    );
    assert_eq!(explain["entry"]["env_var"], "XAVIER_EMBEDDING_MODEL");
    assert_eq!(explain["restart_required"], true);
    assert!(explain["config_path"].is_string());
    assert!(f.json(&["config", "list", "--json"], &[])["entries"].is_array());
}

#[test]
fn config_types_and_env_model_override() {
    let f = Fixture::new();
    for (key, raw, expected) in [
        ("tgd.enabled", "false", json!(false)),
        ("retrieval.keyword_weight", "0.25", json!(0.25)),
        ("workspace.storage_limit_bytes", "123", json!(123)),
        (
            "models.local_model_dirs",
            "[\"/models/a\",\"/models/b\"]",
            json!(["/models/a", "/models/b"]),
        ),
        ("models.llm_model", "123", json!("123")),
    ] {
        f.json(&["config", "set", key, raw, "--json"], &[]);
        assert_eq!(
            f.json(&["config", "get", key, "--json"], &[])["entry"]["value"],
            expected
        );
    }
    f.json(
        &[
            "config",
            "set",
            "models.embedding_model",
            "from-runtime",
            "--json",
        ],
        &[],
    );
    let entry = &f.json(
        &["config", "get", "models.embedding_model", "--json"],
        &[("XAVIER_EMBEDDING_MODEL", "from-env")],
    )["entry"];
    assert_eq!(entry["value"], "from-env");
    assert_eq!(entry["source"], "env");
}

#[test]
fn config_validate_malformed_runtime_and_invalid_env_without_startup_failure() {
    let f = Fixture::new();
    std::fs::write(f.0.path().join("runtime.json"), b"{broken").unwrap();
    let output = f.run(
        &["config", "validate", "--json"],
        &[("XAVIER_PORT", "private-invalid-port")],
    );
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["problems"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["source"] == "runtime-state"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("private-invalid-port"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("private-invalid-port"));
}

#[test]
fn config_list_redacts_credentials_nested_in_lists() {
    let f = Fixture::new();
    f.write("config.json", json!({"workspace":{"mini_experts":[{"name":"fixture","provider":"local","endpoint":"http://localhost","api_key":"private-list-value"}]}}));
    for args in [
        vec!["config", "list", "--json"],
        vec!["config", "get", "workspace.mini_experts", "--json"],
    ] {
        let output = f.run(&args, &[]);
        assert!(output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("private-list-value"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-list-value"));
    }
}

#[test]
fn config_secret_from_stdin_and_noninteractive_missing_value() {
    use std::io::Write;
    let f = Fixture::new();
    let root = f.0.path();
    let mut child = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .env_clear()
        .env("HOME", root)
        .env("XAVIER_CONFIG_PATH", root.join("config.json"))
        .env("XAVIER_RUNTIME_STATE_PATH", root.join("runtime.json"))
        .env("XAVIER_DATA_DIR", root.join("data"))
        .current_dir(root)
        .args([
            "config",
            "set",
            "license.commercial_key",
            "--value-stdin",
            "--json",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"fixture-private-stdin-value\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("fixture-private-stdin-value"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-private-stdin-value"));
    assert_eq!(
        f.run(&["config", "set", "server.port", "--json"], &[])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn config_rejects_pointer_aliases_and_unknown_fields_in_lists() {
    let f = Fixture::new();
    assert_eq!(
        f.run(&["config", "get", "server/port", "--json"], &[])
            .status
            .code(),
        Some(2)
    );
    f.write("runtime.json", json!({"workspace":{"mini_experts":[{"name":"fixture", "provider":"local", "endpoint":"http://localhost", "typo":"private-unknown-value"}]}}));
    let output = f.run(&["config", "validate", "--json"], &[]);
    assert_eq!(output.status.code(), Some(1));
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(body["problems"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["key"] == "workspace.mini_experts.0.typo"));
}
