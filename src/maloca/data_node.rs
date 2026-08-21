//! Data Node Opt-In Consent Registry for P2P mesh participation governance.
//!
//! Every node that participates in the P2P mesh must explicitly opt-in to data
//! sharing. The consent registry tracks per-node consent records persisted as
//! JSON on disk, providing the governance foundation for the mesh layer.
//!
//! Endpoints:
//!   POST   /maloca/consent          — register or update consent for a node
//!   GET    /maloca/consent/{node_id} — query consent status for a node
//!   DELETE /maloca/consent/{node_id} — revoke consent for a node

use anyhow::{bail, Result};
use chrono::Utc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

/// The scope of data sharing the node has consented to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConsentScope {
    /// Full mesh participation — all data types shared.
    Full,
    /// Read-only participation — node can receive but not send data.
    ReadOnly,
    /// Metadata only — no user or content data shared.
    MetadataOnly,
    /// Custom scope string for future extensibility.
    Custom(String),
}

impl ConsentScope {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Full => "full",
            Self::ReadOnly => "read_only",
            Self::MetadataOnly => "metadata_only",
            Self::Custom(s) => s.as_str(),
        }
    }
}

/// A single consent record for a data node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataNodeConsent {
    /// Unique identifier of the consenting node.
    pub node_id: String,
    /// Whether the node has actively consented to mesh participation.
    pub consented: bool,
    /// ISO-8601 timestamp of when consent was last updated.
    pub timestamp: String,
    /// Scope of data sharing the node has opted into.
    pub scope: ConsentScope,
}

impl DataNodeConsent {
    /// Create a new consent record.
    pub fn new(node_id: impl Into<String>, consented: bool, scope: ConsentScope) -> Self {
        Self {
            node_id: node_id.into(),
            consented,
            timestamp: Utc::now().to_rfc3339(),
            scope,
        }
    }
}

/// POST body for registering or updating consent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentBody {
    pub node_id: String,
    pub consented: bool,
    #[serde(default = "default_scope")]
    pub scope: ConsentScope,
}

fn default_scope() -> ConsentScope {
    ConsentScope::ReadOnly
}

/// Internal state persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ConsentState {
    records: HashMap<String, DataNodeConsent>,
}

/// Thread-safe consent registry backed by a JSON file.
pub struct ConsentRegistry {
    inner: RwLock<ConsentState>,
    path: PathBuf,
}

