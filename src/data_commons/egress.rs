//! Typed egress grants for training exports.
//!
//! Every training export runs under an [`EgressGrant`]. The grant fixes the purpose
//! (local training or Data Commons), the privacy level and the clearance ceiling.
//! Sources and the exporter consult [`exclusion_for`] per record and count each
//! exclusion through `SourceBatch::record_exclusion`. Only counts are kept, never
//! record ids or content.
//!
//! Owner-gated defaults (single named points to change later):
//! * [`DATA_COMMONS_CLEARANCE_CEILING`]: clearance ceiling for Data Commons grants.
//! * [`LOCAL_TRAINING_EXCLUDES_PRIVATE`]: whether local training drops private records.
//! * [`DataCommonsConsent`] has no production constructor yet (consent source undecided).
//!   Data Commons grants therefore cannot be built outside tests until it does.

use super::privacy::PrivacyLevel;
use crate::security::clearance::ClearanceLevel;
use serde::Serialize;
use serde_json::Value;
use std::fmt;

/// Owner-gated default: clearance ceiling for Data Commons grants.
pub const DATA_COMMONS_CLEARANCE_CEILING: ClearanceLevel = ClearanceLevel::Unclassified;

/// Owner-gated default: local training excludes records flagged private.
pub const LOCAL_TRAINING_EXCLUDES_PRIVATE: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum EgressPurpose {
    LocalTraining,
    DataCommons,
}

/// The part of a grant that sources need. Copied into `SourceContext`.
#[derive(Clone, Copy, Debug)]
pub struct EgressPolicy {
    pub purpose: EgressPurpose,
    /// `None` means sources keep their own clearance ceilings.
    pub max_clearance: Option<ClearanceLevel>,
}

/// An explicit, typed permission to export. Fields are private: build it with a constructor.
#[derive(Clone, Debug)]
pub struct EgressGrant {
    purpose: EgressPurpose,
    privacy_level: PrivacyLevel,
    workspace_id: Option<String>,
    max_clearance: Option<ClearanceLevel>,
}

/// Returned when a grant cannot be constructed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EgressDenied {
    pub reason: &'static str,
}

impl fmt::Display for EgressDenied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "egress denied: {}", self.reason)
    }
}

impl std::error::Error for EgressDenied {}

/// Proof of Data Commons consent. The field is private and there is no production
/// constructor (NEEDS OWNER: consent source of truth).
#[derive(Debug)]
pub struct DataCommonsConsent {
    _private: (),
}

impl DataCommonsConsent {
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self { _private: () }
    }
}

impl EgressGrant {
    /// Local training grant. No clearance ceiling: sources keep their own.
    pub fn local_training(level: PrivacyLevel) -> Self {
        Self {
            purpose: EgressPurpose::LocalTraining,
            privacy_level: level,
            workspace_id: None,
            max_clearance: None,
        }
    }

    /// Data Commons grant. Fails closed for P4 (local-only) data.
    /// STUB (commit 1, red): accepts every level. The P4 refusal comes in commit 2.
    pub fn data_commons(
        workspace_id: String,
        level: PrivacyLevel,
        _consent: DataCommonsConsent,
    ) -> Result<Self, EgressDenied> {
        Ok(Self {
            purpose: EgressPurpose::DataCommons,
            privacy_level: level,
            workspace_id: Some(workspace_id),
            max_clearance: Some(DATA_COMMONS_CLEARANCE_CEILING),
        })
    }

    pub fn policy(&self) -> EgressPolicy {
        EgressPolicy {
            purpose: self.purpose,
            max_clearance: self.max_clearance,
        }
    }

    pub fn purpose(&self) -> EgressPurpose {
        self.purpose
    }

    pub fn privacy_level(&self) -> PrivacyLevel {
        self.privacy_level
    }

    pub fn workspace_id(&self) -> Option<&str> {
        self.workspace_id.as_deref()
    }
}

