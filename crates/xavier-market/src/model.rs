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
    /// Set once the publisher revokes the listing. Revoked listings cannot be
    /// changed or bought, but their receipts stay.
    pub revoked_at_ms: Option<i64>,
}

/// Input for [`crate::MarketStore::create_listing`].
///
/// `seller_id` must be the publisher's own id, the hex public key that signed
/// the request. The store refuses any other value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewListing {
    pub seller_id: String,
    pub title: String,
    pub license: License,
    pub price_credits: u64,
}

/// Input for [`crate::MarketStore::update_listing`]. Absent fields are kept.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListingUpdate {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub license: Option<License>,
    #[serde(default)]
    pub price_credits: Option<u64>,
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
