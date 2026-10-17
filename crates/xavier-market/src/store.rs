//! SQLite persistence for listings and receipts.

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::config::{Currency, License, MarketConfig};
use crate::error::MarketError;
use crate::model::{Listing, NewListing, NewReceipt, Receipt};

/// Version written to `PRAGMA user_version` by this build.
pub const SCHEMA_VERSION: i64 = 1;

const SCHEMA_V1: &str = "
CREATE TABLE IF NOT EXISTS market_listings (
    id              TEXT PRIMARY KEY,
    seller_id       TEXT NOT NULL,
    title           TEXT NOT NULL,
    license         TEXT NOT NULL,
    price_credits   INTEGER NOT NULL CHECK (price_credits >= 0),
    created_at_ms   INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS market_receipts (
    id              TEXT PRIMARY KEY,
    listing_id      TEXT NOT NULL REFERENCES market_listings(id),
    buyer_id        TEXT NOT NULL,
    amount_credits  INTEGER NOT NULL CHECK (amount_credits >= 0),
    currency        TEXT NOT NULL,
    created_at_ms   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS market_receipts_buyer ON market_receipts (buyer_id);
";

/// Durable marketplace store. Every write is committed before the call returns,
/// so a listing survives a process restart when the store is reopened on the
/// same file.
pub struct MarketStore {
    conn: Mutex<Connection>,
    config: MarketConfig,
}

impl MarketStore {
    /// Opens (or creates) the store at `path`. The config is validated first.
    pub fn open(path: &Path, config: MarketConfig) -> Result<Self, MarketError> {
        config.validate()?;
        let conn = Connection::open(path)?;
        Self::init(conn, config)
    }

    /// An in-memory store with the same schema and rules. For tests.
    pub fn open_in_memory(config: MarketConfig) -> Result<Self, MarketError> {
        config.validate()?;
        Self::init(Connection::open_in_memory()?, config)
    }

    fn init(conn: Connection, config: MarketConfig) -> Result<Self, MarketError> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(MarketError::SchemaTooNew(version));
        }
        if version < SCHEMA_VERSION {
            conn.execute_batch(&format!(
                "BEGIN; {SCHEMA_V1} PRAGMA user_version = {SCHEMA_VERSION}; COMMIT;"
            ))?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
            config,
        })
    }

    pub fn config(&self) -> &MarketConfig {
        &self.config
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite itself consistent, since
        // each write is a single transaction. Recover the guard instead of
        // poisoning every later request.
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Creates a listing. The license must be enabled in `market.licenses`.
    pub fn create_listing(&self, new: NewListing) -> Result<Listing, MarketError> {
        let seller_id = non_empty("seller_id", &new.seller_id)?;
        let title = non_empty("title", &new.title)?;
        if !self.config.licenses.contains(&new.license) {
            return Err(MarketError::LicenseNotEnabled(
                new.license.as_str().to_string(),
            ));
        }
        check_credits(new.price_credits)?;

        if new.price_credits < self.config.price_floor_credits {
            return Err(MarketError::InvalidInput(format!(
                "price_credits {} is below the floor {}",
                new.price_credits, self.config.price_floor_credits
            )));
        }

        let listing = Listing {
            id: uuid::Uuid::new_v4().to_string(),
            seller_id,
            title,
            license: new.license,
            price_credits: new.price_credits,
            created_at_ms: now_ms(),
        };
        self.conn().execute(
            "INSERT INTO market_listings (id, seller_id, title, license, price_credits, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                listing.id,
                listing.seller_id,
                listing.title,
                listing.license.as_str(),
                listing.price_credits as i64,
                listing.created_at_ms,
            ],
        )?;
        Ok(listing)
    }

    pub fn get_listing(&self, id: &str) -> Result<Option<Listing>, MarketError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT id, seller_id, title, license, price_credits, created_at_ms
                 FROM market_listings WHERE id = ?1",
                params![id],
                raw_listing,
            )
            .optional()?;
        row.map(RawListing::into_listing).transpose()
    }

    /// All listings, oldest first.
    pub fn list_listings(&self) -> Result<Vec<Listing>, MarketError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, seller_id, title, license, price_credits, created_at_ms
             FROM market_listings ORDER BY created_at_ms, rowid",
        )?;
        let rows = stmt
            .query_map([], raw_listing)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(RawListing::into_listing).collect()
    }

    /// Records a purchase. The amount is the listing price at this moment and
    /// the currency is the one the store was opened with.
    pub fn record_receipt(&self, new: NewReceipt) -> Result<Receipt, MarketError> {
        let buyer_id = non_empty("buyer_id", &new.buyer_id)?;
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let price: i64 = tx
            .query_row(
                "SELECT price_credits FROM market_listings WHERE id = ?1",
                params![new.listing_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| MarketError::ListingNotFound(new.listing_id.clone()))?;
        let receipt = Receipt {
            id: uuid::Uuid::new_v4().to_string(),
            listing_id: new.listing_id,
            buyer_id,
            amount_credits: price as u64,
            currency: self.config.currency,
            created_at_ms: now_ms(),
        };
        tx.execute(
            "INSERT INTO market_receipts (id, listing_id, buyer_id, amount_credits, currency, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                receipt.id,
                receipt.listing_id,
                receipt.buyer_id,
                receipt.amount_credits as i64,
                receipt.currency.as_str(),
                receipt.created_at_ms,
            ],
        )?;
        tx.commit()?;
        Ok(receipt)
    }

    /// Receipts for one buyer, oldest first.
    pub fn receipts_for_buyer(&self, buyer_id: &str) -> Result<Vec<Receipt>, MarketError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, listing_id, buyer_id, amount_credits, currency, created_at_ms
             FROM market_receipts WHERE buyer_id = ?1 ORDER BY created_at_ms, rowid",
        )?;
        let rows = stmt
            .query_map(params![buyer_id], raw_receipt)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(RawReceipt::into_receipt).collect()
    }
}

