//! Data Commons Consent Manager
//!
//! Manages per-data-type consent levels for telemetry and data collection.
//! Each data type can be assigned one of four consent levels:
//!
//! - `None` — data is not permitted to leave the node
//! - `Metadata` — only non-identifying metadata fields are shared
//! - `Anonymized` — payload is shared with the node ID hashed/anonymized
//! - `Full` — complete payload is shared as-is
//!
//! The consent manager also provides a `sanitize_payload` method that applies
//! the appropriate transformation before data leaves the node.

use crate::mesh::NodeId;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// ConsentLevel
// ---------------------------------------------------------------------------

/// The level of consent granted for a particular data type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsentLevel {
    /// No data of this type may be shared.
    None,
    /// Only metadata fields (non-identifying) may be shared.
    Metadata,
    /// Data may be shared with the node identity anonymized (hashed).
    Anonymized,
    /// Full payload may be shared as-is.
    Full,
}

use serde::Deserialize;

// ---------------------------------------------------------------------------
// ConsentRecord
// ---------------------------------------------------------------------------

/// Tracks consent settings for specific namespaces or filters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentRecord {
    pub namespace_filter: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// DataConsentManager
// ---------------------------------------------------------------------------

/// Manages consent levels per data type and provides payload sanitization.
pub struct DataConsentManager {
    /// The node this consent manager belongs to.
    node_id: NodeId,
    /// Mapping from data type (e.g. "cpu_usage", "error_report") to consent level.
    consent_map: HashMap<String, ConsentLevel>,
}

impl DataConsentManager {
    /// Create a new `DataConsentManager` for the given node with all data types
    /// defaulting to `ConsentLevel::None` (opt-in model).
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            consent_map: HashMap::new(),
        }
    }

    /// Set the consent level for a specific data type.
    pub fn set_consent(&mut self, data_type: &str, level: ConsentLevel) {
        self.consent_map.insert(data_type.to_string(), level);
    }

    /// Get the current consent level for a data type. Returns `ConsentLevel::None`
    /// if no explicit consent has been set.
    pub fn get_consent(&self, data_type: &str) -> ConsentLevel {
        self.consent_map
            .get(data_type)
            .copied()
            .unwrap_or(ConsentLevel::None)
    }

    /// Sanitize a serializable payload according to the consent level for the
    /// given data type.
    ///
    /// - `None` → returns `Ok(None)` (data not permitted)
    /// - `Metadata` → returns only fields named in the allowlist
    /// - `Anonymized` → returns the full payload with `node_id` replaced by a hash
    /// - `Full` → returns the full payload as-is
    pub fn sanitize_payload<T: Serialize>(
        &self,
        data_type: &str,
        payload: &T,
    ) -> Result<Option<Value>, serde_json::Error> {
        let level = self.get_consent(data_type);

        match level {
            ConsentLevel::None => Ok(None),
            ConsentLevel::Metadata => {
                let mut raw = serde_json::to_value(payload)?;
                if let Some(obj) = raw.as_object_mut() {
                    let allowlist = self.metadata_fields(data_type);
                    obj.retain(|key, _| allowlist.contains(&key.as_str()));
                }
                Ok(Some(raw))
            }
            ConsentLevel::Anonymized => {
                let mut raw = serde_json::to_value(payload)?;
                // Hash the node_id for anonymization
                let hashed = self.hash_node_id();
                if let Some(obj) = raw.as_object_mut() {
                    if obj.contains_key("node_id") {
                        obj["node_id"] = json!(hashed);
                    }
                }
                Ok(Some(raw))
            }
            ConsentLevel::Full => Ok(Some(serde_json::to_value(payload)?)),
        }
    }

    /// Returns the set of field names considered "metadata" for a given data type.
    /// These are non-identifying fields safe to share at the Metadata consent level.
    fn metadata_fields(&self, data_type: &str) -> Vec<&'static str> {
        match data_type {
            "cpu_usage" | "memory_usage" | "disk_usage" => {
                vec!["metric_name", "value", "timestamp"]
            }
            "error_report" => vec!["event_kind", "sanitized_message", "timestamp"],
            _ => vec!["timestamp"],
        }
    }

    /// Produce a deterministic hash of the node_id for anonymization purposes.
    fn hash_node_id(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.node_id.0.as_bytes());
        let result = hasher.finalize();
        format!("anon-{}", &crate::crypto::hex_encode(result)[..16])
    }

    /// Returns the node ID this manager belongs to.
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// Returns a reference to the internal consent map.
    pub fn consent_map(&self) -> &HashMap<String, ConsentLevel> {
        &self.consent_map
    }

    /// Register a newly issued sharing token as an active consent.
    pub fn register_active_consent(consent: ActiveConsent) -> anyhow::Result<()> {
        let path = get_active_consents_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut list = if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            serde_json::from_str::<Vec<ActiveConsent>>(&content).unwrap_or_default()
        } else {
            Vec::new()
        };
        list.push(consent);
        let json = serde_json::to_string_pretty(&list)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Mark a token_id as revoked under the `mesh_token_revocations` table.
    pub fn revoke_consent(token_id: &str) -> anyhow::Result<()> {
        let path = get_token_revocations_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut revocations = if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            serde_json::from_str::<Vec<String>>(&content).unwrap_or_default()
        } else {
            Vec::new()
        };
        if !revocations.contains(&token_id.to_string()) {
            revocations.push(token_id.to_string());
            let json = serde_json::to_string_pretty(&revocations)?;
            std::fs::write(&path, json)?;
        }
        Ok(())
    }

    /// Check whether a token_id is revoked.
    pub fn is_token_revoked(token_id: &str) -> anyhow::Result<bool> {
        let path = get_token_revocations_path();
        if !path.exists() {
            return Ok(false);
        }
        let content = std::fs::read_to_string(&path)?;
        let revocations: Vec<String> = serde_json::from_str(&content).unwrap_or_default();
        Ok(revocations.contains(&token_id.to_string()))
    }

    /// List all registered consents that are active (non-revoked and non-expired).
    pub fn list_active_consents() -> anyhow::Result<Vec<ActiveConsent>> {
        let path = get_active_consents_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&path)?;
        let all_consents: Vec<ActiveConsent> = serde_json::from_str(&content).unwrap_or_default();

        let revocations_path = get_token_revocations_path();
        let revocations: std::collections::HashSet<String> = if revocations_path.exists() {
            let rev_content = std::fs::read_to_string(&revocations_path)?;
            serde_json::from_str::<Vec<String>>(&rev_content)
                .unwrap_or_default()
                .into_iter()
                .collect()
        } else {
            std::collections::HashSet::new()
        };

        let now = chrono::Utc::now().timestamp() as u64;
        let active = all_consents
            .into_iter()
            .filter(|c| !revocations.contains(&c.token_id) && c.expires_at >= now)
            .collect();
        Ok(active)
    }
}