/// Why a record was excluded from an export.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ExclusionReason {
    NoConsent,
    Revoked,
    Private,
    ClearanceAboveGrant,
    LocalOnly,
}

impl ExclusionReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExclusionReason::NoConsent => "no_consent",
            ExclusionReason::Revoked => "revoked",
            ExclusionReason::Private => "private",
            ExclusionReason::ClearanceAboveGrant => "clearance_above_grant",
            ExclusionReason::LocalOnly => "local_only",
        }
    }
}

/// Decide whether one record may leave under `policy`. `None` means included.
/// STUB (commit 1, red): always includes. The real rules come in commit 2.
pub fn exclusion_for(
    _policy: EgressPolicy,
    _metadata: &Value,
    _clearance: Option<ClearanceLevel>,
) -> Option<ExclusionReason> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::MemoryRecord;
    use serde_json::json;

    fn local() -> EgressPolicy {
        EgressGrant::local_training(PrivacyLevel::P2).policy()
    }

    fn data_commons() -> EgressPolicy {
        EgressGrant::data_commons(
            "ws1".to_string(),
            PrivacyLevel::P2,
            DataCommonsConsent::for_test(),
        )
        .expect("P2 data commons grant")
        .policy()
    }

    #[test]
    fn exclusion_matrix_local_training() {
        let cases: Vec<(Value, Option<ClearanceLevel>, Option<ExclusionReason>)> = vec![
            (json!({}), Some(ClearanceLevel::Unclassified), None),
            (json!({"revoked": false}), None, None),
            (
                json!({"revoked": true}),
                None,
                Some(ExclusionReason::Revoked),
            ),
            // Non-boolean flags are not flags (same rule as MemoryRecord::is_authorized_for_embedding).
            (json!({"revoked": "true"}), None, None),
            (
                json!({"is_private": true}),
                None,
                Some(ExclusionReason::Private),
            ),
            (
                json!({"visibility": "private"}),
                None,
                Some(ExclusionReason::Private),
            ),
            (
                json!({"visibility": "public", "is_private": false}),
                None,
                None,
            ),
            (
                json!({"revoked": true, "is_private": true}),
                None,
                Some(ExclusionReason::Revoked),
            ),
            // LocalOnly and consent are Data Commons rules only.
            (json!({"privacy_p4_local_only": true}), None, None),
            (json!({"consent_given": false}), None, None),
        ];
        for (meta, clearance, expected) in cases {
            assert_eq!(
                exclusion_for(local(), &meta, clearance),
                expected,
                "local training, metadata {meta}, clearance {clearance:?}"
            );
        }
    }

    #[test]
    fn exclusion_matrix_data_commons() {
        let top = Some(ClearanceLevel::Unclassified);
        let cases: Vec<(Value, Option<ClearanceLevel>, Option<ExclusionReason>)> = vec![
            (json!({}), top, Some(ExclusionReason::NoConsent)),
            (json!({"consent_given": true}), top, None),
            (
                json!({"consent_given": "true"}),
                top,
                Some(ExclusionReason::NoConsent),
            ),
            (
                json!({"consent_given": true}),
                Some(ClearanceLevel::Internal),
                Some(ExclusionReason::ClearanceAboveGrant),
            ),
            (
                json!({"consent_given": true}),
                None,
                Some(ExclusionReason::ClearanceAboveGrant),
            ),
            (
                json!({"consent_given": true, "privacy_p4_local_only": true}),
                top,
                Some(ExclusionReason::LocalOnly),
            ),
            (
                json!({"consent_given": true, "local_only": true}),
                top,
                Some(ExclusionReason::LocalOnly),
            ),
            (
                json!({"consent_given": false, "revoked": true}),
                top,
                Some(ExclusionReason::Revoked),
            ),
            (
                json!({"is_private": true}),
                top,
                Some(ExclusionReason::Private),
            ),
            (
                json!({"consent_given": true, "visibility": "private"}),
                top,
                Some(ExclusionReason::Private),
            ),
            (
                json!({"consent_given": true, "revoked": true}),
                top,
                Some(ExclusionReason::Revoked),
            ),
        ];
        for (meta, clearance, expected) in cases {
            assert_eq!(
                exclusion_for(data_commons(), &meta, clearance),
                expected,
                "data commons, metadata {meta}, clearance {clearance:?}"
            );
        }
    }

    #[test]
    fn data_commons_grant_rejects_p4() {
        let denied = EgressGrant::data_commons(
            "ws1".to_string(),
            PrivacyLevel::P4,
            DataCommonsConsent::for_test(),
        );
        assert!(denied.is_err(), "P4 must not be granted for Data Commons");
        for level in [PrivacyLevel::P0, PrivacyLevel::P2, PrivacyLevel::P3] {
            assert!(
                EgressGrant::data_commons("ws1".to_string(), level, DataCommonsConsent::for_test())
                    .is_ok(),
                "{level:?} should be grantable"
            );
        }
    }

    #[test]
    fn local_policy_agrees_with_is_authorized_for_embedding() {
        let top = ClearanceLevel::TopSecret;
        let revoked_values: [Option<Value>; 4] = [
            None,
            Some(json!(false)),
            Some(json!(true)),
            Some(json!("true")),
        ];
        let private_values: [Option<Value>; 4] = [
            None,
            Some(json!(false)),
            Some(json!(true)),
            Some(json!("yes")),
        ];
        let visibility_values: [Option<Value>; 4] = [
            None,
            Some(json!("public")),
            Some(json!("private")),
            Some(json!("internal")),
        ];
        let mut checked = 0;
        for revoked in &revoked_values {
            for private in &private_values {
                for visibility in &visibility_values {
                    let mut meta = serde_json::Map::new();
                    for (key, value) in [
                        ("revoked", revoked),
                        ("is_private", private),
                        ("visibility", visibility),
                    ] {
                        if let Some(v) = value {
                            meta.insert(key.to_string(), v.clone());
                        }
                    }
                    let metadata = Value::Object(meta);
                    for clearance in [ClearanceLevel::Unclassified, top] {
                        let record = MemoryRecord {
                            metadata: metadata.clone(),
                            clearance,
                            ..MemoryRecord::default()
                        };
                        let authorized = record.is_authorized_for_embedding(true, top);
                        let included = exclusion_for(local(), &metadata, Some(clearance)).is_none();
                        assert_eq!(
                            included, authorized,
                            "metadata {metadata}, clearance {clearance:?}"
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 4 * 4 * 4 * 2);
    }

    #[test]
    fn record_exclusion_counts_only() {
        use crate::data_commons::sources::SourceBatch;

        let mut batch = SourceBatch::default();
        batch.record_exclusion(ExclusionReason::NoConsent);
        batch.record_exclusion(ExclusionReason::Revoked);
        batch.record_exclusion(ExclusionReason::Private);
        batch.record_exclusion(ExclusionReason::Private);
        batch.record_exclusion(ExclusionReason::LocalOnly);
        batch.record_exclusion(ExclusionReason::ClearanceAboveGrant);

        assert_eq!(batch.excluded_no_consent, 1);
        assert_eq!(batch.excluded_revoked, 1);
        // Keys are fixed labels and values are counts: no record ids or content.
        assert_eq!(
            serde_json::to_string(&batch.excluded_other).unwrap(),
            r#"{"egress:clearance_above_grant":1,"egress:local_only":1,"egress:private":2}"#
        );
    }

    #[test]
    fn exclusion_reason_labels_are_stable() {
        assert_eq!(ExclusionReason::NoConsent.as_str(), "no_consent");
        assert_eq!(ExclusionReason::Revoked.as_str(), "revoked");
        assert_eq!(ExclusionReason::Private.as_str(), "private");
        assert_eq!(
            ExclusionReason::ClearanceAboveGrant.as_str(),
            "clearance_above_grant"
        );
        assert_eq!(ExclusionReason::LocalOnly.as_str(), "local_only");
    }
}