impl ConsentRegistry {
    /// Open (or create) the consent registry at `<state_dir>/maloca/consent.json`.
    pub fn open(state_dir: &Path) -> Arc<Self> {
        let path = state_dir.join("maloca").join("consent.json");
        let state = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or_default()
        } else {
            ConsentState::default()
        };
        Arc::new(Self {
            inner: RwLock::new(state),
            path,
        })
    }

    /// Create a registry in the user's standard data directory.
    pub fn new_std() -> Arc<Self> {
        let data_dir = dirs::data_local_dir()
            .or_else(|| dirs::home_dir())
            .unwrap_or_else(|| PathBuf::from("."));
        Self::open(&data_dir)
    }

    /// Persist state to disk.
    fn persist(&self, state: &ConsentState) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(raw) = serde_json::to_string_pretty(state) {
            let _ = std::fs::write(&self.path, raw);
        }
    }

    /// Register or update consent for a node.
    pub fn register(&self, body: ConsentBody) -> DataNodeConsent {
        let consent = DataNodeConsent::new(&body.node_id, body.consented, body.scope);
        let mut state = self.inner.write();
        state.records.insert(body.node_id.clone(), consent.clone());
        self.persist(&state);
        consent
    }

    /// Check consent status for a specific node.
    pub fn check(&self, node_id: &str) -> Result<DataNodeConsent> {
        let state = self.inner.read();
        state
            .records
            .get(node_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("consent record not found for node {}", node_id))
    }

    /// Revoke consent for a node (sets consented=false and records the revocation).
    pub fn revoke(&self, node_id: &str) -> Result<DataNodeConsent> {
        let mut state = self.inner.write();
        let record = state
            .records
            .get_mut(node_id)
            .ok_or_else(|| anyhow::anyhow!("consent record not found for node {}", node_id))?;
        record.consented = false;
        record.timestamp = Utc::now().to_rfc3339();
        let out = record.clone();
        self.persist(&state);
        Ok(out)
    }

    /// List all consent records.
    pub fn list_all(&self) -> Vec<DataNodeConsent> {
        let state = self.inner.read();
        state.records.values().cloned().collect()
    }

    /// Count of nodes that have actively consented.
    pub fn active_consent_count(&self) -> usize {
        let state = self.inner.read();
        state.records.values().filter(|r| r.consented).count()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_registry() -> (Arc<ConsentRegistry>, TempDir) {
        let dir = TempDir::new().unwrap();
        let registry = ConsentRegistry::open(dir.path());
        (registry, dir)
    }

    #[test]
    fn test_register_consent() {
        let (reg, _dir) = make_registry();
        let body = ConsentBody {
            node_id: "node-alpha".into(),
            consented: true,
            scope: ConsentScope::Full,
        };
        let record = reg.register(body);
        assert_eq!(record.node_id, "node-alpha");
        assert!(record.consented);
        assert_eq!(record.scope, ConsentScope::Full);
    }

    #[test]
    fn test_check_consent_found() {
        let (reg, _dir) = make_registry();
        reg.register(ConsentBody {
            node_id: "node-beta".into(),
            consented: true,
            scope: ConsentScope::ReadOnly,
        });
        let record = reg.check("node-beta").unwrap();
        assert_eq!(record.node_id, "node-beta");
        assert!(record.consented);
        assert_eq!(record.scope, ConsentScope::ReadOnly);
    }

    #[test]
    fn test_check_consent_not_found() {
        let (reg, _dir) = make_registry();
        let err = reg.check("node-ghost").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_revoke_consent() {
        let (reg, _dir) = make_registry();
        reg.register(ConsentBody {
            node_id: "node-gamma".into(),
            consented: true,
            scope: ConsentScope::Full,
        });
        let revoked = reg.revoke("node-gamma").unwrap();
        assert!(!revoked.consented);

        // Verify via check
        let record = reg.check("node-gamma").unwrap();
        assert!(!record.consented);
    }

    #[test]
    fn test_revoke_consent_not_found() {
        let (reg, _dir) = make_registry();
        let err = reg.revoke("node-nobody").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_update_consent_overwrites() {
        let (reg, _dir) = make_registry();
        reg.register(ConsentBody {
            node_id: "node-delta".into(),
            consented: true,
            scope: ConsentScope::ReadOnly,
        });
        reg.register(ConsentBody {
            node_id: "node-delta".into(),
            consented: true,
            scope: ConsentScope::Full,
        });
        let record = reg.check("node-delta").unwrap();
        assert_eq!(record.scope, ConsentScope::Full);
    }

    #[test]
    fn test_list_all() {
        let (reg, _dir) = make_registry();
        reg.register(ConsentBody {
            node_id: "a".into(),
            consented: true,
            scope: ConsentScope::Full,
        });
        reg.register(ConsentBody {
            node_id: "b".into(),
            consented: false,
            scope: ConsentScope::MetadataOnly,
        });
        let all = reg.list_all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_active_consent_count() {
        let (reg, _dir) = make_registry();
        reg.register(ConsentBody {
            node_id: "x".into(),
            consented: true,
            scope: ConsentScope::Full,
        });
        reg.register(ConsentBody {
            node_id: "y".into(),
            consented: false,
            scope: ConsentScope::ReadOnly,
        });
        reg.register(ConsentBody {
            node_id: "z".into(),
            consented: true,
            scope: ConsentScope::MetadataOnly,
        });
        assert_eq!(reg.active_consent_count(), 2);
    }

    #[test]
    fn test_consent_scope_serde() {
        let full = ConsentScope::Full;
        let json = serde_json::to_string(&full).unwrap();
        assert_eq!(json, "\"full\"");
        let parsed: ConsentScope = serde_json::from_str("\"read_only\"").unwrap();
        assert_eq!(parsed, ConsentScope::ReadOnly);
    }

    #[test]
    fn test_consent_scope_custom() {
        let custom = ConsentScope::Custom("research_only".into());
        assert_eq!(custom.as_str(), "research_only");
    }

    #[test]
    fn test_persistence_survives_reload() {
        let dir = TempDir::new().unwrap();
        {
            let reg = ConsentRegistry::open(dir.path());
            reg.register(ConsentBody {
                node_id: "persistent-node".into(),
                consented: true,
                scope: ConsentScope::Full,
            });
        }
        // Reload from disk
        let reg2 = ConsentRegistry::open(dir.path());
        let record = reg2.check("persistent-node").unwrap();
        assert!(record.consented);
        assert_eq!(record.scope, ConsentScope::Full);
    }

    #[test]
    fn test_default_scope_deserialization() {
        let body: ConsentBody =
            serde_json::from_str(r#"{"node_id":"n1","consented":true}"#).unwrap();
        assert_eq!(body.scope, ConsentScope::ReadOnly);
    }

    // ---- 8 new edge-case tests ----

    #[test]
    fn test_concurrent_registration() {
        let (reg, _dir) = make_registry();
        let mut handles = vec![];
        for i in 0..16 {
            let reg_clone = Arc::clone(&reg);
            handles.push(std::thread::spawn(move || {
                let body = ConsentBody {
                    node_id: format!("thread-node-{i}"),
                    consented: true,
                    scope: ConsentScope::Full,
                };
                reg_clone.register(body);
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(reg.list_all().len(), 16);
        assert_eq!(reg.active_consent_count(), 16);
    }

    #[test]
    fn test_revoke_already_revoked_idempotent() {
        let (reg, _dir) = make_registry();
        reg.register(ConsentBody {
            node_id: "node-idem".into(),
            consented: true,
            scope: ConsentScope::Full,
        });
        // First revoke
        let r1 = reg.revoke("node-idem").unwrap();
        assert!(!r1.consented);
        // Second revoke — should succeed and be idempotent
        let r2 = reg.revoke("node-idem").unwrap();
        assert!(!r2.consented);
        // Verify via check
        let check = reg.check("node-idem").unwrap();
        assert!(!check.consented);
    }

    #[test]
    fn test_scope_upgrade_metadata_to_full() {
        let (reg, _dir) = make_registry();
        reg.register(ConsentBody {
            node_id: "node-upgrade".into(),
            consented: true,
            scope: ConsentScope::MetadataOnly,
        });
        let before = reg.check("node-upgrade").unwrap();
        assert_eq!(before.scope, ConsentScope::MetadataOnly);

        // Upgrade to Full
        reg.register(ConsentBody {
            node_id: "node-upgrade".into(),
            consented: true,
            scope: ConsentScope::Full,
        });
        let after = reg.check("node-upgrade").unwrap();
        assert_eq!(after.scope, ConsentScope::Full);
    }

    #[test]
    fn test_scope_downgrade_full_to_metadata() {
        let (reg, _dir) = make_registry();
        reg.register(ConsentBody {
            node_id: "node-downgrade".into(),
            consented: true,
            scope: ConsentScope::Full,
        });
        let before = reg.check("node-downgrade").unwrap();
        assert_eq!(before.scope, ConsentScope::Full);

        // Downgrade to MetadataOnly
        reg.register(ConsentBody {
            node_id: "node-downgrade".into(),
            consented: true,
            scope: ConsentScope::MetadataOnly,
        });
        let after = reg.check("node-downgrade").unwrap();
        assert_eq!(after.scope, ConsentScope::MetadataOnly);
    }

    #[test]
    fn test_bulk_operations_consistency() {
        let (reg, _dir) = make_registry();
        let count = 150;
        for i in 0..count {
            reg.register(ConsentBody {
                node_id: format!("bulk-{i}"),
                consented: i % 2 == 0,
                scope: if i % 3 == 0 {
                    ConsentScope::Full
                } else {
                    ConsentScope::MetadataOnly
                },
            });
        }
        let all = reg.list_all();
        assert_eq!(all.len(), count);

        // Verify active count matches even-numbered nodes
        let expected_active = count / 2;
        assert_eq!(reg.active_consent_count(), expected_active);

        // Verify individual records
        for i in 0..count {
            let record = reg.check(&format!("bulk-{i}")).unwrap();
            assert_eq!(record.consented, i % 2 == 0);
        }
    }

    #[test]
    fn test_cross_node_isolation() {
        let (reg, _dir) = make_registry();
        reg.register(ConsentBody {
            node_id: "alpha-node".into(),
            consented: true,
            scope: ConsentScope::Full,
        });
        reg.register(ConsentBody {
            node_id: "beta-node".into(),
            consented: true,
            scope: ConsentScope::ReadOnly,
        });

        // Revoke alpha — beta should be unaffected
        reg.revoke("alpha-node").unwrap();

        let alpha = reg.check("alpha-node").unwrap();
        assert!(!alpha.consented);

        let beta = reg.check("beta-node").unwrap();
        assert!(beta.consented);
        assert_eq!(beta.scope, ConsentScope::ReadOnly);
    }

    #[test]
    fn test_update_overwrites_previous_scope_and_state() {
        let (reg, _dir) = make_registry();
        // Register with ReadOnly scope, consented=true
        reg.register(ConsentBody {
            node_id: "node-ow".into(),
            consented: true,
            scope: ConsentScope::ReadOnly,
        });
        // Revoke it
        reg.revoke("node-ow").unwrap();
        let revoked = reg.check("node-ow").unwrap();
        assert!(!revoked.consented);

        // Now re-register with Full scope, consented=true — should overwrite everything
        reg.register(ConsentBody {
            node_id: "node-ow".into(),
            consented: true,
            scope: ConsentScope::Full,
        });
        let updated = reg.check("node-ow").unwrap();
        assert!(updated.consented);
        assert_eq!(updated.scope, ConsentScope::Full);
    }

    #[test]
    fn test_persistence_survives_reload_multiple_records() {
        let dir = TempDir::new().unwrap();
        let mut revoked_id = String::new();
        {
            let reg = ConsentRegistry::open(dir.path());
            // Register several records with different states
            for i in 0..10 {
                reg.register(ConsentBody {
                    node_id: format!("persist-{i}"),
                    consented: true,
                    scope: ConsentScope::Full,
                });
            }
            // Revoke one
            reg.revoke("persist-3").unwrap();
            revoked_id = "persist-3".into();

            // Add a MetadataOnly record
            reg.register(ConsentBody {
                node_id: "persist-meta".into(),
                consented: true,
                scope: ConsentScope::MetadataOnly,
            });
        }
        // Reload from disk
        let reg2 = ConsentRegistry::open(dir.path());
        let all = reg2.list_all();
        assert_eq!(all.len(), 11); // 10 + 1 meta

        // Revoked record should still be revoked after reload
        let revoked = reg2.check(&revoked_id).unwrap();
        assert!(!revoked.consented);

        // MetadataOnly record should survive
        let meta = reg2.check("persist-meta").unwrap();
        assert!(meta.consented);
        assert_eq!(meta.scope, ConsentScope::MetadataOnly);
    }
}
