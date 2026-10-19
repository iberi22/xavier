//! Marketplace v2: domain records and a persistent SQLite store for listings
//! and receipts.
//!
//! This crate has no dependency on the `xavier` library. The HTTP adapter lives
//! in `src/market_v2/` and sits behind the `market-v2` cargo feature and the
//! `XAVIER_MARKET_V2=beta` runtime flag.

pub mod admission;
pub mod audit_chain;
pub mod auth;
pub mod config;
pub mod consent;
pub mod credential;
mod error;
pub mod gossip;
pub mod grant;
pub mod ivn;
pub mod ledger;
pub mod merkle;
mod model;
pub mod pack;
pub mod ranking;
pub mod settlement;
pub mod signed_receipt;
mod store;
pub mod telemetry_gate;
pub mod terms;

pub use audit_chain::{AuditChain, AuditError, AuditRecord, ENTRY_KINDS, GENESIS_HASH};
pub use auth::{Actor, SignedHeaders, SignedRequest, Signer};
pub use config::{
    AnchorMode, ConfigError, Currency, LegalTextStatus, License, MarketConfig, Settlement,
    DEFAULT_GRANT_MAX_TTL_SECONDS, DEFAULT_GRANT_TTL_SECONDS, KEY_ANCHOR_MODE,
    KEY_GRANT_DEFAULT_TTL_SECONDS, KEY_GRANT_MAX_TTL_SECONDS,
};
pub use consent::{ConsentError, ConsentRecord, ConsentState, ConsentStore};
pub use error::MarketError;
pub use grant::{
    grant_signing_bytes, unwrap_key_as_buyer, wrap_key_for_buyer, AccessGrant, GrantError,
    ACCESS_GRANT_DOMAIN,
};
pub use ivn::{
    CastVote, IdentityRequest, NewIdentityRequest, NewValidator, RequestStatus, Validator, Vote,
    VoteDecision,
};
pub use model::{Listing, ListingUpdate, NewListing, NewReceipt, Receipt};
pub use pack::{
    export_namespace_pack, import_namespace_pack, pack_hash, read_pack, read_pack_info, write_pack,
    write_pack_with_namespace, NamespacePack, PackEntry, PackError, PACK_MAGIC,
};
pub use store::{MarketStore, SCHEMA_VERSION};
