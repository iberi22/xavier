//! Logical concern: Xavier settings module entry point.
//!
//! This module re-exports sub-modules and defines the main interface for settings.

use anyhow::Result;
use parking_lot::RwLock;
use std::path::PathBuf;
#[cfg(not(test))]
use std::sync::Once;
use std::sync::{Arc, LazyLock};

pub mod defaults;
pub mod env;
pub mod serialization;
pub mod types;
pub mod validation;

pub use types::*;

pub static GLOBAL_SETTINGS: LazyLock<Arc<RwLock<XavierSettings>>> = LazyLock::new(|| {
    let settings = serialization::current();
    Arc::new(RwLock::new(settings))
});

#[cfg(not(test))]
static WATCHER_INIT: Once = Once::new();

#[cfg(not(test))]
fn ensure_watcher_started() {
    WATCHER_INIT.call_once(|| {
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async move {
                if let Err(e) = watch_config_changes().await {
                    tracing::error!("Failed to start config hot-reload watcher: {:?}", e);
                }
            });
        }
    });
}

#[cfg(not(test))]
async fn watch_config_changes() -> Result<()> {
    let config_path = serialization::resolve_config_path();
    // Runtime state is written by this very process (HORMER, /v1/settings, the
    // CLI). Watching it too keeps the same behaviour the in-place config write
    // used to provide: after a save, GLOBAL_SETTINGS converges on what was
    // saved. Reload never writes, so this cannot loop.
    let state_path = serialization::resolve_runtime_state_path(&config_path);
    watch_config_changes_impl(vec![config_path, state_path]).await
}

async fn watch_config_changes_impl(paths: Vec<std::path::PathBuf>) -> Result<()> {
    use notify::{RecursiveMode, Watcher};
    use std::collections::HashSet;
    use std::time::Duration;

    // Group the watched files by parent directory: notify reports per directory,
    // and several config/state files can live in the same one.
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    let mut watched_names: HashSet<String> = HashSet::new();
    for path in &paths {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        if !dirs.contains(&parent) {
            dirs.push(parent);
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            watched_names.insert(name.to_string());
        }
    }

    if watched_names.is_empty() {
        tracing::debug!("no config paths resolved, watcher skipped");
        return Ok(());
    }

    let existing: Vec<_> = dirs.iter().filter(|d| d.exists()).cloned().collect();
    if existing.is_empty() {
        tracing::debug!("config dirs absent, watcher skipped: {:?}", dirs);
        return Ok(());
    }

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let mut watcher = notify::RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        notify::Config::default(),
    )?;

    // Watch parent directories to handle atomic saves correctly
    for dir in &existing {
        watcher.watch(dir, RecursiveMode::NonRecursive)?;
    }
    tracing::info!(
        "Xavier Settings Watcher: Monitoring {:?} for changes to {:?}",
        existing,
        watched_names
    );

    // Keep watcher alive in this task's scope
    let _watcher = watcher;

    while let Some(res) = rx.recv().await {
        match res {
            Ok(event) => {
                let is_write_event = matches!(
                    event.kind,
                    notify::EventKind::Modify(_) | notify::EventKind::Create(_)
                );

                if is_write_event {
                    let matches_path = event.paths.iter().any(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| watched_names.contains(n))
                    });

                    if matches_path {
                        tracing::info!(
                            "Xavier config/runtime state changed. Reloading settings..."
                        );
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        if let Err(e) = XavierSettings::reload() {
                            tracing::error!("Failed to reload settings: {:?}", e);
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!("Config watcher error: {:?}", e);
            }
        }
    }

    Ok(())
}

impl XavierSettings {
    #[cfg_attr(not(test), allow(dead_code))]
    /// Resolve config path.
    pub fn resolve_config_path() -> PathBuf {
        serialization::resolve_config_path()
    }

    /// Resolve data dir.
    pub fn resolve_data_dir() -> PathBuf {
        serialization::resolve_data_dir()
    }

    /// Resolve runtime state path (where `save()` writes). Never the versioned
    /// config file — see `serialization` for the defaults/state split (#2801).
    pub fn resolve_runtime_state_path() -> PathBuf {
        serialization::resolve_runtime_state_path(&serialization::resolve_config_path())
    }