/// Row values before the enum fields are parsed back from text.
struct RawListing {
    id: String,
    seller_id: String,
    title: String,
    license: String,
    price_credits: i64,
    created_at_ms: i64,
}

fn raw_listing(row: &Row<'_>) -> rusqlite::Result<RawListing> {
    Ok(RawListing {
        id: row.get(0)?,
        seller_id: row.get(1)?,
        title: row.get(2)?,
        license: row.get(3)?,
        price_credits: row.get(4)?,
        created_at_ms: row.get(5)?,
    })
}

impl RawListing {
    fn into_listing(self) -> Result<Listing, MarketError> {
        let license = License::parse(&self.license)
            .map_err(|err| MarketError::Corrupt(format!("listing {}: {err}", self.id)))?;
        Ok(Listing {
            id: self.id,
            seller_id: self.seller_id,
            title: self.title,
            license,
            price_credits: self.price_credits as u64,
            created_at_ms: self.created_at_ms,
        })
    }
}

struct RawReceipt {
    id: String,
    listing_id: String,
    buyer_id: String,
    amount_credits: i64,
    currency: String,
    created_at_ms: i64,
}

fn raw_receipt(row: &Row<'_>) -> rusqlite::Result<RawReceipt> {
    Ok(RawReceipt {
        id: row.get(0)?,
        listing_id: row.get(1)?,
        buyer_id: row.get(2)?,
        amount_credits: row.get(3)?,
        currency: row.get(4)?,
        created_at_ms: row.get(5)?,
    })
}

impl RawReceipt {
    fn into_receipt(self) -> Result<Receipt, MarketError> {
        let currency = match self.currency.as_str() {
            "internal_credits" => Currency::InternalCredits,
            "usd_polar" => Currency::UsdPolar,
            "swal_token" => Currency::SwalToken,
            other => {
                return Err(MarketError::Corrupt(format!(
                    "receipt {}: unknown currency {other:?}",
                    self.id
                )))
            }
        };
        Ok(Receipt {
            id: self.id,
            listing_id: self.listing_id,
            buyer_id: self.buyer_id,
            amount_credits: self.amount_credits as u64,
            currency,
            created_at_ms: self.created_at_ms,
        })
    }
}

fn non_empty(field: &str, value: &str) -> Result<String, MarketError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(MarketError::InvalidInput(format!(
            "{field} must not be empty"
        )));
    }
    Ok(trimmed.to_string())
}

