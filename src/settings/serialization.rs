//! Logical concern: Serialization and path resolution for Xavier settings.
//!
//! This module handles loading configurations from files and resolving system paths.

use super::types::XavierSettings;
use crate::secrets::vault::HardwareVault;
use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::fs;

const DEFAULT_CONFIG_PATH: &str = "config/xavier.config.json";

/// Resolve config path.
pub fn resolve_config_path() -> PathBuf {
    if let Ok(env_path) = std::env::var("XAVIER_CONFIG_PATH") {
        return PathBuf::from(env_path);
    }

    if let Some(config_dir) = dirs::config_dir() {
        // Priority 1: ~/.config/xavier/xavier.toml (or OS equivalent)
        let xavier_toml = config_dir.join("xavier").join("xavier.toml");
        if xavier_toml.exists() {
            return xavier_toml;
        }
        // Priority 2: ~/.config/xavier/xavier.config.json
        let xavier_json = config_dir.join("xavier").join("xavier.config.json");
        if xavier_json.exists() {
            return xavier_json;
        }
    }

    // Fallback to local project config
    PathBuf::from(DEFAULT_CONFIG_PATH)
}

/// Resolve data dir.
pub fn resolve_data_dir() -> PathBuf {
    if let Ok(env_path) = std::env::var("XAVIER_DATA_DIR") {
        return PathBuf::from(env_path);
    }

    if let Some(data_dir) = dirs::data_dir() {
        return data_dir.join("xavier");
    }

    PathBuf::from("data")
}

/// Load.
pub fn load() -> Result<Option<XavierSettings>> {
    let path = resolve_config_path();

    if !path.exists() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read config file at {}", path.display()))?;

    // Handle both JSON and YAML/TOML if we want, but for now stick to what we have
    let parsed = if path.extension().is_some_and(|ext| ext == "toml") {
        // We need a TOML parser if we want to support .toml
        // But for now, let's assume JSON as per current implementation
        serde_json::from_str::<XavierSettings>(&raw).with_context(|| {
            format!(
                "failed to parse TOML config (as JSON) at {}",
                path.display()
            )
        })?
    } else {
        serde_json::from_str::<XavierSettings>(&raw)
            .with_context(|| format!("failed to parse config file at {}", path.display()))?
    };

    Ok(Some(parsed))
}

