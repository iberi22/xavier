//! Space permissions — role-based ACL over Spaces (T-02)
//!
//! Roles map to capabilities checked via `can(role, action)`. Admin has full
//! control (member management, settings, revoke). Reader is read-only.

use serde::{Deserialize, Serialize};

use super::invite::SpaceRole;

/// Action that can be checked against a role
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceAction {
    /// Read RAG, channel, graph
    Read,
    /// Post messages, add RAG entries
    Write,
    /// Pin messages, manage invites (member level)
    ManageMembers,
    /// Full admin: add/remove/ban, edit settings, revoke grants, promote
    Admin,
}

/// Check if a role can perform an action.
/// Hierarchy: Admin > Moderator > Member > Reader
pub fn can(role: SpaceRole, action: SpaceAction) -> bool {
    match (role, action) {
        // Admin can do everything
        (SpaceRole::Admin, _) => true,
        // Moderator: read, write, manage members (no admin)
        (SpaceRole::Moderator, SpaceAction::Read) => true,
        (SpaceRole::Moderator, SpaceAction::Write) => true,
        (SpaceRole::Moderator, SpaceAction::ManageMembers) => true,
        (SpaceRole::Moderator, SpaceAction::Admin) => false,
        // Member: read + write only
        (SpaceRole::Member, SpaceAction::Read) => true,
        (SpaceRole::Member, SpaceAction::Write) => true,
        (SpaceRole::Member, SpaceAction::ManageMembers) => false,
        (SpaceRole::Member, SpaceAction::Admin) => false,
        // Reader: read only
        (SpaceRole::Reader, SpaceAction::Read) => true,
        (SpaceRole::Reader, SpaceAction::Write) => false,
        (SpaceRole::Reader, SpaceAction::ManageMembers) => false,
        (SpaceRole::Reader, SpaceAction::Admin) => false,
    }
}

/// Membership record for a node in a Space
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceMembership {
    pub node_id: String,
    pub role: SpaceRole,
    pub joined_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_can_all() {
        for action in [
            SpaceAction::Read,
            SpaceAction::Write,
            SpaceAction::ManageMembers,
            SpaceAction::Admin,
        ] {
            assert!(can(SpaceRole::Admin, action));
        }
    }

    #[test]
    fn moderator_limits() {
        assert!(can(SpaceRole::Moderator, SpaceAction::Read));
        assert!(can(SpaceRole::Moderator, SpaceAction::Write));
        assert!(can(SpaceRole::Moderator, SpaceAction::ManageMembers));
        assert!(!can(SpaceRole::Moderator, SpaceAction::Admin));
    }

    #[test]
    fn member_read_write_only() {
        assert!(can(SpaceRole::Member, SpaceAction::Read));
        assert!(can(SpaceRole::Member, SpaceAction::Write));
        assert!(!can(SpaceRole::Member, SpaceAction::ManageMembers));
        assert!(!can(SpaceRole::Member, SpaceAction::Admin));
    }

    #[test]
    fn reader_read_only() {
        assert!(can(SpaceRole::Reader, SpaceAction::Read));
        assert!(!can(SpaceRole::Reader, SpaceAction::Write));
        assert!(!can(SpaceRole::Reader, SpaceAction::ManageMembers));
        assert!(!can(SpaceRole::Reader, SpaceAction::Admin));
    }
    #[test]
    fn membership_records_role_and_timestamp() {
        let m = SpaceMembership {
            node_id: "node-001".to_string(),
            role: SpaceRole::Moderator,
            joined_at: chrono::Utc::now(),
        };
        assert_eq!(m.node_id, "node-001");
        assert_eq!(m.role, SpaceRole::Moderator);
        // joined_at should be very recent (within last minute)
        let now = chrono::Utc::now();
        assert!(m.joined_at <= now);
        assert!(now - m.joined_at < chrono::Duration::minutes(1));
    }

    #[test]
    fn membership_serialization_roundtrip() {
        let m = SpaceMembership {
            node_id: "node-002".to_string(),
            role: SpaceRole::Admin,
            joined_at: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&m).unwrap();
        let m2: SpaceMembership = serde_json::from_str(&json).unwrap();
        assert_eq!(m.node_id, m2.node_id);
        assert_eq!(m.role, m2.role);
        // timestamp roundtrip should be within microseconds
        assert!((m.joined_at - m2.joined_at).num_microseconds().unwrap().abs() < 1000);
    }

    #[test]
    fn action_serialization_uses_snake_case() {
        let action = SpaceAction::ManageMembers;
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, "\"manage_members\"");
        let a2: SpaceAction = serde_json::from_str(&json).unwrap();
        assert_eq!(action, a2);
    }

    #[test]
    fn reader_cannot_manage_members_or_admin() {
        // Defensive: confirm reader is locked out of privileged operations
        assert!(!can(SpaceRole::Reader, SpaceAction::ManageMembers));
        assert!(!can(SpaceRole::Reader, SpaceAction::Admin));
    }

    #[test]
    fn member_cannot_manage_members_or_admin() {
        // Member has read+write only, no member management
        assert!(!can(SpaceRole::Member, SpaceAction::ManageMembers));
        assert!(!can(SpaceRole::Member, SpaceAction::Admin));
    }

    #[test]
    fn moderator_cannot_admin() {
        // Moderator has all except admin
        assert!(!can(SpaceRole::Moderator, SpaceAction::Admin));
    }

    #[test]
    fn admin_cannot_be_revoked_by_lower_role() {
        // Admin can do everything, including admin operations
        // Lower roles cannot promote themselves
        assert!(can(SpaceRole::Admin, SpaceAction::Admin));
        // No lower role can do admin
        for role in [SpaceRole::Moderator, SpaceRole::Member, SpaceRole::Reader] {
            assert!(!can(role, SpaceAction::Admin));
        }
    }

}
