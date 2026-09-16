use crate::mesh::visibility::Visibility;
use serde::{Deserialize, Serialize};

/// Request sent by a node to join a mesh network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinRequest {
    pub network_id: String,
    pub node_id: String,
    pub visibility: Visibility,
    pub pairing_secret: String,
}

/// Decision response for a join request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinDecision {
    pub network_id: String,
    pub node_id: String,
    pub approved: bool,
    pub reason: Option<String>,
    pub expires_at: u64,
}

impl JoinDecision {
    /// Returns true if the decision has expired relative to `now_secs` (unix epoch seconds).
    pub fn is_expired(&self, now_secs: u64) -> bool {
        now_secs > self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_request_serde_roundtrip() {
        let req = JoinRequest {
            network_id: "net-alpha".to_string(),
            node_id: "node-101".to_string(),
            visibility: Visibility::Public,
            pairing_secret: "secret-123".to_string(),
        };

        let json = serde_json::to_string(&req).unwrap();
        let deserialized: JoinRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, deserialized);
    }

    #[test]
    fn test_join_decision_serde_roundtrip() {
        let decision = JoinDecision {
            network_id: "net-alpha".to_string(),
            node_id: "node-101".to_string(),
            approved: true,
            reason: Some("Approved by admin".to_string()),
            expires_at: 1700000000,
        };

        let json = serde_json::to_string(&decision).unwrap();
        let deserialized: JoinDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(decision, deserialized);
    }

    #[test]
    fn test_join_decision_expiry_boundary() {
        let decision = JoinDecision {
            network_id: "net-alpha".to_string(),
            node_id: "node-101".to_string(),
            approved: true,
            reason: None,
            expires_at: 1000,
        };

        // Window testing (past, exact boundary, future window)
        assert!(!decision.is_expired(500));
        assert!(!decision.is_expired(1000));
        assert!(decision.is_expired(1001));
        assert!(decision.is_expired(2000));
    }
}
