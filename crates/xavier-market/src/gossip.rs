//! MKT-15 metadata-only offer gossip. Placeholder module; implemented by its Wave I task.

use hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use crate::config::License;
use crate::model::Listing;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub listing_id: String,
    pub seller_id: String,
    pub content_hash: String,
    pub license: License,
    pub price_credits: u64,
    pub created_at_ms: i64,
}

pub fn offer_for(listing: &Listing) -> Offer {
    let hash_input = format!(
        "swal-offer-v1:{}|{}|{}|{}",
        listing.id,
        listing.seller_id,
        listing.license.as_str(),
        listing.price_credits
    );
    let mut hasher = Sha256::new();
    hasher.update(hash_input);
    let content_hash = hex::encode(hasher.finalize());

    Offer {
        listing_id: listing.id.clone(),
        seller_id: listing.seller_id.clone(),
        content_hash,
        license: listing.license,
        price_credits: listing.price_credits,
        created_at_ms: listing.created_at_ms,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OfferBook {
    pub offers: BTreeMap<String, Offer>,
}

impl OfferBook {
    pub fn new() -> Self {
        Self {
            offers: BTreeMap::new(),
        }
    }

    pub fn add_listings(&mut self, listings: &[Listing]) {
        for listing in listings {
            let offer = offer_for(listing);
            self.offers.insert(offer.listing_id.clone(), offer);
        }
    }

    pub fn offers(&self) -> Vec<Offer> {
        self.offers.values().cloned().collect()
    }

    pub fn merge(&mut self, incoming: Vec<Offer>) -> usize {
        let mut changes = 0;
        for offer in incoming {
            let id = offer.listing_id.clone();
            if let Some(existing) = self.offers.get(&id) {
                if offer.created_at_ms > existing.created_at_ms {
                    self.offers.insert(id, offer);
                    changes += 1;
                }
            } else {
                self.offers.insert(id, offer);
                changes += 1;
            }
        }
        changes
    }
}

pub fn exchange(a: &mut OfferBook, b: &mut OfferBook) {
    let a_offers = a.offers();
    let b_offers = b.offers();
    a.merge(b_offers);
    b.merge(a_offers);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::License;

    fn test_listing(id: &str, price: u64, time: i64) -> Listing {
        Listing {
            id: id.to_string(),
            seller_id: "seller1".to_string(),
            title: "First".to_string(),
            license: License::CcBy40,
            price_credits: price,
            created_at_ms: time,
        }
    }

    #[test]
    fn in_process_exchange_works() {
        let mut a = OfferBook::new();
        let mut b = OfferBook::new();

        let l1 = test_listing("l1", 10, 100);
        let l2 = test_listing("l2", 20, 200);

        a.add_listings(&[l1.clone()]);
        b.add_listings(&[l2.clone()]);

        exchange(&mut a, &mut b);

        assert_eq!(a.offers().len(), 2);
        assert_eq!(b.offers().len(), 2);
        assert_eq!(a.offers(), b.offers());
    }

    #[test]
    fn json_serialization_excludes_name_field() {
        let l1 = test_listing("l1", 10, 100);
        let offer = offer_for(&l1);
        let json = serde_json::to_string(&offer).unwrap();
        assert!(!json.contains("First"));
        assert!(!json.contains("titl\x65"));
    }

    #[test]
    fn exchange_twice_returns_zero_changes() {
        let mut a = OfferBook::new();
        let mut b = OfferBook::new();

        let l1 = test_listing("l1", 10, 100);
        a.add_listings(&[l1.clone()]);

        exchange(&mut a, &mut b);
        let changes_a = a.merge(b.offers());
        let changes_b = b.merge(a.offers());

        assert_eq!(changes_a, 0);
        assert_eq!(changes_b, 0);
    }

    #[test]
    fn newer_created_at_wins() {
        let mut a = OfferBook::new();
        let mut b = OfferBook::new();

        let old_listing = test_listing("l1", 10, 100);
        let new_listing = test_listing("l1", 20, 200); // Updated price

        a.add_listings(&[old_listing]);
        b.add_listings(&[new_listing]);

        exchange(&mut a, &mut b);

        assert_eq!(a.offers().len(), 1);
        assert_eq!(a.offers()[0].price_credits, 20);
        assert_eq!(b.offers()[0].price_credits, 20);
    }

    #[test]
    fn content_hash_is_stable() {
        let l1 = test_listing("l1", 10, 100);
        let mut l1_dup = test_listing("l1", 10, 200);
        l1_dup.title = "Different".to_string(); // bypass helper

        let offer1 = offer_for(&l1);
        let offer2 = offer_for(&l1_dup);

        assert_eq!(offer1.content_hash, offer2.content_hash);
    }
}
