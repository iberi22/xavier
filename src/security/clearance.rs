use serde::{Deserialize, Serialize};

/// Clearance levels for information classification.
/// 0 = UNCLASSIFIED
/// 1 = INTERNAL
/// 2 = RESTRICTED
/// 3 = CONFIDENTIAL
/// 4 = SECRET
/// 5 = TOPSECRET
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ClearanceLevel {
    #[serde(rename = "UNCLASSIFIED")]
    Unclassified = 0,
    #[serde(rename = "INTERNAL")]
    Internal = 1,
    #[serde(rename = "RESTRICTED")]
    Restricted = 2,
    #[serde(rename = "CONFIDENTIAL")]
    Confidential = 3,
    #[serde(rename = "SECRET")]
    Secret = 4,
    #[serde(rename = "TOPSECRET")]
    TopSecret = 5,
}

impl Default for ClearanceLevel {
    fn default() -> Self {
        Self::Unclassified
    }
}

impl From<u8> for ClearanceLevel {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Unclassified,
            1 => Self::Internal,
            2 => Self::Restricted,
            3 => Self::Confidential,
            4 => Self::Secret,
            5 => Self::TopSecret,
            _ => Self::Unclassified,
        }
    }
}

impl From<ClearanceLevel> for u8 {
    fn from(level: ClearanceLevel) -> Self {
        level as u8
    }
}

impl From<&str> for ClearanceLevel {
    fn from(value: &str) -> Self {
        match value.to_ascii_uppercase().as_str() {
            "UNCLASSIFIED" => Self::Unclassified,
            "INTERNAL" => Self::Internal,
            "RESTRICTED" => Self::Restricted,
            "CONFIDENTIAL" => Self::Confidential,
            "SECRET" => Self::Secret,
            "TOPSECRET" | "TOP_SECRET" => Self::TopSecret,
            _ => Self::Unclassified,
        }
    }
}

impl ClearanceLevel {
    /// As str.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unclassified => "unclassified",
            Self::Internal => "internal",
            Self::Restricted => "restricted",
            Self::Confidential => "confidential",
            Self::Secret => "secret",
            Self::TopSecret => "top_secret",
        }
    }

    /// Parse.
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "unclassified" => Self::Unclassified,
            "internal" => Self::Internal,
            "restricted" => Self::Restricted,
            "confidential" => Self::Confidential,
            "secret" => Self::Secret,
            _ => Self::TopSecret,
        }
    }
}

/// Helper that checks if a requester has a clearance level high enough
/// to access a document with a given clearance level.
/// Access is granted if requester >= doc.
pub fn can_access(requester: ClearanceLevel, doc: ClearanceLevel) -> bool {
    requester >= doc
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
}
