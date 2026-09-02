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
}
