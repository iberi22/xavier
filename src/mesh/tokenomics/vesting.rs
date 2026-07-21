// SPDX-License-Identifier: MIT OR LICENSE-MESH
use crate::mesh::tokenomics::wallet::VestingState;
use chrono::{Datelike, Utc};

pub struct VestingEngine;

impl VestingEngine {
    /// Calculates the total releasable amount for a given vesting state.
    pub fn calculate_releasable(state: &VestingState) -> u64 {
        let now = Utc::now();
        let start = chrono::DateTime::from_timestamp(state.start_timestamp, 0)
            .unwrap_or_else(Utc::now);

        let months_passed =
            (now.year() - start.year()) * 12 + (now.month() as i32 - start.month() as i32);
        let months_passed = months_passed.max(0) as u32;

        let tier = state.tier;
        let release_50 = tier.release_50_month();
        let release_100 = tier.release_100_month();

        let total_eligible = if months_passed >= release_100 {
            state.amount_total
        } else if months_passed >= release_50 {
            state.amount_total / 2
        } else {
            0
        };

        total_eligible.saturating_sub(state.amount_released)
    }
}
