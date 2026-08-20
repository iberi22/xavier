//! Maloca Data Node Opt-In Consent and Storage Quota Allocation Management.
//!
//! Provides core data node consent configuration, persistence, state transitions,
//! and Axum HTTP handlers for `/v1/maloca/node/consent`.

use axum::{
    extract::{Json, State},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Configuration for user opt-in consent and local storage quota allocation in the Maloca network.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataNodeConfig {
    /// Flag indicating whether the node has opted into participating in the Maloca data network.
    pub opt_in: bool,
    /// Storage quota allocated for P2P data sharing, specified in megabytes (MB).
    pub storage_quota_mb: u32,
}

impl Default for DataNodeConfig {
    fn default() -> Self {
        Self {
            opt_in: false,
            storage_quota_mb: 1024, // Default 1024 MB (1 GB)
        }
    }
}

impl DataNodeConfig {
    /// Constructs a new `DataNodeConfig` instance.
    pub fn new(opt_in: bool, storage_quota_mb: u32) -> Self {
        Self {
            opt_in,
            storage_quota_mb,
        }
    }

    /// Calculates the allocated storage quota in bytes.
    pub fn quota_bytes(&self) -> u64 {
        (self.storage_quota_mb as u64) * 1024 * 1024
    }
}

/// Request payload for updating data node consent or storage quota allocation.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ConsentUpdateRequest {
    /// Optional updated opt-in status.
    pub opt_in: Option<bool>,
    /// Optional updated storage quota allocation in MB.
    pub storage_quota_mb: Option<u32>,
}

/// Response format for data node consent and storage quota status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataNodeConsentResponse {
    /// Current opt-in consent flag.
    pub opt_in: bool,
    /// Allocated storage quota in megabytes (MB).
    pub storage_quota_mb: u32,
    /// Allocated storage quota converted to bytes.
    pub storage_quota_bytes: u64,
    /// Human-readable status string ("opted_in" or "opted_out").
    pub status: String,
}

impl From<&DataNodeConfig> for DataNodeConsentResponse {
    fn from(config: &DataNodeConfig) -> Self {
        let status = if config.opt_in {
            "opted_in".to_string()
        } else {
            "opted_out".to_string()
        };
        Self {
            opt_in: config.opt_in,
            storage_quota_mb: config.storage_quota_mb,
            storage_quota_bytes: config.quota_bytes(),
            status,
        }
    }
}

/// Thread-safe manager for Data Node consent settings and local storage allocation.
#[derive(Clone, Debug)]
pub struct DataNodeManager {
    config: Arc<RwLock<DataNodeConfig>>,
    file_path: Option<PathBuf>,
}

impl Default for DataNodeManager {
    fn default() -> Self {
        Self::new(DataNodeConfig::default())
    }
}

impl DataNodeManager {
    /// Creates a new `DataNodeManager` with the specified initial configuration.
    pub fn new(config: DataNodeConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            file_path: None,
        }
    }

    /// Attaches a file path for persistence to the manager.
    pub fn with_file_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.file_path = Some(path.into());
        self
    }

    /// Attempts to load consent configuration from a JSON file path, falling back to default if missing or unparseable.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, String> {
        let path_ref = path.as_ref();
        let config = if path_ref.exists() {
            let content = fs::read_to_string(path_ref)
                .map_err(|e| format!("Failed to read consent config file: {}", e))?;
            serde_json::from_str::<DataNodeConfig>(&content)
                .map_err(|e| format!("Failed to deserialize consent config: {}", e))?
        } else {
            DataNodeConfig::default()
        };

        Ok(Self::new(config).with_file_path(path_ref.to_path_buf()))
    }

    /// Saves current configuration to the attached file path if set.
    pub fn save_to_file(&self) -> Result<(), String> {
        if let Some(ref path) = self.file_path {
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed to create parent directory: {}", e))?;
                }
            }
            let config = self.get_config();
            let json = serde_json::to_string_pretty(&config)
                .map_err(|e| format!("Failed to serialize consent config: {}", e))?;
            fs::write(path, json)
                .map_err(|e| format!("Failed to write consent config to file: {}", e))?;
        }
        Ok(())
    }

    /// Returns a copy of the current `DataNodeConfig`.
    pub fn get_config(&self) -> DataNodeConfig {
        self.config
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Returns a structured `DataNodeConsentResponse`.
    pub fn get_consent_response(&self) -> DataNodeConsentResponse {
        let cfg = self.get_config();
        DataNodeConsentResponse::from(&cfg)
    }

    /// Returns whether the user has opted in.
    pub fn is_opted_in(&self) -> bool {
        self.config
            .read()
            .map(|guard| guard.opt_in)
            .unwrap_or(false)
    }

    /// Returns the storage quota in MB.
    pub fn storage_quota_mb(&self) -> u32 {
        self.config
            .read()
            .map(|guard| guard.storage_quota_mb)
            .unwrap_or(0)
    }

    /// Returns the storage quota in bytes.
    pub fn storage_quota_bytes(&self) -> u64 {
        self.get_config().quota_bytes()
    }

    /// Updates opt-in consent and/or storage quota allocation, saving to file if configured.
    pub fn update_consent(
        &self,
        opt_in: Option<bool>,
        storage_quota_mb: Option<u32>,
    ) -> DataNodeConsentResponse {
        {
            if let Ok(mut guard) = self.config.write() {
                if let Some(val) = opt_in {
                    guard.opt_in = val;
                }
                if let Some(quota) = storage_quota_mb {
                    guard.storage_quota_mb = quota;
                }
            }
        }

        let _ = self.save_to_file();
        self.get_consent_response()
    }
}

/// GET `/v1/maloca/node/consent`: Retrieves current Data Node consent and storage allocation status.
pub async fn get_consent_handler(State(manager): State<DataNodeManager>) -> impl IntoResponse {
    Json(manager.get_consent_response())
}

/// POST `/v1/maloca/node/consent`: Updates Data Node consent and storage quota allocation settings.
pub async fn update_consent_handler(
    State(manager): State<DataNodeManager>,
    Json(payload): Json<ConsentUpdateRequest>,
) -> impl IntoResponse {
    let response = manager.update_consent(payload.opt_in, payload.storage_quota_mb);
    Json(response)
}

/// Constructs the Axum router for Data Node consent management.
pub fn router(manager: DataNodeManager) -> Router {
    Router::new()
        .route(
            "/v1/maloca/node/consent",
            get(get_consent_handler).post(update_consent_handler),
        )
        .with_state(manager)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_node_config_default() {
        let config = DataNodeConfig::default();
        assert!(!config.opt_in);
        assert_eq!(config.storage_quota_mb, 1024);
        assert_eq!(config.quota_bytes(), 1024 * 1024 * 1024);
    }

    #[test]
    fn test_data_node_manager_updates() {
        let manager = DataNodeManager::default();
        assert!(!manager.is_opted_in());
        assert_eq!(manager.storage_quota_mb(), 1024);

        let res = manager.update_consent(Some(true), Some(2048));
        assert!(res.opt_in);
        assert_eq!(res.storage_quota_mb, 2048);
        assert_eq!(res.storage_quota_bytes, 2048 * 1024 * 1024);
        assert_eq!(res.status, "opted_in");

        assert!(manager.is_opted_in());
        assert_eq!(manager.storage_quota_mb(), 2048);
    }
}
