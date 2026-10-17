//! Space management — one isolated store per Space (T-01)
//!
//! A Space is a user-meaningful workspace with namespace `xavier://{space_id}/{appId}/{instanceId}`
//! (built by [`SpaceManager::namespace_for`]). Maps to Telegram-like group: 1..n Xavier nodes,
//! admin-controlled permissions.
//! Storage: `{root}/spaces/{space_id}/` (layout in [`store`]). Isolation is per file: each space
//! owns its own `espacio.sqlite`, its own keystore and data key, and its own tokens. No table is
//! shared between spaces, and spaces do not go through `multi_db` or `WorkspaceRegistry`.

pub mod channel;
pub mod context;
pub mod graph;
pub mod invite;
pub mod keys;
pub mod link;
pub mod manager;
pub mod marketplace;
pub mod migrate;
pub mod p2p;
pub mod pack;
pub mod permissions;
pub mod public;
pub mod retention;
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
pub use link::{create_link, inbound_links, linked_fetch_limit, revoke_links_naming, SpaceLink};
pub use manager::{CreateSpaceRequest, KeyOptions, SpaceError, SpaceInfo, SpaceManager};
pub use marketplace::{folder_dataset, list_folder_pack, query_folder_pack, FolderEntry};
pub use migrate::{AdoptableDb, LegacySpace, LegacyWorkspace};
pub use p2p::{ClosedNetwork, ClosedNetworkManager, EncryptedEnvelope};
pub use pack::{Pack, PackManifest, PackMemory};
pub use permissions::{can, SpaceAction, SpaceMembership};
pub use public::{PublicConnector, PublicPack};
pub use search::{search_over, visible_space_ids, RankedResult, SearchFilters};
pub use store::{validate_space_id, PlaintextCodec, RecordCodec, SpaceStore, SpaceStores};
pub use tokens::{SpaceAuth, SpaceContext, TokenError, TokenInfo};
