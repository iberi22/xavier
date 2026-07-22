//! # Dataset Token Pricing Oracle
//!
//! Calculates dynamically-adjusted pricing for shared datasets in the SWAL/Xavier
//! network based on dataset size, freshness (age decay), and consumer demand.
//!
//! High demand increases prices, while stale or older data receives discounts.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a value in $SWAL/XAV tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TokenAmount(pub u64);

/// Represents the quality refinement level of a dataset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityLevel {
    /// Raw unverified technical logs / telemetry.
    Raw,
    /// Verified / structured datasets passing basic quality checks.
    Verified,
    /// High-quality annotated or premium training datasets.
    Gold,
}

impl QualityLevel {
    /// Returns the multiplier associated with this quality level.
    pub fn multiplier(&self) -> f64 {
        match self {
            QualityLevel::Raw => 1.0,
            QualityLevel::Verified => 1.5,
            QualityLevel::Gold => 2.5,
        }
    }
}

/// Metadata tracked for a dataset to compute dynamic prices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetMetadata {
    /// Unique identifier for the dataset.
    pub dataset_id: String,
    /// Size of the dataset (e.g., number of records or bytes).
    pub size: u64,
    /// Unix timestamp when the dataset was published.
    pub created_at: u64,
    /// Dynamic demand score (e.g., query/purchase counts, decaying over time).
    pub demand: f64,
    /// Unix timestamp of the last access/query.
    pub last_accessed: u64,
}

/// A pricing oracle that tracks dataset metrics and computes dynamic token pricing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceOracle {
    /// Tracked datasets.
    pub datasets: HashMap<String, DatasetMetadata>,
    /// Base token price per unit of size (e.g. per record/byte).
    pub base_price_per_unit: f64,
    /// Half-life decay for dataset age in seconds (e.g., 86400 for 1 day).
    pub decay_half_life: u64,
    /// Influence multiplier for demand on final price.
    pub demand_multiplier: f64,
    /// Unix timestamp of the last periodic update.
    pub last_update_time: u64,
    /// Periodic update interval in seconds.
    pub update_interval: u64,
}

impl PriceOracle {
    /// Creates a new PriceOracle instance.
    pub fn new(current_time: u64) -> Self {
        Self {
            datasets: HashMap::new(),
            base_price_per_unit: 0.01,
            decay_half_life: 86400, // 1 day decay
            demand_multiplier: 0.1,  // each query adds 10% premium
            last_update_time: current_time,
            update_interval: 3600,   // 1 hour periodic update
        }
    }

    /// Registers a new dataset with the oracle.
    pub fn register_dataset(&mut self, dataset_id: String, size: u64, current_time: u64) {
        let metadata = DatasetMetadata {
            dataset_id: dataset_id.clone(),
            size,
            created_at: current_time,
            demand: 0.0,
            last_accessed: current_time,
        };
        self.datasets.insert(dataset_id, metadata);
    }

    /// Records a query/access of a dataset, increasing its demand metric.
    pub fn record_query(&mut self, dataset_id: &str, current_time: u64) -> Result<(), String> {
        if let Some(metadata) = self.datasets.get_mut(dataset_id) {
            metadata.demand += 1.0;
            metadata.last_accessed = current_time;
            Ok(())
        } else {
            Err(format!("Dataset '{}' not found", dataset_id))
        }
    }

    /// Triggers a periodic update, decaying demand across all datasets over elapsed intervals.
    pub fn update_periodically(&mut self, current_time: u64) {
        if current_time > self.last_update_time {
            let elapsed = current_time - self.last_update_time;
            if elapsed >= self.update_interval {
                let intervals = elapsed / self.update_interval;
                // Decay demand by 5% per interval elapsed
                let decay_factor = 0.95_f64.powi(intervals as i32);
                for dataset in self.datasets.values_mut() {
                    dataset.demand *= decay_factor;
                }
                self.last_update_time = current_time;
            }
        }
    }

