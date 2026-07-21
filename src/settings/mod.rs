// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Logical concern: Xavier settings module entry point.
//!
//! This module re-exports sub-modules and defines the main interface for settings.

use anyhow::Result;
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
#[cfg(not(test))]
use std::sync::Once;

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
    use notify::{RecursiveMode, Watcher};
    use std::time::Duration;

    let path = serialization::resolve_config_path();
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let config_file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("xavier.config.json")
        .to_string();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let mut watcher = notify::RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        notify::Config::default(),
    )?;

    // Watch parent directory to handle atomic saves correctly
    watcher.watch(parent, RecursiveMode::NonRecursive)?;
    tracing::info!(
        "Xavier Settings Watcher: Monitoring directory {:?} for changes to {}",
        parent,
        config_file_name
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
                        p.file_name().and_then(|n| n.to_str()) == Some(&config_file_name)
                    });

                    if matches_path {
                        tracing::info!("xavier.config.json changed. Reloading settings...");
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
    #[allow(dead_code)]
    pub fn resolve_config_path() -> PathBuf {
        serialization::resolve_config_path()
    }

    pub fn resolve_data_dir() -> PathBuf {
        serialization::resolve_data_dir()
    }

    pub fn load() -> Result<Option<Self>> {
        serialization::load()
    }

    pub fn apply_to_env(&self) {
        env::apply_to_env_impl(self);
    }

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

    pub fn reload() -> Result<()> {
        let settings = serialization::current();
        settings.apply_to_env();
        let mut lock = GLOBAL_SETTINGS.write();
        *lock = settings;
        Ok(())
    }

    pub async fn save(&self) -> Result<()> {
        serialization::save(self).await
    }

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

    #[test]
    fn test_default_settings() {
        let settings = XavierSettings::default();
        assert_eq!(settings.server.port, 8006);
        assert_eq!(settings.server.host, "0.0.0.0");
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

    #[test]
    fn test_load_config_json() {
        let _guard = ENV_LOCK.lock().expect("test assertion");

        // Ensure we load from the actual config/xavier.config.json
        std::env::remove_var("XAVIER_CONFIG_PATH");

        let settings = XavierSettings::load().expect("Should parse config/xavier.config.json");
        assert!(
            settings.is_some(),
            "config/xavier.config.json should exist in the environment"
        );

        let s = settings.unwrap();
        // Check local-first defaults from Step 1
        assert_eq!(s.workspace.embedding_provider_mode, "local");
        assert_eq!(s.models.embedding_model, "embeddinggemma");
        assert_eq!(s.models.router_fast_model, "");
        assert_eq!(s.models.router_quality_model, "");
        assert_eq!(
            s.embedding.endpoint,
            "http://localhost:11434/api/embeddings"
        );
        assert_eq!(s.embedding.embedder, "local");
        assert_eq!(s.embedding.gllm_model, "embeddinggemma");

        // Assertions for provider=local and local_llm_* fields
        assert_eq!(s.models.provider, "local");
        assert_eq!(s.models.local_llm_model, "qwen3-coder");
        assert_eq!(s.models.local_llm_url, "http://localhost:11434/v1");
    }

    #[test]
    fn test_apply_to_env_sets_vars() {
        let _guard = ENV_LOCK.lock().expect("test assertion");

        // Clean env
        for (key, _) in std::env::vars() {
            if key.starts_with("XAVIER_") || key == "STRIPE_SECRET_KEY" {
                std::env::remove_var(&key);
            }
        }

        let settings = XavierSettings::default();
        settings.apply_to_env();

        assert_eq!(std::env::var("XAVIER_HOST").unwrap(), "0.0.0.0");
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

        // Clean up
        for (key, _) in std::env::vars() {
            if key.starts_with("XAVIER_") {
                std::env::remove_var(&key);
            }
        }
    }

    #[test]
    fn test_apply_to_env_respects_existing_vars() {
        let _guard = ENV_LOCK.lock().expect("test assertion");

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
        assert_eq!(std::env::var("XAVIER_HOST").unwrap(), "0.0.0.0");

        // Clean up
        std::env::remove_var("XAVIER_HOST");
        std::env::remove_var("XAVIER_PORT");
        std::env::remove_var("XAVIER_RRF_K");
    }

    #[test]
    fn test_config_file_missing_returns_none() {
        let path = std::env::temp_dir().join("nonexistent_xavier_config.json");
        // Ensure it doesn't exist
        let _ = std::fs::remove_file(&path);

        let _old_path = XavierSettings::resolve_config_path();

        // Temporarily redirect config path
        std::env::set_var("XAVIER_CONFIG_PATH", path.to_str().unwrap());
        let result = XavierSettings::load().unwrap();
        assert!(result.is_none());
        std::env::remove_var("XAVIER_CONFIG_PATH");
    }

    #[test]
    fn test_current_falls_back_to_defaults() {
        let _guard = ENV_LOCK.lock().expect("test assertion");

        // Remove XAVIER_TOKEN if present
        std::env::remove_var("XAVIER_TOKEN");

        // Temporarily point config to a nonexistent path so defaults are used
        std::env::set_var("XAVIER_CONFIG_PATH", "/tmp/nonexistent-xavier-config.json");
        let settings = XavierSettings::current();
        // Without a real config file, host falls to default (0.0.0.0)
        assert_eq!(settings.server.host, "0.0.0.0");
        assert_eq!(settings.server.port, 8006);
        assert!(settings.auth_token.is_none());
        std::env::remove_var("XAVIER_CONFIG_PATH");
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
        let _guard = ENV_LOCK.lock().expect("test assertion");

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

        // Clean up
        for (key, _) in std::env::vars() {
            if key.starts_with("XAVIER_") {
                std::env::remove_var(&key);
            }
        }
    }

    #[test]
    fn test_reload_updates_global_settings() {
        let _guard = ENV_LOCK.lock().expect("test assertion");

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
        std::env::remove_var("XAVIER_CONFIG_PATH");
        let _ = std::fs::remove_file(&test_config_path);
    }
}
