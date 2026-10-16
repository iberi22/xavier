//! Marketplace v2: domain records and a persistent SQLite store for listings
//! and receipts.
//!
//! This crate has no dependency on the `xavier` library. The HTTP adapter lives
//! in `src/market_v2/` and sits behind the `market-v2` cargo feature and the
//! `XAVIER_MARKET_V2=beta` runtime flag.

pub mod config;
mod error;
mod model;
mod store;

pub use config::{ConfigError, Currency, LegalTextStatus, License, MarketConfig, Settlement};
pub use error::MarketError;
pub use model::{Listing, NewListing, NewReceipt, Receipt};
pub use store::{MarketStore, SCHEMA_VERSION};
