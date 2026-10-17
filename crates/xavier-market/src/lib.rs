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
pub mod ledger;
pub mod merkle;
mod model;
pub mod pack;
pub mod ranking;
pub mod signed_receipt;
mod store;
pub mod telemetry_gate;
pub mod terms;

pub use auth::{Actor, SignedHeaders, SignedRequest, Signer};
pub use config::{ConfigError, Currency, LegalTextStatus, License, MarketConfig, Settlement};
pub use error::MarketError;
pub use model::{Listing, ListingUpdate, NewListing, NewReceipt, Receipt};
pub use pack::{
    export_namespace_pack, import_namespace_pack, pack_hash, read_pack, read_pack_info, write_pack,
    write_pack_with_namespace, NamespacePack, PackEntry, PackError,
};
pub use store::{MarketStore, SCHEMA_VERSION};
