//! SQLite persistence for listings and receipts.
//!
//! Every mutating method takes the [`Actor`] that signed the request. Create,
//! update and revoke only succeed for the listing's own publisher. The check
//! runs inside the same transaction as the write, so no request can change a
//! listing it does not own.

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::auth::Actor;
use crate::config::{Currency, License, MarketConfig};
use crate::error::MarketError;
use crate::model::{Listing, ListingUpdate, NewListing, NewReceipt, Receipt};

/// Version written to `PRAGMA user_version` by this build.
pub const SCHEMA_VERSION: i64 = 4;

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

/// v2 adds the revocation timestamp. Rows written under v1 read as not revoked.
const SCHEMA_V2: &str = "ALTER TABLE market_listings ADD COLUMN revoked_at_ms INTEGER;";

/// MKT-17: validator registry, identity requests and signed validator votes.
/// Votes are append-only: the triggers refuse any update or delete.
const SCHEMA_V3: &str = "
CREATE TABLE IF NOT EXISTS ivn_validators (
    node_id             TEXT PRIMARY KEY,
    public_key          TEXT NOT NULL,
    registered_at_ms    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ivn_requests (
    id                  TEXT PRIMARY KEY,
    applicant           TEXT NOT NULL,
    status              TEXT NOT NULL CHECK (status IN ('pending', 'passed', 'rejected', 'quorum_not_met')),
    created_at_ms       INTEGER NOT NULL,
    updated_at_ms       INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ivn_request_validators (
    request_id          TEXT NOT NULL REFERENCES ivn_requests(id),
    validator_id        TEXT NOT NULL REFERENCES ivn_validators(node_id),
    PRIMARY KEY (request_id, validator_id)
);
CREATE TABLE IF NOT EXISTS ivn_votes (
    request_id          TEXT NOT NULL,
    validator_id        TEXT NOT NULL,
    decision            TEXT NOT NULL CHECK (decision IN ('check', 'reject', 'abstain')),
    signature           TEXT NOT NULL,
    cast_at_ms          INTEGER NOT NULL,
    PRIMARY KEY (request_id, validator_id),
    FOREIGN KEY (request_id, validator_id)
        REFERENCES ivn_request_validators (request_id, validator_id)
);
CREATE TRIGGER IF NOT EXISTS ivn_votes_no_update BEFORE UPDATE ON ivn_votes
BEGIN
    SELECT RAISE(ABORT, 'ivn votes are append-only');
END;
CREATE TRIGGER IF NOT EXISTS ivn_votes_no_delete BEFORE DELETE ON ivn_votes
BEGIN
    SELECT RAISE(ABORT, 'ivn votes are append-only');
END;
";

/// MKT-09: access grants with buyer-wrapped package keys, time limits and revocation.
const SCHEMA_V4: &str = "
CREATE TABLE IF NOT EXISTS market_access_grants (
    id                  TEXT PRIMARY KEY,
    listing_id          TEXT NOT NULL REFERENCES market_listings(id),
    buyer_id            TEXT NOT NULL,
    seller_id           TEXT NOT NULL,
    ephemeral_pubkey    TEXT NOT NULL,
    wrapped_key         TEXT NOT NULL,
    nonce               TEXT NOT NULL,
    created_at_ms       INTEGER NOT NULL,
    expires_at_ms       INTEGER NOT NULL,
    revoked_at_ms       INTEGER,
    signature           TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS market_access_grants_buyer ON market_access_grants (buyer_id);
CREATE INDEX IF NOT EXISTS market_access_grants_listing ON market_access_grants (listing_id);
";

/// Migration steps in order. Step `n` (zero-based) brings the schema to `n + 1`.
const MIGRATIONS: [&str; 4] = [SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4];

const _: () = assert!(MIGRATIONS.len() as i64 == SCHEMA_VERSION);
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
        let applied = usize::try_from(version)
            .map_err(|_| MarketError::Corrupt(format!("negative schema version {version}")))?;
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(applied) {
            conn.execute_batch(&format!(
                "BEGIN; {sql} PRAGMA user_version = {}; COMMIT;",
                index + 1
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

    pub(crate) fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite itself consistent, since
        // each write is a single transaction. Recover the guard instead of
        // poisoning every later request.
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn ensure_license_enabled(&self, license: License) -> Result<(), MarketError> {
        if self.config.licenses.contains(&license) {
            Ok(())
        } else {
            Err(MarketError::LicenseNotEnabled(license.as_str().to_string()))
        }
    }

    /// Creates a listing owned by `actor`. `new.seller_id` must be the actor's
    /// own id. The license must be enabled in `market.licenses`.
    pub fn create_listing(&self, actor: &Actor, new: NewListing) -> Result<Listing, MarketError> {
        if !new.seller_id.trim().eq_ignore_ascii_case(actor.id()) {
            return Err(MarketError::Forbidden(
                "seller_id must be the id of the key that signed the request".to_string(),
            ));
        }
        let title = non_empty("title", &new.title)?;
        self.ensure_license_enabled(new.license)?;
        check_credits(new.price_credits)?;

        if new.price_credits < self.config.price_floor_credits {
            return Err(MarketError::InvalidInput(format!(
                "price_credits {} is below the floor {}",
                new.price_credits, self.config.price_floor_credits
            )));
        }

        let listing = Listing {
            id: uuid::Uuid::new_v4().to_string(),
            seller_id: actor.id().to_string(),
            title,
            license: new.license,
            price_credits: new.price_credits,
            created_at_ms: now_ms(),
            revoked_at_ms: None,
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

    /// Changes the fields present in `update`. Only the listing's publisher may
    /// do this, and only while the listing is not revoked.
    pub fn update_listing(
        &self,
        actor: &Actor,
        id: &str,
        update: ListingUpdate,
    ) -> Result<Listing, MarketError> {
        if update == ListingUpdate::default() {
            return Err(MarketError::InvalidInput(
                "update must change at least one field".to_string(),
            ));
        }
        let title = update
            .title
            .as_deref()
            .map(|title| non_empty("title", title))
            .transpose()?;
        if let Some(license) = update.license {
            self.ensure_license_enabled(license)?;
        }
        if let Some(price) = update.price_credits {
            check_credits(price)?;
        }

        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let mut listing = owned_listing(&tx, id, actor)?;
        if listing.revoked_at_ms.is_some() {
            return Err(MarketError::ListingRevoked(id.to_string()));
        }
        if let Some(title) = title {
            listing.title = title;
        }
        if let Some(license) = update.license {
            listing.license = license;
        }
        if let Some(price) = update.price_credits {
            listing.price_credits = price;
        }
        tx.execute(
            "UPDATE market_listings SET title = ?2, license = ?3, price_credits = ?4 WHERE id = ?1",
            params![
                listing.id,
                listing.title,
                listing.license.as_str(),
                listing.price_credits as i64,
            ],
        )?;
        tx.commit()?;
        Ok(listing)
    }

    /// Revokes a listing the actor owns. Revoking again returns the listing
    /// unchanged, keeping the first revocation time.
    pub fn revoke_listing(&self, actor: &Actor, id: &str) -> Result<Listing, MarketError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let mut listing = owned_listing(&tx, id, actor)?;
        if listing.revoked_at_ms.is_none() {
            let revoked_at = now_ms();
            tx.execute(
                "UPDATE market_listings SET revoked_at_ms = ?2 WHERE id = ?1",
                params![listing.id, revoked_at],
            )?;
            listing.revoked_at_ms = Some(revoked_at);
        }
        tx.commit()?;
        Ok(listing)
    }

    pub fn get_listing(&self, id: &str) -> Result<Option<Listing>, MarketError> {
        query_listing(&self.conn(), id)
    }

    /// All listings, oldest first. Revoked listings are included with their
    /// `revoked_at_ms` set, so buyers can see why a listing is gone.
    pub fn list_listings(&self) -> Result<Vec<Listing>, MarketError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, seller_id, title, license, price_credits, created_at_ms, revoked_at_ms
             FROM market_listings ORDER BY created_at_ms, rowid",
        )?;
        let rows = stmt
            .query_map([], raw_listing)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(RawListing::into_listing).collect()
    }

    /// Records a purchase. The amount is the listing price at this moment and
    /// the currency is the one the store was opened with. Revoked listings
    /// cannot be bought.
    pub fn record_receipt(&self, new: NewReceipt) -> Result<Receipt, MarketError> {
        let buyer_id = non_empty("buyer_id", &new.buyer_id)?;
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let (price, revoked_at): (i64, Option<i64>) = tx
            .query_row(
                "SELECT price_credits, revoked_at_ms FROM market_listings WHERE id = ?1",
                params![new.listing_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| MarketError::ListingNotFound(new.listing_id.clone()))?;
        if revoked_at.is_some() {
            return Err(MarketError::ListingRevoked(new.listing_id));
        }
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

fn query_listing(conn: &Connection, id: &str) -> Result<Option<Listing>, MarketError> {
    let row = conn
        .query_row(
            "SELECT id, seller_id, title, license, price_credits, created_at_ms, revoked_at_ms
             FROM market_listings WHERE id = ?1",
            params![id],
            raw_listing,
        )
        .optional()?;
    row.map(RawListing::into_listing).transpose()
}

/// Loads a listing for a mutation and checks that `actor` owns it.
fn owned_listing(conn: &Connection, id: &str, actor: &Actor) -> Result<Listing, MarketError> {
    let listing =
        query_listing(conn, id)?.ok_or_else(|| MarketError::ListingNotFound(id.to_string()))?;
    if listing.seller_id != actor.id() {
        return Err(MarketError::Forbidden(format!(
            "listing {id} belongs to another publisher"
        )));
    }
    Ok(listing)
}

/// Row values before the enum fields are parsed back from text.
struct RawListing {
    id: String,
    seller_id: String,
    title: String,
    license: String,
    price_credits: i64,
    created_at_ms: i64,
    revoked_at_ms: Option<i64>,
}

fn raw_listing(row: &Row<'_>) -> rusqlite::Result<RawListing> {
    Ok(RawListing {
        id: row.get(0)?,
        seller_id: row.get(1)?,
        title: row.get(2)?,
        license: row.get(3)?,
        price_credits: row.get(4)?,
        created_at_ms: row.get(5)?,
        revoked_at_ms: row.get(6)?,
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
            revoked_at_ms: self.revoked_at_ms,
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

pub(crate) fn non_empty(field: &str, value: &str) -> Result<String, MarketError> {
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

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner_id() -> String {
        "ab".repeat(32)
    }

    fn owner() -> Actor {
        Actor::for_test(&owner_id())
    }

    fn other_identity() -> Actor {
        Actor::for_test(&"cd".repeat(32))
    }

    fn new_listing(price: u64) -> NewListing {
        NewListing {
            seller_id: owner_id(),
            title: "Weather station logs".to_string(),
            license: License::CcBy40,
            price_credits: price,
        }
    }

    fn update_price(price: u64) -> ListingUpdate {
        ListingUpdate {
            price_credits: Some(price),
            ..ListingUpdate::default()
        }
    }

    #[test]
    fn listing_survives_reopen_on_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("market.sqlite3");

        let created = {
            let store = MarketStore::open(&path, MarketConfig::default()).unwrap();
            store.create_listing(&owner(), new_listing(25)).unwrap()
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
            let listing = store.create_listing(&owner(), new_listing(40)).unwrap();
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
        let err = store.create_listing(&owner(), new_listing(1)).unwrap_err();
        assert!(matches!(err, MarketError::LicenseNotEnabled(ref l) if l == "CC-BY-4.0"));
        assert!(store.list_listings().unwrap().is_empty());
    }

    #[test]
    fn blank_title_and_oversized_price_are_refused() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let blank_title = NewListing {
            title: String::new(),
            ..new_listing(1)
        };
        assert!(matches!(
            store.create_listing(&owner(), blank_title),
            Err(MarketError::InvalidInput(_))
        ));
        assert!(matches!(
            store.create_listing(&owner(), new_listing(u64::MAX)),
            Err(MarketError::InvalidInput(_))
        ));
        assert!(store.list_listings().unwrap().is_empty());
    }

    #[test]
    fn create_refuses_a_seller_id_that_is_not_the_signer() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let impersonation = NewListing {
            seller_id: "seller-1".to_string(),
            ..new_listing(1)
        };
        assert!(matches!(
            store.create_listing(&other_identity(), impersonation),
            Err(MarketError::Forbidden(_))
        ));
        let other_key = NewListing {
            seller_id: owner_id(),
            ..new_listing(1)
        };
        assert!(matches!(
            store.create_listing(&other_identity(), other_key),
            Err(MarketError::Forbidden(_))
        ));
        assert!(store.list_listings().unwrap().is_empty());
    }

    #[test]
    fn create_accepts_the_signers_own_id_in_any_case() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let upper = NewListing {
            seller_id: owner_id().to_uppercase(),
            ..new_listing(1)
        };
        let listing = store.create_listing(&owner(), upper).unwrap();
        assert_eq!(listing.seller_id, owner_id());
    }

    #[test]
    fn another_identity_cannot_update_or_revoke_a_listing() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let created = store.create_listing(&owner(), new_listing(10)).unwrap();

        let err = store
            .update_listing(&other_identity(), &created.id, update_price(1))
            .unwrap_err();
        assert!(matches!(err, MarketError::Forbidden(_)));
        let err = store
            .revoke_listing(&other_identity(), &created.id)
            .unwrap_err();
        assert!(matches!(err, MarketError::Forbidden(_)));

        // Neither failed call changed the stored listing.
        assert_eq!(store.get_listing(&created.id).unwrap(), Some(created));
    }

    #[test]
    fn the_owner_can_update_and_revoke_its_listing() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let created = store.create_listing(&owner(), new_listing(10)).unwrap();

        let updated = store
            .update_listing(&owner(), &created.id, update_price(20))
            .unwrap();
        assert_eq!(updated.price_credits, 20);
        assert_eq!(updated.title, created.title);
        assert_eq!(updated.revoked_at_ms, None);

        let revoked = store.revoke_listing(&owner(), &created.id).unwrap();
        let first_revocation = revoked.revoked_at_ms;
        assert!(first_revocation.is_some());
        let again = store.revoke_listing(&owner(), &created.id).unwrap();
        assert_eq!(again.revoked_at_ms, first_revocation);
        assert_eq!(store.get_listing(&created.id).unwrap(), Some(again));
    }

    #[test]
    fn a_revoked_listing_cannot_change_or_be_bought() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let created = store.create_listing(&owner(), new_listing(10)).unwrap();
        store.revoke_listing(&owner(), &created.id).unwrap();

        assert!(matches!(
            store.update_listing(&owner(), &created.id, update_price(5)),
            Err(MarketError::ListingRevoked(_))
        ));
        let err = store
            .record_receipt(NewReceipt {
                listing_id: created.id.clone(),
                buyer_id: "buyer-1".to_string(),
            })
            .unwrap_err();
        assert!(matches!(err, MarketError::ListingRevoked(_)));
        assert!(store.receipts_for_buyer("buyer-1").unwrap().is_empty());
        // Revoked listings stay visible, marked as revoked.
        let listed = store.list_listings().unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].revoked_at_ms.is_some());
    }

    #[test]
    fn update_changes_only_the_fields_present() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let created = store.create_listing(&owner(), new_listing(10)).unwrap();
        let retitled = store
            .update_listing(
                &owner(),
                &created.id,
                ListingUpdate {
                    title: Some("  Renamed  ".to_string()),
                    ..ListingUpdate::default()
                },
            )
            .unwrap();
        assert_eq!(retitled.title, "Renamed");
        assert_eq!(retitled.price_credits, 10);
        assert_eq!(retitled.license, License::CcBy40);
    }

    #[test]
    fn empty_update_and_disabled_license_are_refused() {
        let config = MarketConfig {
            licenses: vec![License::CcByNc40],
            ..MarketConfig::default()
        };
        let store = MarketStore::open_in_memory(config).unwrap();
        let created = store
            .create_listing(
                &owner(),
                NewListing {
                    license: License::CcByNc40,
                    ..new_listing(10)
                },
            )
            .unwrap();
        assert!(matches!(
            store.update_listing(&owner(), &created.id, ListingUpdate::default()),
            Err(MarketError::InvalidInput(_))
        ));
        let relicensed = ListingUpdate {
            license: Some(License::CcBy40),
            ..ListingUpdate::default()
        };
        assert!(matches!(
            store.update_listing(&owner(), &created.id, relicensed),
            Err(MarketError::LicenseNotEnabled(_))
        ));
    }

    #[test]
    fn mutating_an_unknown_listing_is_not_found_for_everyone() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        assert!(matches!(
            store.update_listing(&owner(), "missing", update_price(1)),
            Err(MarketError::ListingNotFound(_))
        ));
        assert!(matches!(
            store.revoke_listing(&other_identity(), "missing"),
            Err(MarketError::ListingNotFound(_))
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
    fn a_v1_store_migrates_in_place_and_keeps_its_listings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("market.sqlite3");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(&format!(
                "{SCHEMA_V1}
                 INSERT INTO market_listings (id, seller_id, title, license, price_credits, created_at_ms)
                 VALUES ('legacy', '{}', 'Old listing', 'CC-BY-4.0', 7, 1);
                 PRAGMA user_version = 1;",
                owner_id()
            ))
            .unwrap();
        }

        let store = MarketStore::open(&path, MarketConfig::default()).unwrap();
        let legacy = store.get_listing("legacy").unwrap().unwrap();
        assert_eq!(legacy.price_credits, 7);
        assert_eq!(legacy.revoked_at_ms, None);
        assert!(store
            .revoke_listing(&owner(), "legacy")
            .unwrap()
            .revoked_at_ms
            .is_some());
        drop(store);

        let conn = Connection::open(&path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
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
        let listing = store.create_listing(&owner(), new_listing(50)).unwrap();
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
        let err = store.create_listing(&owner(), new_listing(49)).unwrap_err();
        assert!(matches!(err, MarketError::InvalidInput(_)));
        assert!(store.list_listings().unwrap().is_empty());
    }

    #[test]
    fn new_listing_with_unknown_fields_deserializes() {
        let json = format!(
            r#"{{
            "seller_id": "{}",
            "title": "Reputation dataset",
            "license": "CC-BY-4.0",
            "price_credits": 100,
            "reputation": 99
        }}"#,
            owner_id()
        );
        let new: NewListing = serde_json::from_str(&json).unwrap();
        assert_eq!(new.price_credits, 100);
        assert_eq!(new.seller_id, owner_id());

        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let listing = store.create_listing(&owner(), new).unwrap();
        assert_eq!(listing.price_credits, 100);
    }
}
