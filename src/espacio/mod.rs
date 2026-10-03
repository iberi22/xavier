//! Space management — isolated WorkspaceState per Space (T-01)
//!
//! A Space is an isolated WorkspaceState with namespace `xavier://{space_id}/{appId}/{instanceId}`.
//! Maps to Telegram-like group: 1..n Xavier nodes, admin-controlled permissions.
//! Storage: `data/spaces/{space_id}/` via multi_db. Isolation guaranteed by WorkspaceRegistry.

pub mod channel;
pub mod context;
pub mod graph;
pub mod invite;
pub mod keys;
pub mod link;
pub mod manager;
pub mod marketplace;
pub mod p2p;
pub mod pack;
pub mod permissions;
pub mod public;
pub mod search;
pub mod store;
pub mod tokens;

pub use channel::{ChannelManager, ChannelMessage};
pub use context::{ContextBridge, ContextEntry, ContextKind};
pub use graph::{GraphEdge, GraphManager, GraphNode, GraphSnippet};
pub use invite::{InviteManager, SpaceInvite, SpaceRole};
pub use keys::{
    KeyHandle, KeyRing, KeysError, MasterNodeKek, NodeKek, RecoveryCode, StaticNodeKek, UnlockMode,
};
pub use link::{create_link, inbound_links, SpaceLink};
pub use manager::{CreateSpaceRequest, KeyOptions, SpaceError, SpaceInfo, SpaceManager};
pub use marketplace::{folder_dataset, list_folder_pack, query_folder_pack, FolderEntry};
pub use p2p::{ClosedNetwork, ClosedNetworkManager, EncryptedEnvelope};
pub use pack::{Pack, PackManifest, PackMemory};
pub use permissions::{can, SpaceAction, SpaceMembership};
pub use public::{PublicConnector, PublicPack};
pub use search::{search_over, visible_space_ids, RankedResult, SearchFilters};
pub use store::{validate_space_id, PlaintextCodec, RecordCodec, SpaceStore, SpaceStores};
pub use tokens::{SpaceAuth, SpaceContext, TokenError, TokenInfo};
