//! Workspace configuration management
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use super::protocol::SpaceProtocol;
use crate::memory::store::MemoryBackend;
use crate::settings::{types::DedupSettings, XavierSettings};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const MB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanTier {
    Community,
    Free,
    Personal,
    Pro,
}

impl PlanTier {
    /// From env.
    pub fn from_env(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "free" => Self::Free,
            "personal" => Self::Personal,
            "pro" => Self::Pro,
            _ => Self::Community,
        }
    }

    /// Default storage limit bytes.
    pub fn default_storage_limit_bytes(self) -> Option<u64> {
        match self {
            Self::Community => None,
            Self::Free => Some(100 * MB),
            Self::Personal => Some(500 * MB),
            Self::Pro => Some(2 * 1024 * MB),
        }
    }

    /// Default request limit.
    pub fn default_request_limit(self) -> Option<usize> {
        match self {
            Self::Community => None,
            Self::Free => Some(5_000),
            Self::Personal => Some(50_000),
            Self::Pro => Some(250_000),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingProviderMode {
    BringYourOwn,
    Managed,
}

impl EmbeddingProviderMode {
    /// From env.
    pub fn from_env(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "managed" => Self::Managed,
            _ => Self::BringYourOwn,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncPolicy {
    LocalOnly,
    CloudMirror,
    MetadataOnly,
    CloudHotCache,
    GitChunk,
}

impl SyncPolicy {
    /// From env.
    pub fn from_env(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "cloud_mirror" => Self::CloudMirror,
            "metadata_only" => Self::MetadataOnly,
            "cloud_hot_cache" => Self::CloudHotCache,
            "git_chunk" => Self::GitChunk,
            _ => Self::LocalOnly,
        }
    }

    /// Supported.
    pub fn supported() -> &'static [SyncPolicy] {
        &[
            SyncPolicy::LocalOnly,
            SyncPolicy::CloudMirror,
            SyncPolicy::MetadataOnly,
            SyncPolicy::CloudHotCache,
            SyncPolicy::GitChunk,
        ]
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub id: String,
    pub token: String,
    pub plan: PlanTier,
    pub memory_backend: MemoryBackend,
    pub storage_limit_bytes: Option<u64>,
    pub request_limit: Option<usize>,
    pub request_unit_limit: Option<u64>,
    pub embedding_provider_mode: EmbeddingProviderMode,
    pub managed_google_embeddings: bool,
    pub sync_policy: SyncPolicy,
    pub dedup: DedupSettings,
    #[serde(default)]
    pub protocol: SpaceProtocol,
}

impl fmt::Debug for WorkspaceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkspaceConfig")
            .field("id", &self.id)
            .field("token", &"[REDACTED]")
            .field("plan", &self.plan)
            .field("memory_backend", &self.memory_backend)
            .field("storage_limit_bytes", &self.storage_limit_bytes)
            .field("request_limit", &self.request_limit)
            .field("request_unit_limit", &self.request_unit_limit)
            .field("embedding_provider_mode", &self.embedding_provider_mode)
            .field("managed_google_embeddings", &self.managed_google_embeddings)
            .field("sync_policy", &self.sync_policy)
            .field("dedup", &self.dedup)
            .field("protocol", &self.protocol)
            .finish()
    }
}

impl WorkspaceConfig {
    /// From env.
    pub fn from_env() -> Self {
        let settings = XavierSettings::current();
        let plan = PlanTier::from_env(&settings.workspace.default_plan);

        let storage_limit_bytes = settings
            .workspace
            .storage_limit_bytes
            .or_else(|| plan.default_storage_limit_bytes());

        let request_limit = settings
            .workspace
            .request_limit
            .or_else(|| plan.default_request_limit());
        let request_unit_limit = settings
            .workspace
            .request_unit_limit
            .or_else(|| request_limit.map(|value| value as u64 * 2));

        Self {
            id: settings.workspace.default_workspace_id.clone(),
            token: settings
                .auth_token
                .clone()
                .or_else(|| std::env::var("XAVIER_TOKEN").ok())
                .or_else(|| std::env::var("XAVIER_AUTH_TOKEN").ok())
                .expect("XAVIER_TOKEN must be set"),
            plan,
            memory_backend: MemoryBackend::from_env(&settings.memory.backend),
            storage_limit_bytes,
            request_limit,
            request_unit_limit,
            embedding_provider_mode: EmbeddingProviderMode::from_env(
                &settings.workspace.embedding_provider_mode,
            ),
            managed_google_embeddings: settings.workspace.managed_google_embeddings,
            sync_policy: SyncPolicy::from_env(&settings.workspace.sync_policy),
            dedup: settings.memory.dedup.clone(),
            protocol: SpaceProtocol::default(),
        }
    }
}

#[cfg(test)]
mod protocol_compat_tests {
    use super::*;

    fn sample() -> WorkspaceConfig {
        WorkspaceConfig {
            id: "ws".into(),
            token: "t".into(),
            plan: PlanTier::Community,
            memory_backend: MemoryBackend::File,
            storage_limit_bytes: None,
            request_limit: None,
            request_unit_limit: None,
            embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: SyncPolicy::LocalOnly,
            dedup: crate::settings::types::DedupSettings::default(),
            protocol: SpaceProtocol::default(),
        }
    }

    #[test]
    fn config_without_protocol_still_parses() {
        let mut v = serde_json::to_value(sample()).unwrap();
        v.as_object_mut().unwrap().remove("protocol");
        let cfg: WorkspaceConfig = serde_json::from_value(v).unwrap();
        assert!(cfg.protocol.validate().is_ok());
        assert_eq!(cfg.id, "ws");
    }

    #[test]
    fn config_with_invalid_protocol_field_is_rejected() {
        let mut v = serde_json::to_value(sample()).unwrap();
        v["protocol"]["bogus"] = serde_json::json!(true);
        assert!(serde_json::from_value::<WorkspaceConfig>(v).is_err());
    }
}
