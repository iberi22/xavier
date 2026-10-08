//! Xavier billing.
//!
//! * [`plans`]: the canonical plan catalog v1 (prices and limits), vendored from
//!   `iberi22/xavier-cloud` → `plans/catalog.v1.json`.
//! * [`client`]: HTTP client for swal-billing (catalog + checkout) and
//!   xavier-cloud (`/v1/cloud/usage`), used by `xavier billing`.
//!
//! Payments are handled by swal-billing (Polar as merchant of record); Xavier
//! itself never talks to a payment provider or stores payment data.

pub mod client;
pub mod plans;
