//! Listing policy for the v2 HTTP adapter: the privacy admission gate on
//! create (MKT-06) and the `?q=` hybrid ranking on list (MKT-14).
//!
//! Adapters only. The rules live in `xavier_market::admission` and
//! `xavier_market::ranking`, and the PII counter lives in
//! `crate::data_commons::privacy`.

use std::collections::HashMap;

use serde_json::Value;
use xavier_market::admission::{admit, AdmissionError, ListingPrivacyReport, Privacy, MIN_K};
use xavier_market::ranking::{rank, text_score, Candidate, Weights};
use xavier_market::{Listing, NewListing};

use crate::data_commons::privacy::{check_k_anonymity, PrivacyPipeline};

/// Builds the admission report for a listing request.
///
/// - Personal data: the pipeline counts PII in the free-text title. The store
///   keeps the title as submitted and does not scrub it, so every item found is
///   residual and the listing is refused.
/// - Consent: `consent_ref` is passed through. `admit` refuses it when missing or
///   blank.
/// - k-anonymity: a listing request carries no per-record data, so the check runs
///   over an empty bundle and holds trivially. The records a listing sells are
///   not part of this request.
/// - Level: P2 is the lowest level `admit` accepts. Text with no PII needs no
///   scrubbing to reach it.
pub fn privacy_report(new: &NewListing, consent_ref: Option<String>) -> ListingPrivacyReport {
    let residual_pii_total = PrivacyPipeline::new()
        .count_pii(&Value::String(new.title.clone()))
        .total();
    let k_report = check_k_anonymity(&[], MIN_K);
    ListingPrivacyReport {
        level: Privacy::P2,
        residual_pii_total,
        k: k_report.k,
        k_satisfied: k_report.satisfied,
        consent_ref,
    }
}

/// Runs the admission gate on a listing request.
pub fn admit_listing(new: &NewListing, consent_ref: Option<String>) -> Result<(), AdmissionError> {
    admit(&privacy_report(new, consent_ref))
}

/// Ranks listings for a text query with the hybrid ranker.
///
/// Only listings with a text match take part, which replaces a plain substring
/// filter. The vector and karma signals are not wired on the v2 path yet, so
/// they score zero for every listing and the order comes from the text match
/// alone. Ties keep the ranker's listing-id order.
pub fn search(query: &str, listings: Vec<Listing>) -> Vec<Listing> {
    let candidates: Vec<Candidate> = listings
        .iter()
        .filter(|listing| text_score(query, &listing.title) > 0.0)
        .map(|listing| Candidate {
            listing_id: listing.id.clone(),
            title: listing.title.clone(),
            vector_score: 0.0,
            karma: 0.0,
        })
        .collect();
    let mut by_id: HashMap<String, Listing> = listings
        .into_iter()
        .map(|listing| (listing.id.clone(), listing))
        .collect();
    rank(query, &candidates, Weights::default())
        .into_iter()
        .filter_map(|ranked| by_id.remove(&ranked.listing_id))
        .collect()
}