    /// Computes the dynamic price of a dataset.
    ///
    /// Pricing incorporates:
    /// - Size: base pricing scaled with size.
    /// - Freshness: age-based exponential decay.
    /// - Demand: premium added for popular datasets.
    /// - Quality level: constant multiplier for verified or premium states.
    pub fn get_price(&self, dataset_id: &str, quality_level: QualityLevel) -> Result<TokenAmount, String> {
        let metadata = self.datasets.get(dataset_id)
            .ok_or_else(|| format!("Dataset '{}' not found", dataset_id))?;

        // 1. Size base pricing
        let size_base = (metadata.size as f64) * self.base_price_per_unit;

        // 2. Freshness decay: 1 / (1 + (age / half_life))
        let age = if metadata.last_accessed >= metadata.created_at {
            metadata.last_accessed - metadata.created_at
        } else {
            0
        };
        let freshness_factor = 1.0 / (1.0 + (age as f64 / self.decay_half_life as f64));

        // 3. Demand multiplier: (1 + demand * demand_multiplier)
        let demand_factor = 1.0 + (metadata.demand * self.demand_multiplier);

        // 4. Quality multiplier
        let quality_multiplier = quality_level.multiplier();

        // Combined dynamic calculation
        let raw_price = size_base * freshness_factor * demand_factor * quality_multiplier;

        // Ensure a minimum floor price of 1 token
        let final_price = (raw_price.round() as u64).max(1);

        Ok(TokenAmount(final_price))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_increases_with_demand() {
        let mut oracle = PriceOracle::new(1000);
        let dataset_id = "test_dataset_demand".to_string();

        // Register a dataset of size 1000
        // Base price = 1000 * 0.01 = 10.0 tokens
        oracle.register_dataset(dataset_id.clone(), 1000, 1000);

        let initial_price = oracle.get_price(&dataset_id, QualityLevel::Raw).unwrap();
        assert_eq!(initial_price, TokenAmount(10));

        // Record a few queries/demands at same timestamp to isolate demand factor
        oracle.record_query(&dataset_id, 1000).unwrap();
        let price_after_1_query = oracle.get_price(&dataset_id, QualityLevel::Raw).unwrap();
        // demand = 1.0 -> demand_factor = 1.0 + (1.0 * 0.1) = 1.1 -> price = 10 * 1.1 = 11
        assert_eq!(price_after_1_query, TokenAmount(11));

        oracle.record_query(&dataset_id, 1000).unwrap();
        oracle.record_query(&dataset_id, 1000).unwrap();
        let price_after_3_queries = oracle.get_price(&dataset_id, QualityLevel::Raw).unwrap();
        // demand = 3.0 -> demand_factor = 1.0 + (3.0 * 0.1) = 1.3 -> price = 10 * 1.3 = 13
        assert_eq!(price_after_3_queries, TokenAmount(13));
        assert!(price_after_3_queries.0 > initial_price.0);
    }

    #[test]
    fn stale_data_cheaper() {
        let mut oracle = PriceOracle::new(1000);
        let dataset_id = "test_dataset_stale".to_string();

        // Register a dataset of size 1000
        oracle.register_dataset(dataset_id.clone(), 1000, 1000);

        let initial_price = oracle.get_price(&dataset_id, QualityLevel::Raw).unwrap();
        assert_eq!(initial_price, TokenAmount(10));

        // Let's simulate a large elapsed time between creation and access
        // If age = decay_half_life (86400 seconds), freshness_factor should be 0.5
        // Price should decay from 10 to 5 tokens.
        if let Some(metadata) = oracle.datasets.get_mut(&dataset_id) {
            metadata.last_accessed = 1000 + oracle.decay_half_life;
        }

        let stale_price = oracle.get_price(&dataset_id, QualityLevel::Raw).unwrap();
        assert_eq!(stale_price, TokenAmount(5));
        assert!(stale_price.0 < initial_price.0);
    }

    #[test]
    fn quality_multiplier_increases_price() {
        let mut oracle = PriceOracle::new(1000);
        let dataset_id = "test_dataset_quality".to_string();

        oracle.register_dataset(dataset_id.clone(), 1000, 1000);

        let raw_price = oracle.get_price(&dataset_id, QualityLevel::Raw).unwrap();
        let verified_price = oracle.get_price(&dataset_id, QualityLevel::Verified).unwrap();
        let gold_price = oracle.get_price(&dataset_id, QualityLevel::Gold).unwrap();

        assert_eq!(raw_price, TokenAmount(10));
        assert_eq!(verified_price, TokenAmount(15));
        assert_eq!(gold_price, TokenAmount(25));

        assert!(gold_price.0 > verified_price.0);
        assert!(verified_price.0 > raw_price.0);
    }

    #[test]
    fn test_periodic_update_demand_decay() {
        let mut oracle = PriceOracle::new(1000);
        let dataset_id = "test_dataset_periodic".to_string();

        oracle.register_dataset(dataset_id.clone(), 1000, 1000);
        oracle.record_query(&dataset_id, 1000).unwrap();
        oracle.record_query(&dataset_id, 1000).unwrap();

        let price_with_demand = oracle.get_price(&dataset_id, QualityLevel::Raw).unwrap();
        assert_eq!(price_with_demand, TokenAmount(12)); // demand = 2.0 -> 10 * 1.2 = 12

        // Trigger periodic update with 1 hour passing (update_interval = 3600)
        oracle.update_periodically(1000 + 3600);

        // Demand should decay by 5% (decay_factor = 0.95 -> 2.0 * 0.95 = 1.90)
        let dataset = oracle.datasets.get(&dataset_id).unwrap();
        assert_eq!(dataset.demand, 1.90);

        let price_after_decay = oracle.get_price(&dataset_id, QualityLevel::Raw).unwrap();
        assert!(price_after_decay.0 <= price_with_demand.0);
        // 10 * (1.0 + 1.90 * 0.1) = 11.90 -> rounds to 12. Let's decay more intervals
        oracle.update_periodically(1000 + 3600 + 3600 * 5); // 5 more hours passing
        let dataset_more_decay = oracle.datasets.get(&dataset_id).unwrap();
        // demand decay: 1.90 * 0.95^5 = 1.90 * 0.77378 = 1.47
        assert!(dataset_more_decay.demand < 1.5);

        let price_after_more_decay = oracle.get_price(&dataset_id, QualityLevel::Raw).unwrap();
        assert!(price_after_more_decay.0 < price_with_demand.0);
    }
}
