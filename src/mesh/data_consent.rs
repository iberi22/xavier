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
use std::collections::HashMap;
use sha2::{Sha256, Digest};
use hex;

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
                    obj.retain(|key, _| allowlist.iter().any(|&f| f == key.as_str()));
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
        format!("anon-{}", &hex::encode(result)[..16])
    }

    /// Returns the node ID this manager belongs to.
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// Returns a reference to the internal consent map.
    pub fn consent_map(&self) -> &HashMap<String, ConsentLevel> {
        &self.consent_map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

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
}