/// Current.
pub fn current() -> XavierSettings {
    let mut settings = load().ok().flatten().unwrap_or_default();
    settings.auth_token = std::env::var("XAVIER_TOKEN")
        .ok()
        .or_else(|| std::env::var("XAVIER_AUTH_TOKEN").ok());
    // Populate sensitive fields from env if not set via config file
    if settings.security.token_secret.is_none() {
        settings.security.token_secret = std::env::var("XAVIER_TOKEN_SECRET").ok();
    }
    if settings.telegram.bot_token.is_none() {
        settings.telegram.bot_token = std::env::var("XAVIER_TELEGRAM_TOKEN").ok();
    }
    if settings.telegram.bot_token.is_none() {
        let vault = HardwareVault::new("xavier-telegram");
        settings.telegram.bot_token = vault.get_secret("bot_token").ok();
    }
    if settings.telegram.notification_chat_id.is_none() {
        settings.telegram.notification_chat_id =
            std::env::var("XAVIER_TELEGRAM_NOTIFICATION_CHAT_ID").ok();
    }
    if settings.telegram.notification_chat_id.is_none() {
        let vault = HardwareVault::new("xavier-telegram");
        settings.telegram.notification_chat_id = vault.get_secret("notification_chat_id").ok();
    }
    if settings.pgheart.url.is_none() {
        settings.pgheart.url = std::env::var("PGHEART_URL").ok();
    }
    if settings.pgheart.token.is_none() {
        settings.pgheart.token = std::env::var("PGHEART_TOKEN").ok();
    }
    if settings.pgheart.instance_id.is_none() {
        settings.pgheart.instance_id = std::env::var("PGHEART_INSTANCE_ID").ok();
    }
    if settings.models.llm_api_key.is_none() {
        settings.models.llm_api_key = std::env::var("XAVIER_LLM_API_KEY").ok();
    }
    if settings.models.local_llm_api_key.is_none() {
        settings.models.local_llm_api_key = std::env::var("XAVIER_LOCAL_LLM_API_KEY").ok();
    }
    if let Ok(data_dir) = std::env::var("XAVIER_DATA_DIR") {
        settings.memory.data_dir = data_dir;
    }
    if let Ok(workspace_dir) = std::env::var("XAVIER_WORKSPACE_DIR") {
        settings.memory.workspace_dir = workspace_dir;
    }
    if let Ok(file_path) = std::env::var("XAVIER_MEMORY_FILE_PATH") {
        settings.memory.file_path = file_path;
    }
    if let Ok(sqlite_path) = std::env::var("XAVIER_MEMORY_SQLITE_PATH") {
        settings.memory.sqlite_path = sqlite_path;
    }
    if let Ok(vec_path) = std::env::var("XAVIER_MEMORY_VEC_PATH") {
        settings.memory.vec_path = vec_path;
    }
    if let Ok(dimensions) = std::env::var("XAVIER_EMBEDDING_DIMENSIONS") {
        if let Ok(v) = dimensions.parse() {
            settings.memory.embedding_dimensions = v;
        }
    }
    if let Ok(embedding_url) = std::env::var("XAVIER_EMBEDDING_URL") {
        settings.models.embedding_url = embedding_url;
    }
    if settings.embedding.api_key.is_none() {
        settings.embedding.api_key = std::env::var("XAVIER_EMBEDDING_API_KEY")
            .ok()
            .or_else(|| std::env::var("XAVIER_OPENROUTER_API_KEY").ok());
    }
    // Retrieval fallbacks
    if settings.retrieval.rrf_k.is_none() {
        settings.retrieval.rrf_k = std::env::var("XAVIER_RRF_K")
            .ok()
            .and_then(|v| v.parse().ok());
    }
    // Sync fallbacks
    if let Ok(val) = std::env::var("XAVIER_SYNC_INTERVAL_MS") {
        if let Ok(v) = val.parse() {
            settings.sync.interval_ms = v;
        }
    }
    if let Ok(val) = std::env::var("XAVIER_SYNC_LAG_THRESHOLD_MS") {
        if let Ok(v) = val.parse() {
            settings.sync.lag_threshold_ms = v;
        }
    }
    if let Ok(val) = std::env::var("XAVIER_SYNC_SAVE_OK_RATE_THRESHOLD") {
        if let Ok(v) = val.parse() {
            settings.sync.save_ok_rate_threshold = v;
        }
    }
    if let Ok(val) = std::env::var("XAVIER_SYNC_MAX_RETRIES") {
        if let Ok(v) = val.parse() {
            settings.sync.max_retries = v;
        }
    }
    if let Ok(val) = std::env::var("XAVIER_SYNC_MIN_HEALTH_INTERVAL_MS") {
        if let Ok(v) = val.parse() {
            settings.sync.min_health_interval_ms = v;
        }
    }
    if let Ok(val) = std::env::var("XAVIER_SYNC_TIMEOUT_MS") {
        if let Ok(v) = val.parse() {
            settings.sync.timeout_ms = v;
        }
    }
    if settings.retrieval.zone_boost_multiplier.is_none() {
        settings.retrieval.zone_boost_multiplier = std::env::var("XAVIER_ZONE_BOOST")
            .ok()
            .and_then(|v| v.parse().ok());
    }
    if settings.retrieval.zone_penalty_multiplier.is_none() {
        settings.retrieval.zone_penalty_multiplier = std::env::var("XAVIER_ZONE_PENALTY")
            .ok()
            .and_then(|v| v.parse().ok());
    }
    settings
}

/// Save.
pub async fn save(settings: &XavierSettings) -> Result<()> {
    let path = resolve_config_path();
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).await.with_context(|| {
                format!("failed to create config directory at {}", parent.display())
            })?;
        }
    }

    let raw = serde_json::to_string_pretty(settings)
        .with_context(|| "failed to serialize settings to JSON")?;

    fs::write(&path, raw).await.with_context(|| {
        format!(
            "failed to write settings to config file at {}",
            path.display()
        )
    })?;
    Ok(())
}
