//! Per-space protocol configuration (`SpaceProtocol`).
//!
//! Declarative, validated description of how a space stores, syncs and
//! transports data. Reuses the existing `MemoryBackend`, `SyncPolicy`,
//! `PlanTier` and `EmbeddingProviderMode` types. Nothing consumes it yet.
use super::config::{EmbeddingProviderMode, PlanTier, SyncPolicy};
use crate::memory::store::MemoryBackend;
use crate::settings::types::DedupSettings;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    LocalOnly,
    Https,
    Iroh,
    /// Future value only; always rejected by `validate`.
    Onion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageProtocol {
    pub backend: MemoryBackend,
    pub encrypt_records: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncProtocol {
    pub policy: SyncPolicy,
    pub mirror_target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportProtocol {
    pub kind: TransportKind,
    #[serde(default)]
    pub chain: Vec<TransportKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitsProtocol {
    pub plan: PlanTier,
    pub storage_limit_bytes: Option<u64>,
    pub request_limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceProtocol {
    pub version: u32,
    pub storage: StorageProtocol,
    pub sync: SyncProtocol,
    pub transport: TransportProtocol,
    pub limits: LimitsProtocol,
    pub dedup: Option<DedupSettings>,
    pub embedding_mode: EmbeddingProviderMode,
    pub members_cap: u32,
    pub invite_default_ttl_hours: u32,
}

impl Default for SpaceProtocol {
    /// Local-only safe configuration.
    fn default() -> Self {
        Self {
            version: PROTOCOL_VERSION,
            storage: StorageProtocol {
                backend: MemoryBackend::Vec,
                encrypt_records: true,
            },
            sync: SyncProtocol {
                policy: SyncPolicy::LocalOnly,
                mirror_target: None,
            },
            transport: TransportProtocol {
                kind: TransportKind::LocalOnly,
                chain: Vec::new(),
            },
            limits: LimitsProtocol {
                plan: PlanTier::Community,
                storage_limit_bytes: None,
                request_limit: None,
            },
            dedup: None,
            embedding_mode: EmbeddingProviderMode::BringYourOwn,
            members_cap: 100,
            invite_default_ttl_hours: 24,
        }
    }
}

fn check_transport(kind: TransportKind, label: &str, errors: &mut Vec<String>) {
    match kind {
        TransportKind::Onion => errors.push(format!(
            "{label}: onion transport is not supported (future)"
        )),
        TransportKind::Iroh if !cfg!(feature = "mesh") => errors.push(format!(
            "{label}: iroh transport requires the `mesh` feature"
        )),
        _ => {}
    }
}

impl SpaceProtocol {
    /// Pure validation; returns every violated rule.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.version != PROTOCOL_VERSION {
            errors.push(format!(
                "version {} is not supported (expected {PROTOCOL_VERSION})",
                self.version
            ));
        }

        if !matches!(
            self.storage.backend,
            MemoryBackend::File | MemoryBackend::Sqlite | MemoryBackend::Vec
        ) {
            errors.push(format!(
                "storage.backend '{}' is not allowed (use file, sqlite or vec)",
                self.storage.backend.as_str()
            ));
        }

        if self.sync.policy != SyncPolicy::LocalOnly && !self.storage.encrypt_records {
            errors.push("sync.policy other than local_only requires encrypt_records=true".into());
        }
        let needs_mirror = matches!(
            self.sync.policy,
            SyncPolicy::CloudMirror | SyncPolicy::CloudHotCache | SyncPolicy::GitChunk
        );
        if needs_mirror
            && self
                .sync
                .mirror_target
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
        {
            errors.push("sync.mirror_target is required for this sync.policy".into());
        }

        check_transport(self.transport.kind, "transport.kind", &mut errors);
        for kind in &self.transport.chain {
            check_transport(*kind, "transport.chain", &mut errors);
        }

        if self.limits.storage_limit_bytes == Some(0) {
            errors.push("limits.storage_limit_bytes must be positive".into());
        }
        if self.limits.request_limit == Some(0) {
            errors.push("limits.request_limit must be positive".into());
        }
        if self.members_cap == 0 {
            errors.push("members_cap must be positive".into());
        }
        if self.invite_default_ttl_hours == 0 {
            errors.push("invite_default_ttl_hours must be positive".into());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Reject post-creation changes to immutable fields (v1: `storage.backend`).
pub fn immutable_change(old: &SpaceProtocol, new: &SpaceProtocol) -> Result<(), String> {
    if old.storage.backend != new.storage.backend {
        return Err(format!(
            "storage.backend is immutable after creation ('{}' -> '{}')",
            old.storage.backend.as_str(),
            new.storage.backend.as_str()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(p: &SpaceProtocol, needle: &str) -> bool {
        match p.validate() {
            Err(e) => e.iter().any(|m| m.contains(needle)),
            Ok(()) => false,
        }
    }

    #[test]
    fn default_is_valid() {
        assert!(SpaceProtocol::default().validate().is_ok());
    }

    #[test]
    fn default_encrypts_records_and_is_local_only() {
        let p = SpaceProtocol::default();
        assert!(p.storage.encrypt_records);
        assert_eq!(p.sync.policy, SyncPolicy::LocalOnly);
    }

    #[test]
    fn allowed_backends_valid() {
        for b in [
            MemoryBackend::File,
            MemoryBackend::Sqlite,
            MemoryBackend::Vec,
        ] {
            let mut p = SpaceProtocol::default();
            p.storage.backend = b;
            assert!(p.validate().is_ok(), "{b:?}");
        }
    }

    #[test]
    fn shared_credential_backends_rejected() {
        for b in [
            MemoryBackend::Supabase,
            MemoryBackend::Postgres,
            MemoryBackend::Auto,
            MemoryBackend::Memory,
            MemoryBackend::Fallback,
        ] {
            let mut p = SpaceProtocol::default();
            p.storage.backend = b;
            assert!(has(&p, "storage.backend"), "{b:?}");
        }
    }

    #[test]
    fn non_local_sync_requires_encryption() {
        let mut p = SpaceProtocol::default();
        p.sync.policy = SyncPolicy::MetadataOnly;
        p.storage.encrypt_records = false;
        assert!(has(&p, "encrypt_records"));
        p.storage.encrypt_records = true;
        assert!(p.validate().is_ok());
    }

    #[test]
    fn local_only_does_not_require_encryption() {
        let mut p = SpaceProtocol::default();
        p.storage.encrypt_records = false;
        assert!(p.validate().is_ok());
    }

    #[test]
    fn mirror_target_required_where_needed() {
        for policy in [
            SyncPolicy::CloudMirror,
            SyncPolicy::CloudHotCache,
            SyncPolicy::GitChunk,
        ] {
            let mut p = SpaceProtocol::default();
            p.sync.policy = policy;
            assert!(has(&p, "mirror_target"), "{policy:?} missing");
            p.sync.mirror_target = Some("  ".into());
            assert!(has(&p, "mirror_target"), "{policy:?} blank");
            p.sync.mirror_target = Some("https://mirror.example".into());
            assert!(p.validate().is_ok(), "{policy:?} ok");
        }
    }

    #[test]
    fn onion_rejected() {
        let mut p = SpaceProtocol::default();
        p.transport.kind = TransportKind::Onion;
        assert!(has(&p, "onion"));
        let mut p = SpaceProtocol::default();
        p.transport.chain = vec![TransportKind::Https, TransportKind::Onion];
        assert!(has(&p, "onion"));
    }

    #[test]
    fn iroh_depends_on_mesh_feature() {
        let mut p = SpaceProtocol::default();
        p.transport.kind = TransportKind::Iroh;
        p.transport.chain = vec![TransportKind::Iroh];
        if cfg!(feature = "mesh") {
            assert!(p.validate().is_ok());
        } else {
            assert!(has(&p, "iroh"));
        }
    }

    #[test]
    fn https_transport_valid() {
        let mut p = SpaceProtocol::default();
        p.transport.kind = TransportKind::Https;
        p.transport.chain = vec![TransportKind::Https];
        assert!(p.validate().is_ok());
    }

    #[test]
    fn limits_must_be_positive() {
        let mut p = SpaceProtocol::default();
        p.limits.storage_limit_bytes = Some(0);
        assert!(has(&p, "storage_limit_bytes"));
        let mut p = SpaceProtocol::default();
        p.limits.request_limit = Some(0);
        assert!(has(&p, "request_limit"));
        let p = SpaceProtocol {
            members_cap: 0,
            ..Default::default()
        };
        assert!(has(&p, "members_cap"));
        let p = SpaceProtocol {
            invite_default_ttl_hours: 0,
            ..Default::default()
        };
        assert!(has(&p, "invite_default_ttl_hours"));
        let mut p = SpaceProtocol::default();
        p.limits.storage_limit_bytes = Some(1);
        p.limits.request_limit = Some(1);
        assert!(p.validate().is_ok());
    }

    #[test]
    fn version_must_match() {
        let p = SpaceProtocol {
            version: 2,
            ..Default::default()
        };
        assert!(has(&p, "version"));
    }

    #[test]
    fn all_violations_are_reported() {
        let mut p = SpaceProtocol::default();
        p.storage.backend = MemoryBackend::Postgres;
        p.members_cap = 0;
        p.transport.kind = TransportKind::Onion;
        assert!(p.validate().unwrap_err().len() >= 3);
    }

    #[test]
    fn unknown_fields_rejected() {
        let mut v = serde_json::to_value(SpaceProtocol::default()).unwrap();
        v["bogus"] = serde_json::json!(1);
        assert!(serde_json::from_value::<SpaceProtocol>(v).is_err());
        let mut v = serde_json::to_value(SpaceProtocol::default()).unwrap();
        v["storage"]["bogus"] = serde_json::json!(1);
        assert!(serde_json::from_value::<SpaceProtocol>(v).is_err());
    }

    #[test]
    fn default_roundtrips_through_json() {
        let json = serde_json::to_string(&SpaceProtocol::default()).unwrap();
        let back: SpaceProtocol = serde_json::from_str(&json).unwrap();
        assert!(back.validate().is_ok());
        assert_eq!(back.storage.backend, MemoryBackend::Vec);
    }

    #[test]
    fn backend_change_rejected() {
        let old = SpaceProtocol::default();
        let mut new = SpaceProtocol::default();
        assert!(immutable_change(&old, &new).is_ok());
        new.storage.backend = MemoryBackend::Sqlite;
        assert!(immutable_change(&old, &new).is_err());
    }
}
