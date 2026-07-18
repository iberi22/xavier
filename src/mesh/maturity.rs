use serde::{Deserialize, Serialize};

/// Maturity report of the Xavier Mesh components.
/// Exposes honest maturity percentages and feature presence flags.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeshMaturityReport {
    /// HTTP mesh transport: fully functional for handshakes, manifests, chunks sync, session sharing.
    pub http_transport: bool,
    pub http_transport_percent: u8,
    /// libp2p transport: legacy/broken in current build, superseded by Iroh.
    pub libp2p: bool,
    pub libp2p_percent: u8,
    /// Mesh access control lists (ACL): fully functional.
    pub acl: bool,
    pub acl_percent: u8,
    /// Tokenomics: XP-based placeholder/mock system is present.
    pub tokenomics: bool,
    pub tokenomics_percent: u8,
    /// On-chain governance (DAO): not implemented/unsupported.
    pub onchain_gov: bool,
    pub onchain_gov_percent: u8,
}

impl Default for MeshMaturityReport {
    fn default() -> Self {
        Self {
            http_transport: true,
            http_transport_percent: 100,
            libp2p: false,
            libp2p_percent: 10, // Broken / legacy code exists, hence 10%
            acl: true,
            acl_percent: 90, // Fully operational, with some enterprise/namespaces expansions planned
            tokenomics: true,
            tokenomics_percent: 40, // Placeholder XP simulation is implemented
            onchain_gov: false,
            onchain_gov_percent: 0, // Unimplemented
        }
    }
}
