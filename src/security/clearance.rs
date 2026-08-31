use crate::security::auth::UserRole;
use serde::{Deserialize, Serialize};

pub use xavier_core_logic::ClearanceLevel;

/// Helper that checks if a requester has a clearance level high enough
/// to access a document with a given clearance level.
/// Access is granted if requester >= doc.
pub fn can_access(requester: ClearanceLevel, doc: ClearanceLevel) -> bool {
    requester >= doc
}

/// Returns default clearance level assigned to a given user role.
pub fn role_clearance(role: UserRole) -> ClearanceLevel {
    match role {
        UserRole::Admin => ClearanceLevel::TopSecret,
        UserRole::User => ClearanceLevel::Confidential,
        UserRole::Readonly => ClearanceLevel::Internal,
    }
}

/// Helper that checks if a user role has clearance level high enough
/// to access a resource requiring a given clearance level.
pub fn can_access_clearance(role: UserRole, required_level: ClearanceLevel) -> bool {
    role_clearance(role) >= required_level
}

/// Read-middleware: redact content if requester clearance is insufficient.
/// Returns `REDACTED` placeholder when access is denied, otherwise original content.
pub fn redact_if_needed(
    requester: ClearanceLevel,
    doc_level: ClearanceLevel,
    content: &str,
) -> String {
    if can_access(requester, doc_level) {
        content.to_string()
    } else {
        format!("[REDACTED: requires {}]", doc_level.as_str().to_uppercase())
    }
}

/// Filter a list of (id, clearance, content) tuples by requester clearance.
/// Returns only accessible entries with redacted content for denied ones excluded.
pub fn filter_by_clearance(
    requester: ClearanceLevel,
    docs: Vec<(String, ClearanceLevel, String)>,
) -> Vec<(String, ClearanceLevel, String)> {
    docs.into_iter()
        .filter(|(_, lvl, _)| can_access(requester, *lvl))
        .collect()
}

/// Clearance enforcer middleware — wraps read paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClearanceEnforcer {
    pub requester_level: ClearanceLevel,
}

impl ClearanceEnforcer {
    pub fn new(requester_level: ClearanceLevel) -> Self {
        Self { requester_level }
    }

    pub fn from_role(role: UserRole) -> Self {
        Self::new(role_clearance(role))
    }

    pub fn can_read(&self, doc_level: ClearanceLevel) -> bool {
        can_access(self.requester_level, doc_level)
    }