    /// Load.
    pub fn load() -> Result<Option<Self>> {
        serialization::load()
    }

    /// Apply to env.
    pub fn apply_to_env(&self) {
        env::apply_to_env_impl(self);
    }

    /// Current.
    pub fn current() -> Self {
        #[cfg(test)]
        {
            serialization::current()
        }
        #[cfg(not(test))]
        {
            ensure_watcher_started();
            GLOBAL_SETTINGS.read().clone()
        }
    }

    /// Reload.
    pub fn reload() -> Result<()> {
        let settings = serialization::current();
        settings.apply_to_env();
        let mut lock = GLOBAL_SETTINGS.write();
        *lock = settings;
        Ok(())
    }

    /// Save.
    pub async fn save(&self) -> Result<()> {
        serialization::save(self).await
    }

    /// Client base url.
    pub fn client_base_url(&self) -> String {
        let host = match self.server.host.as_str() {
            "0.0.0.0" | "::" => "127.0.0.1",
            other => other,
        };
        format!("http://{}:{}", host, self.server.port)
    }
}

#[cfg(test)]
pub mod tests {
    use super::validation::non_empty;
    use super::*;
    use std::sync::{LazyLock, Mutex};

    pub static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    pub struct TempEnv {
        saved_vars: std::collections::HashMap<String, String>,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl TempEnv {
        pub fn new() -> Self {
            let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut saved_vars = std::collections::HashMap::new();
            for (key, val) in std::env::vars() {
                let lower = key.to_ascii_uppercase();
                if lower.starts_with("XAVIER_")
                    || lower.starts_with("PGHEART_")
                    || lower == "STRIPE_SECRET_KEY"
                    || lower == "OPENAI_API_KEY"
                {
                    saved_vars.insert(key, val);
                }
            }
            Self {
                saved_vars,
                _guard: guard,
            }
        }
    }

    impl Drop for TempEnv {
        fn drop(&mut self) {
            // Remove any current XAVIER_, PGHEART_, or test variables that are set but weren't originally saved
            for (key, _) in std::env::vars() {
                let lower = key.to_ascii_uppercase();
                if (lower.starts_with("XAVIER_")
                    || lower.starts_with("PGHEART_")
                    || lower == "STRIPE_SECRET_KEY"
                    || lower == "OPENAI_API_KEY")
                    && !self.saved_vars.contains_key(&key)
                {
                    std::env::remove_var(&key);
                }
            }
            // Restore saved ones
            for (key, val) in &self.saved_vars {
                std::env::set_var(key, val);
            }
        }
    }

    #[test]
    fn test_default_settings() {
        let settings = XavierSettings::default();
        assert_eq!(settings.server.port, 8006);
        assert_eq!(settings.server.host, "127.0.0.1");
        assert_eq!(settings.workspace.default_workspace_id, "default");
        assert_eq!(settings.memory.backend, "vec");
        assert_eq!(settings.models.provider, "local");
        assert!(settings.retrieval.disable_hyde);
        assert_eq!(settings.sync.interval_ms, 300_000);
        assert_eq!(settings.advanced.qjl_threshold, 500);
        assert!(settings.advanced.entity_extraction_enabled);
        assert!(settings.advanced.audit_chain_enabled);
        assert_eq!(settings.memory_layers.working.capacity, 100);
        assert_eq!(settings.memory_layers.episodic.max_sessions, 50);
        assert!(!settings.telegram.enabled);
        assert_eq!(settings.enterprise.db_path, "data/enterprise.db");
    }

    /// Read the versioned defaults file if the repo has one.
    ///
    /// #2805: this test used to `set_var("XAVIER_CONFIG_PATH",
    /// "config/xavier.config.json")` and then assert exact values out of it,
    /// which made it an assertion about a file a live daemon rewrites. The
    /// versioned file is now read-only defaults, but the invariant worth
    /// keeping is the *contract* (local-first defaults are parseable and
    /// complete), not the exact string a PR might have changed. Values that
    /// encode a live policy (learned weights, license acceptance, tokens) are
    /// deliberately absent here — they are runtime state, and a daemon writes
    /// them outside the git tree.
    fn read_versioned_defaults() -> Option<XavierSettings> {
        // The tracked template, not the runtime file (#2801): the latter is
        // untracked and may be rewritten or absent.
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/xavier.config.example.json");
        let raw = std::fs::read_to_string(&path).ok()?;
        Some(
            serde_json::from_str::<XavierSettings>(&raw).unwrap_or_else(|e| {
                panic!(
                    "versioned defaults at {} must parse as XavierSettings: {}",
                    path.display(),
                    e
                )
            }),
        )
    }

    #[test]
    fn test_load_config_json() {
        let _env = TempEnv::new();

        // Read-only defaults: the versioned file is a plain parse, with no env
        // var pointing at it and no daemon able to mutate it underneath us.
        let Some(s) = read_versioned_defaults() else {
            // The suite must not depend on the repo layout; when the file is
            // absent the compiled-in defaults are the contract.
            let s = XavierSettings::default();
            assert_eq!(s.workspace.embedding_provider_mode, "local");
            return;
        };

        // Local-first defaults from Step 1: embedding model, provider mode and
        // the embedder endpoint stay pinned to the local stack.
        assert_eq!(s.workspace.embedding_provider_mode, "local");
        assert!(
            s.models.embedding_model == "embeddinggemma"
                || s.models.embedding_model == "nomic-embed-text"
                || s.models.embedding_model == "nomic-embed-text:latest"
        );
        assert_eq!(
            s.embedding.endpoint,
            "http://localhost:11434/api/embeddings"
        );
        assert_eq!(s.embedding.embedder, "local");
        assert!(
            s.embedding.gllm_model == "embeddinggemma"
                || s.embedding.gllm_model == "nomic-embed-text"
                || s.embedding.gllm_model == "nomic-embed-text:latest"
        );

        // Assertions for provider=local and local_llm_* fields.
        // The exact model name / URL are operator-pinned workspace values
        // (they evolve with merged PRs), so assert the contractual shape —
        // local provider, non-empty model, valid http(s) URL — instead of
        // exact strings. Pinning exact values here made the suite fail on
        // every model bump (CI Parallel Rust Tests).
        assert_eq!(s.models.provider, "local");
        assert!(
            !s.models.local_llm_model.trim().is_empty(),
            "local_llm_model must be non-empty"
        );
        assert!(
            s.models.local_llm_url.starts_with("http://")
                || s.models.local_llm_url.starts_with("https://"),
            "local_llm_url must be a valid http(s) URL, got '{}'",
            s.models.local_llm_url
        );
    }

    /// #2805: loading must succeed with *any* content in the config file, and
    /// the values a daemon owns must not leak in from it.
    #[test]
    fn test_load_config_json_is_independent_of_config_content() {
        let _env = TempEnv::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("xavier.config.json");
        let state_path = dir.path().join("xavier.runtime.json");

        // A config whose router models are set: the old assertion
        // `router_fast_model == ""` turns red on exactly this content.
        let hostile = serde_json::json!({
            "models": {
                "router_fast_model": "some/other-model",
                "router_quality_model": "some/other-quality-model"
            },
            "retrieval": { "learned_policy": { "working_weight": 0.91, "update_count": 4242 } }
        });
        std::fs::write(&config_path, hostile.to_string()).expect("write config");
        std::env::set_var("XAVIER_CONFIG_PATH", &config_path);
        std::env::set_var("XAVIER_RUNTIME_STATE_PATH", &state_path);

        let loaded = XavierSettings::load().expect("load").expect("settings");

        // #2805: the test must survive ANY content in the config. The old
        // assertion `router_fast_model == ""` was an assertion about a file a
        // live daemon could rewrite, and it turned red on exactly this
        // content. Loading must now succeed and honour what is there.
        assert_eq!(loaded.models.router_fast_model, "some/other-model");
        assert_eq!(
            loaded.models.router_quality_model,
            "some/other-quality-model"
        );
        // Partial nested objects are tolerated: absent fields fall back to the
        // compiled-in defaults instead of failing the parse.
        assert_eq!(loaded.retrieval.learned_policy.working_weight, 0.91);
        assert_eq!(loaded.retrieval.learned_policy.semantic_weight, 0.4);

        std::env::remove_var("XAVIER_CONFIG_PATH");
        std::env::remove_var("XAVIER_RUNTIME_STATE_PATH");
    }

    /// D11: `save()` must not persist credentials.
    #[tokio::test]
    async fn test_save_does_not_persist_secrets() {
        let _env = TempEnv::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("xavier.config.json");
        let state_path = dir.path().join("xavier.runtime.json");
        std::env::set_var("XAVIER_CONFIG_PATH", &config_path);
        std::env::set_var("XAVIER_RUNTIME_STATE_PATH", &state_path);

        let mut settings = XavierSettings::default();
        settings.security.token_secret = Some("tok-secret-value".into());
        settings.embedding.api_key = Some("emb-key-value".into());
        settings.pgheart.token = Some("pg-token-value".into());
        settings.telegram.bot_token = Some("tg-token-value".into());
        settings.save().await.expect("save");

        let raw = std::fs::read_to_string(&state_path).expect("read state");
        for needle in [
            "tok-secret-value",
            "emb-key-value",
            "pg-token-value",
            "tg-token-value",
        ] {
            assert!(!raw.contains(needle), "{needle} leaked into runtime state");
        }

        std::env::remove_var("XAVIER_CONFIG_PATH");
        std::env::remove_var("XAVIER_RUNTIME_STATE_PATH");
    }

    /// #2801: `save()` must land in the runtime state path and leave the
    /// versioned config byte-for-byte untouched.
    #[tokio::test]
    async fn test_save_writes_runtime_state_and_leaves_config_alone() {
        let _env = TempEnv::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("xavier.config.json");
        let state_path = dir.path().join("xavier.runtime.json");

        let config_body = serde_json::to_string_pretty(&XavierSettings::default()).unwrap();
        std::fs::write(&config_path, &config_body).expect("write config");

        std::env::set_var("XAVIER_CONFIG_PATH", &config_path);
        std::env::set_var("XAVIER_RUNTIME_STATE_PATH", &state_path);

        let mut settings = XavierSettings::load().expect("load").expect("settings");
        settings.retrieval.learned_policy.working_weight = 0.29999998_f32;
        settings.retrieval.learned_policy.update_count = 7;
        settings.save().await.expect("save");

        assert!(
            state_path.exists(),
            "runtime state must be written to the state path"
        );
        let state_raw = std::fs::read_to_string(&state_path).expect("read state");
        let state_json: serde_json::Value = serde_json::from_str(&state_raw).expect("parse state");
        assert_eq!(state_json["retrieval"]["learned_policy"]["update_count"], 7);
        assert_eq!(
            state_json["retrieval"]["learned_policy"]["working_weight"],
            0.29999998
        );

        assert_eq!(
            std::fs::read_to_string(&config_path).expect("read config"),
            config_body,
            "the versioned config must not be rewritten by save()"
        );

        std::env::remove_var("XAVIER_CONFIG_PATH");
        std::env::remove_var("XAVIER_RUNTIME_STATE_PATH");
    }

    /// #2801: state written by `save()` is read back by `load()` as an overlay
    /// on top of the defaults, without the defaults file changing.
    #[tokio::test]
    async fn test_runtime_state_overlays_defaults_on_next_load() {
        let _env = TempEnv::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("xavier.config.json");
        let state_path = dir.path().join("xavier.runtime.json");

        let mut base = XavierSettings::default();
        base.server.port = 8006;
        base.retrieval.learned_policy.update_count = 0;
        let config_body = serde_json::to_string_pretty(&base).unwrap();
        std::fs::write(&config_path, &config_body).expect("write config");

        std::env::set_var("XAVIER_CONFIG_PATH", &config_path);
        std::env::set_var("XAVIER_RUNTIME_STATE_PATH", &state_path);

        // Clean machine: no state file yet, the defaults stand on their own.
        let fresh = XavierSettings::load().expect("load").expect("settings");
        assert_eq!(fresh.server.port, 8006);
        assert_eq!(fresh.retrieval.learned_policy.update_count, 0);

        let mut saved = fresh.clone();
        saved.server.port = 9999;
        saved.retrieval.learned_policy.update_count = 12;
        saved.save().await.expect("save");

        let reloaded = XavierSettings::load().expect("load").expect("settings");
        assert_eq!(reloaded.server.port, 9999, "state must win on reload");
        assert_eq!(reloaded.retrieval.learned_policy.update_count, 12);
        // Untouched keys keep the defaults, they are not zeroed by the overlay.
        assert_eq!(reloaded.workspace.default_workspace_id, "default");
        assert_eq!(
            std::fs::read_to_string(&config_path).expect("read config"),
            config_body,
            "overlay must not rewrite the versioned config"
        );

        std::env::remove_var("XAVIER_CONFIG_PATH");
        std::env::remove_var("XAVIER_RUNTIME_STATE_PATH");
    }

    /// A corrupt state file degrades to the defaults instead of bricking boot.
    #[test]
    fn test_corrupt_runtime_state_falls_back_to_defaults() {
        let _env = TempEnv::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("xavier.config.json");
        let state_path = dir.path().join("xavier.runtime.json");

        let base = XavierSettings::default();
        std::fs::write(
            &config_path,
            serde_json::to_string_pretty(&base).expect("serialize"),
        )
        .expect("write config");
        std::fs::write(&state_path, "{ not json").expect("write state");

        std::env::set_var("XAVIER_CONFIG_PATH", &config_path);
        std::env::set_var("XAVIER_RUNTIME_STATE_PATH", &state_path);

        let loaded = XavierSettings::load().expect("load").expect("settings");
        assert_eq!(loaded.server.port, base.server.port);

        std::env::remove_var("XAVIER_CONFIG_PATH");
        std::env::remove_var("XAVIER_RUNTIME_STATE_PATH");
    }

    /// The state path is never the config path, and it is configurable.
    #[test]
    fn test_runtime_state_path_is_outside_the_config_file() {
        let _env = TempEnv::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("xavier.config.json");
        std::fs::write(&config_path, "{}").expect("write config");
        std::env::set_var("XAVIER_CONFIG_PATH", &config_path);

        // No explicit pin, no XAVIER_DATA_DIR: the XDG state home.
        let state = XavierSettings::resolve_runtime_state_path();
        assert_ne!(state, config_path, "state must not be the config file");
        assert!(
            !state.starts_with("config"),
            "state must not live in the git tree, got {}",
            state.display()
        );

        std::env::set_var("XAVIER_RUNTIME_STATE_PATH", dir.path().join("pinned.json"));
        assert_eq!(
            XavierSettings::resolve_runtime_state_path(),
            dir.path().join("pinned.json")
        );

        std::env::remove_var("XAVIER_RUNTIME_STATE_PATH");
        std::env::remove_var("XAVIER_CONFIG_PATH");
    }

    #[test]
    fn test_apply_to_env_sets_vars() {
        let _env = TempEnv::new();

        // Clean env
        for (key, _) in std::env::vars() {
            if key.starts_with("XAVIER_") || key == "STRIPE_SECRET_KEY" {
                std::env::remove_var(&key);
            }
        }

        let settings = XavierSettings::default();
        settings.apply_to_env();

        assert_eq!(std::env::var("XAVIER_HOST").unwrap(), "127.0.0.1");
        assert_eq!(std::env::var("XAVIER_PORT").unwrap(), "8006");
        assert_eq!(
            std::env::var("XAVIER_WORKING_MEMORY_CAPACITY").unwrap(),
            "100"
        );
        assert_eq!(std::env::var("XAVIER_QJL_THRESHOLD").unwrap(), "500");
        assert_eq!(std::env::var("XAVIER_TELEGRAM_ENABLED").unwrap(), "false");
        assert_eq!(
            std::env::var("XAVIER_ENTERPRISE_DB_PATH").unwrap(),
            "data/enterprise.db"
        );
    }

    #[test]
    fn test_apply_to_env_respects_existing_vars() {
        let _env = TempEnv::new();

        // Set an override
        std::env::set_var("XAVIER_PORT", "9999");
        std::env::set_var("XAVIER_RRF_K", "100");
        std::env::remove_var("XAVIER_HOST");

        let settings = XavierSettings::default();
        settings.apply_to_env();

        // Existing vars should NOT be overwritten
        assert_eq!(std::env::var("XAVIER_PORT").unwrap(), "9999");
        assert_eq!(std::env::var("XAVIER_RRF_K").unwrap(), "100");
        // Missing vars should be set
        assert_eq!(std::env::var("XAVIER_HOST").unwrap(), "127.0.0.1");
    }

    #[test]
    fn test_config_file_missing_returns_none() {
        let _env = TempEnv::new();

        let path = std::env::temp_dir().join("nonexistent_xavier_config.json");
        // Ensure it doesn't exist
        let _ = std::fs::remove_file(&path);

        // Temporarily redirect config path
        std::env::set_var("XAVIER_CONFIG_PATH", path.to_str().unwrap());
        let result = XavierSettings::load().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_current_falls_back_to_defaults() {
        let _env = TempEnv::new();

        // Remove XAVIER_TOKEN if present
        std::env::remove_var("XAVIER_TOKEN");

        // Temporarily point config to a nonexistent path so defaults are used
        std::env::set_var("XAVIER_CONFIG_PATH", "/tmp/nonexistent-xavier-config.json");
        let settings = XavierSettings::current();
        // Without a real config file, host falls to the loopback default
        assert_eq!(settings.server.host, "127.0.0.1");
        assert_eq!(settings.server.port, 8006);
        assert!(settings.auth_token.is_none());
    }

    #[test]
    fn test_non_empty_helper() {
        assert_eq!(non_empty(""), None);
        assert_eq!(non_empty("   "), None);
        assert_eq!(non_empty("hello"), Some("hello".to_string()));
        assert_eq!(non_empty("  world  "), Some("world".to_string()));
    }

    #[test]
    fn test_client_base_url() {
        let settings = XavierSettings::default();
        let url = settings.client_base_url();
        assert!(url.starts_with("http://"));
        assert!(url.contains("127.0.0.1"));
        assert!(url.contains("8006"));
    }

    #[test]
    fn test_resolve_data_dir() {
        // Without env var, should return platform data dir or "data"
        let data_dir = XavierSettings::resolve_data_dir();
        assert!(!data_dir.as_os_str().is_empty());
    }

    #[test]
    fn test_apply_to_env_all_new_sections() {
        let _env = TempEnv::new();

        // Clean all XAVIER_ vars
        for (key, _) in std::env::vars() {
            if key.starts_with("XAVIER_") {
                std::env::remove_var(&key);
            }
        }

        let settings = XavierSettings::default();
        settings.apply_to_env();

        // Memory layers
        assert_eq!(
            std::env::var("XAVIER_WORKING_MEMORY_CAPACITY").unwrap(),
            "100"
        );
        assert_eq!(std::env::var("XAVIER_WORKING_LRU_THRESHOLD").unwrap(), "2");
        assert_eq!(std::env::var("XAVIER_WORKING_BM25_K1").unwrap(), "1.5");
        assert_eq!(std::env::var("XAVIER_WORKING_BM25_B").unwrap(), "0.75");
        assert_eq!(
            std::env::var("XAVIER_EPISODIC_SUMMARY_WINDOW").unwrap(),
            "10"
        );
        assert_eq!(std::env::var("XAVIER_MAX_EPISODIC_SESSIONS").unwrap(), "50");
        assert_eq!(
            std::env::var("XAVIER_EPISODIC_MIN_EVENT_IMPORTANCE").unwrap(),
            "0.5"
        );
        assert_eq!(
            std::env::var("XAVIER_EPISODIC_LLM_SUMMARY_ENABLED").unwrap(),
            "false"
        );

        // Advanced
        assert_eq!(std::env::var("XAVIER_QJL_THRESHOLD").unwrap(), "500");
        assert_eq!(
            std::env::var("XAVIER_ENTITY_EXTRACTION_ENABLED").unwrap(),
            "true"
        );
        assert_eq!(std::env::var("XAVIER_AUDIT_CHAIN_ENABLED").unwrap(), "true");

        // Router
        assert_eq!(
            std::env::var("XAVIER_ROUTER_POLICY_REFRESH_SECS").unwrap(),
            "300"
        );

        // Enterprise
        assert_eq!(
            std::env::var("XAVIER_ENTERPRISE_DB_PATH").unwrap(),
            "data/enterprise.db"
        );
    }

    #[test]
    fn test_reload_updates_global_settings() {
        let _env = TempEnv::new();

        // Save initial GLOBAL_SETTINGS
        let original_global = GLOBAL_SETTINGS.read().clone();

        // Ensure we restore it on drop
        struct GlobalSettingsRestore(XavierSettings);
        impl Drop for GlobalSettingsRestore {
            fn drop(&mut self) {
                let mut lock = GLOBAL_SETTINGS.write();
                *lock = self.0.clone();
            }
        }
        let _restore = GlobalSettingsRestore(original_global);

        // Temporarily point to a test config file
        let temp_dir = std::env::temp_dir();
        let test_config_path = temp_dir.join("test_xavier_config_reload.json");

        let initial_settings = XavierSettings::default();
        let mut modified_settings = XavierSettings::default();
        modified_settings.server.port = 9999;

        // Write initial settings to file
        let raw_initial = serde_json::to_string_pretty(&initial_settings).unwrap();
        std::fs::write(&test_config_path, raw_initial).unwrap();

        std::env::set_var("XAVIER_CONFIG_PATH", test_config_path.to_str().unwrap());

        // Reload global settings
        XavierSettings::reload().unwrap();
        assert_eq!(GLOBAL_SETTINGS.read().server.port, 8006);

        // Write modified settings
        let raw_modified = serde_json::to_string_pretty(&modified_settings).unwrap();
        std::fs::write(&test_config_path, raw_modified).unwrap();

        // Reload again
        XavierSettings::reload().unwrap();
        assert_eq!(GLOBAL_SETTINGS.read().server.port, 9999);

        // Clean up
        let _ = std::fs::remove_file(&test_config_path);
    }

    #[test]
    fn test_default_host_is_loopback_not_wildcard() {
        // Regression: the daemon used to default to 0.0.0.0, which published the
        // whole authenticated API — secret routes included — to every interface.
        // A node that must listen wider sets XAVIER_HOST or server.host explicitly.
        let host = XavierSettings::default().server.host;
        assert_eq!(
            host, "127.0.0.1",
            "the default bind must stay on loopback; widening it has to be an explicit choice"
        );
        assert_ne!(
            XavierSettings::default_host(),
            "0.0.0.0",
            "default_host is what a config without a `server.host` inherits"
        );
    }

    #[tokio::test]
    async fn test_watcher_skipped_when_dir_absent() {
        let nonexistent_path = std::path::PathBuf::from("/nonexistent/path/xavier.config.json");
        let result = super::watch_config_changes_impl(vec![nonexistent_path]).await;
        assert!(
            result.is_ok(),
            "Watcher should gracefully succeed and return Ok when parent dir is absent"
        );
    }

    #[tokio::test]
    async fn test_watcher_skipped_when_no_paths() {
        let result = super::watch_config_changes_impl(Vec::new()).await;
        assert!(
            result.is_ok(),
            "Watcher should succeed when there is nothing to watch"
        );
    }

    #[tokio::test]
    async fn test_watcher_starts_when_dir_present() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("xavier.config.json");
        let state_path = dir.path().join("xavier.runtime.json");

        // Create the dummy files so that the parent and files exist
        std::fs::write(&config_path, "{}").unwrap();
        std::fs::write(&state_path, "{}").unwrap();

        // Since the watcher loops forever awaiting RX, let's run it with a short timeout
        let run_watcher = super::watch_config_changes_impl(vec![config_path, state_path]);
        let result = tokio::time::timeout(std::time::Duration::from_millis(100), run_watcher).await;

        // It should time out (since it waits for changes in a loop), which proves it started up properly and didn't fail immediately
        assert!(
            result.is_err(),
            "Watcher should run indefinitely when parent dir exists, thus timing out here"
        );
    }
}
