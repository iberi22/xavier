use serde::{Deserialize, Serialize};
use std::fmt;

/// Network or resource visibility status within the mesh ecosystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Private,
}

impl Default for Visibility {
    fn default() -> Self {
        Self::Private
    }
}

impl Visibility {
    /// Returns true if the visibility is Public.
    pub fn is_public(&self) -> bool {
        matches!(self, Self::Public)
    }
}

impl fmt::Display for Visibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Public => write!(f, "public"),
            Self::Private => write!(f, "private"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visibility_default_and_helpers() {
        let vis = Visibility::default();
        assert_eq!(vis, Visibility::Private);
        assert!(!vis.is_public());
        assert_eq!(vis.to_string(), "private");

        let pub_vis = Visibility::Public;
        assert!(pub_vis.is_public());
        assert_eq!(pub_vis.to_string(), "public");
    }

    #[test]
    fn test_visibility_serde_roundtrip() {
        let pub_vis = Visibility::Public;
        let json_pub = serde_json::to_string(&pub_vis).unwrap();
        assert_eq!(json_pub, "\"public\"");
        let deserialized_pub: Visibility = serde_json::from_str(&json_pub).unwrap();
        assert_eq!(deserialized_pub, Visibility::Public);

        let priv_vis = Visibility::Private;
        let json_priv = serde_json::to_string(&priv_vis).unwrap();
        assert_eq!(json_priv, "\"private\"");
        let deserialized_priv: Visibility = serde_json::from_str(&json_priv).unwrap();
        assert_eq!(deserialized_priv, Visibility::Private);
    }
}