/// SQLite stores signed 64-bit integers, so larger prices are refused.
fn check_credits(price: u64) -> Result<(), MarketError> {
    if price > i64::MAX as u64 {
        return Err(MarketError::InvalidInput(
            "price_credits exceeds the storable range".to_string(),
        ));
    }
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_listing(price: u64) -> NewListing {
        NewListing {
            seller_id: "seller-1".to_string(),
            title: "Weather station logs".to_string(),
            license: License::CcBy40,
            price_credits: price,
        }
    }

    #[test]
    fn listing_survives_reopen_on_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("market.sqlite3");

        let created = {
            let store = MarketStore::open(&path, MarketConfig::default()).unwrap();
            store.create_listing(new_listing(25)).unwrap()
        };

        let reopened = MarketStore::open(&path, MarketConfig::default()).unwrap();
        assert_eq!(
            reopened.get_listing(&created.id).unwrap(),
            Some(created.clone())
        );
        assert_eq!(reopened.list_listings().unwrap(), vec![created]);
    }

    #[test]
    fn receipt_survives_reopen_and_copies_the_price_at_purchase() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("market.sqlite3");
        let listing_id = {
            let store = MarketStore::open(&path, MarketConfig::default()).unwrap();
            let listing = store.create_listing(new_listing(40)).unwrap();
            store
                .record_receipt(NewReceipt {
                    listing_id: listing.id.clone(),
                    buyer_id: "buyer-1".to_string(),
                })
                .unwrap();
            listing.id
        };

        let reopened = MarketStore::open(&path, MarketConfig::default()).unwrap();
        let receipts = reopened.receipts_for_buyer("buyer-1").unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].listing_id, listing_id);
        assert_eq!(receipts[0].amount_credits, 40);
        assert_eq!(receipts[0].currency, Currency::InternalCredits);
        assert!(reopened
            .receipts_for_buyer("someone-else")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn unknown_listing_id_is_absent_not_an_error() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        assert_eq!(store.get_listing("missing").unwrap(), None);
    }

    #[test]
    fn license_outside_the_configured_set_is_refused() {
        let config = MarketConfig {
            licenses: vec![License::CcByNc40],
            ..MarketConfig::default()
        };
        let store = MarketStore::open_in_memory(config).unwrap();
        let err = store.create_listing(new_listing(1)).unwrap_err();
        assert!(matches!(err, MarketError::LicenseNotEnabled(ref l) if l == "CC-BY-4.0"));
        assert!(store.list_listings().unwrap().is_empty());
    }

    #[test]
    fn blank_seller_title_and_oversized_price_are_refused() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let blank_seller = NewListing {
            seller_id: "  ".to_string(),
            ..new_listing(1)
        };
        assert!(matches!(
            store.create_listing(blank_seller),
            Err(MarketError::InvalidInput(_))
        ));
        let blank_title = NewListing {
            title: String::new(),
            ..new_listing(1)
        };
        assert!(matches!(
            store.create_listing(blank_title),
            Err(MarketError::InvalidInput(_))
        ));
        assert!(matches!(
            store.create_listing(new_listing(u64::MAX)),
            Err(MarketError::InvalidInput(_))
        ));
    }

    #[test]
    fn receipt_for_missing_listing_is_refused() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let err = store
            .record_receipt(NewReceipt {
                listing_id: "nope".to_string(),
                buyer_id: "buyer-1".to_string(),
            })
            .unwrap_err();
        assert!(matches!(err, MarketError::ListingNotFound(ref id) if id == "nope"));
    }

    #[test]
    fn store_refuses_a_schema_newer_than_this_build() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("market.sqlite3");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("PRAGMA user_version = 99;").unwrap();
        }
        assert!(matches!(
            MarketStore::open(&path, MarketConfig::default()),
            Err(MarketError::SchemaTooNew(99))
        ));
    }

    #[test]
    fn invalid_config_never_opens_a_store() {
        let config = MarketConfig {
            settlement: crate::config::Settlement::OnChain,
            ..MarketConfig::default()
        };
        assert!(matches!(
            MarketStore::open_in_memory(config),
            Err(MarketError::Config(_))
        ));
    }

    #[test]
    fn ask_equal_to_the_floor_is_stored_unchanged() {
        let config = MarketConfig {
            price_floor_credits: 50,
            ..MarketConfig::default()
        };
        let store = MarketStore::open_in_memory(config).unwrap();
        let listing = store.create_listing(new_listing(50)).unwrap();
        assert_eq!(listing.price_credits, 50);
        let listed = store.list_listings().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].price_credits, 50);
    }

    #[test]
    fn ask_below_the_floor_is_refused() {
        let config = MarketConfig {
            price_floor_credits: 50,
            ..MarketConfig::default()
        };
        let store = MarketStore::open_in_memory(config).unwrap();
        let err = store.create_listing(new_listing(49)).unwrap_err();
        assert!(matches!(err, MarketError::InvalidInput(_)));
        assert!(store.list_listings().unwrap().is_empty());
    }

    #[test]
    fn new_listing_with_unknown_fields_deserializes() {
        let json = r#"{
            "seller_id": "seller-99",
            "title": "Reputation dataset",
            "license": "CC-BY-4.0",
            "price_credits": 100,
            "reputation": 99
        }"#;
        let new: NewListing = serde_json::from_str(json).unwrap();
        assert_eq!(new.price_credits, 100);
        assert_eq!(new.seller_id, "seller-99");

        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let listing = store.create_listing(new).unwrap();
        assert_eq!(listing.price_credits, 100);
    }
}