// ---------------------------------------------------------------------------
// ActiveConsent
// ---------------------------------------------------------------------------

/// Represents a workspace sharing consent registered at the node level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveConsent {
    pub token_id: String,
    pub workspace_id: String,
    pub expires_at: u64,
    pub token: String,
}

fn get_config_dir() -> std::path::PathBuf {
    if let Ok(val) = std::env::var("XAVIER_CONFIG_DIR") {
        std::path::PathBuf::from(val)
    } else if let Some(dir) = dirs::config_dir() {
        dir.join("xavier")
    } else {
        std::path::PathBuf::from(".").join("xavier")
    }
}

fn get_active_consents_path() -> std::path::PathBuf {
    get_config_dir().join("mesh_active_consents.json")
}

fn get_token_revocations_path() -> std::path::PathBuf {
    get_config_dir().join("mesh_token_revocations.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    struct ConfigDirGuard(Option<std::ffi::OsString>);

    impl ConfigDirGuard {
        fn set(path: &std::path::Path) -> Self {
            let previous = std::env::var_os("XAVIER_CONFIG_DIR");
            std::env::set_var("XAVIER_CONFIG_DIR", path);
            Self(previous)
        }
    }

    impl Drop for ConfigDirGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => std::env::set_var("XAVIER_CONFIG_DIR", value),
                None => std::env::remove_var("XAVIER_CONFIG_DIR"),
            }
        }
    }

    #[derive(Serialize)]
    struct TestPayload {
        node_id: String,
        metric_name: String,
        value: f64,
        timestamp: i64,
    }

    fn make_manager() -> DataConsentManager {
        let node_id = NodeId("xv1-editor-one-abc123".to_string());
        let mut mgr = DataConsentManager::new(node_id);
        mgr.set_consent("cpu_usage", ConsentLevel::Metadata);
        mgr.set_consent("full_report", ConsentLevel::Full);
        mgr.set_consent("anonymous_report", ConsentLevel::Anonymized);
        mgr
    }

    #[test]
    fn test_default_consent_is_none() {
        let node_id = NodeId("xv1-test".to_string());
        let mgr = DataConsentManager::new(node_id);
        assert_eq!(mgr.get_consent("unknown_type"), ConsentLevel::None);
    }

    #[test]
    fn test_set_and_get_consent() {
        let mut mgr = DataConsentManager::new(NodeId("xv1-test".to_string()));
        mgr.set_consent("cpu_usage", ConsentLevel::Full);
        assert_eq!(mgr.get_consent("cpu_usage"), ConsentLevel::Full);
    }

    #[test]
    fn test_none_returns_none() {
        let mgr = DataConsentManager::new(NodeId("xv1-test".to_string()));
        let payload = TestPayload {
            node_id: "should-not-leak".to_string(),
            metric_name: "cpu".to_string(),
            value: 42.0,
            timestamp: 12345,
        };
        let result = mgr.sanitize_payload("unknown", &payload).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_metadata_only() {
        let mgr = make_manager();
        let payload = TestPayload {
            node_id: "xv1-secret".to_string(),
            metric_name: "cpu_usage".to_string(),
            value: 72.5,
            timestamp: 1000,
        };
        let result = mgr.sanitize_payload("cpu_usage", &payload).unwrap();
        let val = result.expect("should return Some");
        let obj = val.as_object().unwrap();
        // Metadata fields present
        assert!(obj.contains_key("metric_name"));
        assert!(obj.contains_key("value"));
        assert!(obj.contains_key("timestamp"));
        // Node ID removed
        assert!(!obj.contains_key("node_id"));
    }

    #[test]
    fn test_full_preserves_all() {
        let mgr = make_manager();
        let payload = TestPayload {
            node_id: "xv1-full-access".to_string(),
            metric_name: "full_report".to_string(),
            value: 99.9,
            timestamp: 2000,
        };
        let result = mgr.sanitize_payload("full_report", &payload).unwrap();
        let val = result.expect("should return Some");
        let obj = val.as_object().unwrap();
        assert!(obj.contains_key("node_id"));
        assert_eq!(obj["node_id"], "xv1-full-access");
    }

    #[test]
    fn test_anonymized_hashes_node_id() {
        let mgr = make_manager();
        let payload = TestPayload {
            node_id: "xv1-sensitive".to_string(),
            metric_name: "anonymous_report".to_string(),
            value: 50.0,
            timestamp: 3000,
        };
        let result = mgr.sanitize_payload("anonymous_report", &payload).unwrap();
        let val = result.expect("should return Some");
        let obj = val.as_object().unwrap();
        assert!(obj.contains_key("node_id"));
        assert_ne!(obj["node_id"], "xv1-sensitive");
        assert!(obj["node_id"].as_str().unwrap().starts_with("anon-"));
    }

    #[test]
    fn test_opt_out() {
        let mut mgr = make_manager();
        mgr.set_consent("full_report", ConsentLevel::Full);
        mgr.set_consent("full_report", ConsentLevel::None);
        let payload = TestPayload {
            node_id: "xv1-sensitive".to_string(),
            metric_name: "full_report".to_string(),
            value: 50.0,
            timestamp: 3000,
        };
        let result = mgr.sanitize_payload("full_report", &payload).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_error_report_allowlist() {
        let mut mgr = make_manager();
        mgr.set_consent("error_report", ConsentLevel::Metadata);
        #[derive(Serialize)]
        struct ErrorPayload {
            node_id: String,
            event_kind: String,
            sanitized_message: String,
            timestamp: i64,
            other_field: String,
        }
        let payload = ErrorPayload {
            node_id: "xv1-test".to_string(),
            event_kind: "crash".to_string(),
            sanitized_message: "null pointer".to_string(),
            timestamp: 1000,
            other_field: "drop me".to_string(),
        };
        let result = mgr.sanitize_payload("error_report", &payload).unwrap();
        let val = result.expect("should return Some");
        let obj = val.as_object().unwrap();
        assert!(obj.contains_key("event_kind"));
        assert!(obj.contains_key("sanitized_message"));
        assert!(obj.contains_key("timestamp"));
        assert!(!obj.contains_key("node_id"));
        assert!(!obj.contains_key("other_field"));
    }

    #[test]
    fn test_unknown_type_metadata() {
        let mut mgr = make_manager();
        mgr.set_consent("unknown_type", ConsentLevel::Metadata);
        let payload = TestPayload {
            node_id: "xv1-test".to_string(),
            metric_name: "unknown_type".to_string(),
            value: 50.0,
            timestamp: 3000,
        };
        let result = mgr.sanitize_payload("unknown_type", &payload).unwrap();
        let val = result.expect("should return Some");
        let obj = val.as_object().unwrap();
        assert!(obj.contains_key("timestamp"));
        assert!(!obj.contains_key("node_id"));
        assert!(!obj.contains_key("metric_name"));
        assert!(!obj.contains_key("value"));
    }

    #[test]
    fn test_anonymized_without_node_id() {
        let mut mgr = make_manager();
        mgr.set_consent("anonymous_report", ConsentLevel::Anonymized);
        #[derive(Serialize)]
        struct NoNodeIdPayload {
            metric_name: String,
            value: f64,
            timestamp: i64,
        }
        let payload = NoNodeIdPayload {
            metric_name: "anonymous_report".to_string(),
            value: 50.0,
            timestamp: 3000,
        };
        let result = mgr.sanitize_payload("anonymous_report", &payload).unwrap();
        let val = result.expect("should return Some");
        let obj = val.as_object().unwrap();
        assert!(!obj.contains_key("node_id"));
        assert!(obj.contains_key("metric_name"));
    }

    #[test]
    fn test_hash_behaviour() {
        let mgr1 = DataConsentManager::new(NodeId("node-a".to_string()));
        let mgr2 = DataConsentManager::new(NodeId("node-a".to_string()));
        let mgr3 = DataConsentManager::new(NodeId("node-b".to_string()));
        assert_eq!(mgr1.hash_node_id(), mgr2.hash_node_id());
        assert_ne!(mgr1.hash_node_id(), mgr3.hash_node_id());
    }

    #[test]
    fn test_non_object_payload_metadata() {
        let mut mgr = make_manager();
        mgr.set_consent("cpu_usage", ConsentLevel::Metadata);
        // Pinned behaviour: non-object payloads are returned unchanged under Metadata.
        let payload = vec![1, 2];
        let result = mgr.sanitize_payload("cpu_usage", &payload).unwrap();
        let val = result.expect("should return Some");
        assert!(val.is_array());
        let arr = val.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].as_u64().unwrap(), 1);
        assert_eq!(arr[1].as_u64().unwrap(), 2);
    }

    #[test]
    #[serial_test::serial]
    fn test_registry_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = ConfigDirGuard::set(dir.path());
        let token_id = "token-123".to_string();
        let consent = ActiveConsent {
            token_id: token_id.clone(),
            workspace_id: "ws-1".to_string(),
            expires_at: chrono::Utc::now().timestamp() as u64 + 10000,
            token: "secret".to_string(),
        };
        DataConsentManager::register_active_consent(consent).unwrap();
        let active = DataConsentManager::list_active_consents().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].token_id, token_id);

        DataConsentManager::revoke_consent(&token_id).unwrap();
        assert!(DataConsentManager::is_token_revoked(&token_id).unwrap());
        let active_after = DataConsentManager::list_active_consents().unwrap();
        assert_eq!(active_after.len(), 0);
    }

    #[test]
    #[serial_test::serial]
    fn test_registry_revoke_twice() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = ConfigDirGuard::set(dir.path());
        let token_id = "token-123".to_string();

        DataConsentManager::revoke_consent(&token_id).unwrap();
        DataConsentManager::revoke_consent(&token_id).unwrap();

        let rev_content =
            std::fs::read_to_string(dir.path().join("mesh_token_revocations.json")).unwrap();
        let revocations: Vec<String> = serde_json::from_str(&rev_content).unwrap();
        assert_eq!(revocations.len(), 1);
    }

    #[test]
    #[serial_test::serial]
    fn test_registry_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = ConfigDirGuard::set(dir.path());
        let consent = ActiveConsent {
            token_id: "token-exp".to_string(),
            workspace_id: "ws-2".to_string(),
            expires_at: chrono::Utc::now().timestamp() as u64 - 10000, // Past
            token: "secret-exp".to_string(),
        };
        DataConsentManager::register_active_consent(consent).unwrap();
        let active = DataConsentManager::list_active_consents().unwrap();
        assert_eq!(active.len(), 0);
    }

    #[test]
    fn test_accessors() {
        let node_id = NodeId("xv1-accessor".to_string());
        let mut mgr = DataConsentManager::new(node_id.clone());
        assert_eq!(mgr.node_id(), &node_id);
        mgr.set_consent("acc_test", ConsentLevel::Metadata);
        let map = mgr.consent_map();
        assert_eq!(map.get("acc_test"), Some(&ConsentLevel::Metadata));
    }
}