    pub fn redact(&self, doc_level: ClearanceLevel, content: &str) -> String {
        redact_if_needed(self.requester_level, doc_level, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clearance_ordering() {
        assert!(ClearanceLevel::Unclassified < ClearanceLevel::Internal);
        assert!(ClearanceLevel::Internal < ClearanceLevel::Restricted);
        assert!(ClearanceLevel::Restricted < ClearanceLevel::Confidential);
        assert!(ClearanceLevel::Confidential < ClearanceLevel::Secret);
        assert!(ClearanceLevel::Secret < ClearanceLevel::TopSecret);
    }

    #[test]
    fn test_clearance_from_u8() {
        assert_eq!(ClearanceLevel::from(0), ClearanceLevel::Unclassified);
        assert_eq!(ClearanceLevel::from(1), ClearanceLevel::Internal);
        assert_eq!(ClearanceLevel::from(2), ClearanceLevel::Restricted);
        assert_eq!(ClearanceLevel::from(3), ClearanceLevel::Confidential);
        assert_eq!(ClearanceLevel::from(4), ClearanceLevel::Secret);
        assert_eq!(ClearanceLevel::from(5), ClearanceLevel::TopSecret);
        assert_eq!(ClearanceLevel::from(100), ClearanceLevel::Unclassified); // Fallback
    }

    #[test]
    fn test_clearance_into_u8() {
        assert_eq!(u8::from(ClearanceLevel::Unclassified), 0);
        assert_eq!(u8::from(ClearanceLevel::Internal), 1);
        assert_eq!(u8::from(ClearanceLevel::Restricted), 2);
        assert_eq!(u8::from(ClearanceLevel::Confidential), 3);
        assert_eq!(u8::from(ClearanceLevel::Secret), 4);
        assert_eq!(u8::from(ClearanceLevel::TopSecret), 5);
    }

    #[test]
    fn test_clearance_from_str() {
        assert_eq!(
            ClearanceLevel::from("unclassified"),
            ClearanceLevel::Unclassified
        );
        assert_eq!(ClearanceLevel::from("INTERNAL"), ClearanceLevel::Internal);
        assert_eq!(
            ClearanceLevel::from("Restricted"),
            ClearanceLevel::Restricted
        );
        assert_eq!(
            ClearanceLevel::from("confidential"),
            ClearanceLevel::Confidential
        );
        assert_eq!(ClearanceLevel::from("SECRET"), ClearanceLevel::Secret);
        assert_eq!(ClearanceLevel::from("topsecret"), ClearanceLevel::TopSecret);
        assert_eq!(
            ClearanceLevel::from("TOP_SECRET"),
            ClearanceLevel::TopSecret
        );
        assert_eq!(ClearanceLevel::from("BOGUS"), ClearanceLevel::Unclassified);
        // Fallback
    }

    #[test]
    fn test_can_access_logic() {
        // Equal clearance can access
        assert!(can_access(
            ClearanceLevel::Internal,
            ClearanceLevel::Internal
        ));
        // Higher clearance can access lower clearance
        assert!(can_access(
            ClearanceLevel::TopSecret,
            ClearanceLevel::Secret
        ));
        assert!(can_access(
            ClearanceLevel::Secret,
            ClearanceLevel::Unclassified
        ));
        // Lower clearance CANNOT access higher clearance
        assert!(!can_access(
            ClearanceLevel::Unclassified,
            ClearanceLevel::Internal
        ));
        assert!(!can_access(
            ClearanceLevel::Confidential,
            ClearanceLevel::TopSecret
        ));
    }

    #[test]
    fn test_clearance_serialization() {
        let serialized = serde_json::to_string(&ClearanceLevel::Unclassified).unwrap();
        assert_eq!(serialized, "\"UNCLASSIFIED\"");

        let deserialized: ClearanceLevel = serde_json::from_str("\"UNCLASSIFIED\"").unwrap();
        assert_eq!(deserialized, ClearanceLevel::Unclassified);

        let serialized_ts = serde_json::to_string(&ClearanceLevel::TopSecret).unwrap();
        assert_eq!(serialized_ts, "\"TOPSECRET\"");

        let deserialized_ts: ClearanceLevel = serde_json::from_str("\"TOPSECRET\"").unwrap();
        assert_eq!(deserialized_ts, ClearanceLevel::TopSecret);
    }

    #[test]
    fn test_clearance_role_inheritance_matrix() {
        assert_eq!(role_clearance(UserRole::Admin), ClearanceLevel::TopSecret);
        assert_eq!(role_clearance(UserRole::User), ClearanceLevel::Confidential);
        assert_eq!(role_clearance(UserRole::Readonly), ClearanceLevel::Internal);

        // Admin (TopSecret) can access all levels
        assert!(can_access_clearance(
            UserRole::Admin,
            ClearanceLevel::Unclassified
        ));
        assert!(can_access_clearance(
            UserRole::Admin,
            ClearanceLevel::Internal
        ));
        assert!(can_access_clearance(
            UserRole::Admin,
            ClearanceLevel::Restricted
        ));
        assert!(can_access_clearance(
            UserRole::Admin,
            ClearanceLevel::Confidential
        ));
        assert!(can_access_clearance(
            UserRole::Admin,
            ClearanceLevel::Secret
        ));
        assert!(can_access_clearance(
            UserRole::Admin,
            ClearanceLevel::TopSecret
        ));

        // User (Confidential) can access up to Confidential, cannot access Secret or TopSecret
        assert!(can_access_clearance(
            UserRole::User,
            ClearanceLevel::Unclassified
        ));
        assert!(can_access_clearance(
            UserRole::User,
            ClearanceLevel::Internal
        ));
        assert!(can_access_clearance(
            UserRole::User,
            ClearanceLevel::Restricted
        ));
        assert!(can_access_clearance(
            UserRole::User,
            ClearanceLevel::Confidential
        ));
        assert!(!can_access_clearance(
            UserRole::User,
            ClearanceLevel::Secret
        ));
        assert!(!can_access_clearance(
            UserRole::User,
            ClearanceLevel::TopSecret
        ));

        // Readonly (Internal) can access Unclassified and Internal, cannot access Restricted+
        assert!(can_access_clearance(
            UserRole::Readonly,
            ClearanceLevel::Unclassified
        ));
        assert!(can_access_clearance(
            UserRole::Readonly,
            ClearanceLevel::Internal
        ));
        assert!(!can_access_clearance(
            UserRole::Readonly,
            ClearanceLevel::Restricted
        ));
        assert!(!can_access_clearance(
            UserRole::Readonly,
            ClearanceLevel::Confidential
        ));
        assert!(!can_access_clearance(
            UserRole::Readonly,
            ClearanceLevel::Secret
        ));
        assert!(!can_access_clearance(
            UserRole::Readonly,
            ClearanceLevel::TopSecret
        ));
    }

    #[test]
    fn test_redact_middleware() {
        let enforcer = ClearanceEnforcer::new(ClearanceLevel::Confidential);
        // Can read its own level
        assert_eq!(
            enforcer.redact(ClearanceLevel::Confidential, "secret data"),
            "secret data"
        );
        // Cannot read higher level -> REDACTED
        let redacted = enforcer.redact(ClearanceLevel::TopSecret, "top secret");
        assert!(redacted.contains("REDACTED"));
        assert!(redacted.contains("TOP_SECRET"));
        // standalone helpers
        assert_eq!(
            redact_if_needed(ClearanceLevel::Internal, ClearanceLevel::Unclassified, "ok"),
            "ok"
        );
        assert!(
            redact_if_needed(ClearanceLevel::Unclassified, ClearanceLevel::Secret, "x")
                .contains("REDACTED")
        );
        // filter_by_clearance
        let docs = vec![
            ("a".into(), ClearanceLevel::Unclassified, "a".into()),
            ("b".into(), ClearanceLevel::TopSecret, "b".into()),
            ("c".into(), ClearanceLevel::Confidential, "c".into()),
        ];
        let filtered = filter_by_clearance(ClearanceLevel::Confidential, docs);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().any(|(id, _, _)| id == "a"));
        assert!(filtered.iter().any(|(id, _, _)| id == "c"));
    }
}
