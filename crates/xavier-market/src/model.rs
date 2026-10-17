//! Listing and receipt records as the store returns them.

use serde::{Deserialize, Serialize};

use crate::config::{Currency, License};

/// A dataset offered for internal credits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    pub id: String,
    pub seller_id: String,
    pub title: String,
    pub license: License,
    pub price_credits: u64,
    pub created_at_ms: i64,
}

/// Input for [`crate::MarketStore::create_listing`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewListing {
    pub seller_id: String,
    pub title: String,
    pub license: License,
    pub price_credits: u64,
}

/// Proof that a buyer bought a listing. The amount is copied from the listing
/// at purchase time, so later price changes do not rewrite history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub id: String,
    pub listing_id: String,
    pub buyer_id: String,
    pub amount_credits: u64,
    pub currency: Currency,
    pub created_at_ms: i64,
}

/// Input for [`crate::MarketStore::record_receipt`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewReceipt {
    pub listing_id: String,
    pub buyer_id: String,
}
