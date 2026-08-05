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
            demand_multiplier: 0.1, // each query adds 10% premium
            last_update_time: current_time,
            update_interval: 3600, // 1 hour periodic update
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
    pub fn get_price(
        &self,
        dataset_id: &str,
        quality_level: QualityLevel,
    ) -> Result<TokenAmount, String> {
        let metadata = self
            .datasets
            .get(dataset_id)
            .ok_or_else(|| format!("Dataset '{}' not found", dataset_id))?;

        // 1. Size base pricing
        let size_base = (metadata.size as f64) * self.base_price_per_unit;

        // 2. Freshness decay: 1 / (1 + (age / half_life))
        let age = metadata.last_accessed.saturating_sub(metadata.created_at);
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

/// Represents the tier structure for Data Commons marketplace pricing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PricingTier {
    /// Zero price for access
    Free,
    /// Base tier for standard collaborators
    Colaborador,
    /// Premium tier with advanced features or higher guarantees
    #[serde(rename = "Colaborador+")]
    ColaboradorPlus,
}

/// Calculates the token price for a dataset based on its size, pricing tier, and provider reputation.
///
/// Reputation is expected to be in the range `[-1.0, 1.0]`. If it is outside this range,
/// it will be clamped for safety.
///
/// Formulas:
/// - Free: 0 tokens
/// - Colaborador: base price of 0.10 tokens per unit of size, scaled by `1.0 + max(0.0, reputation)`
/// - Colaborador+: base price of 0.25 tokens per unit of size, scaled by `1.0 + max(0.0, reputation) * 1.5`
pub fn calculate_price(size: u64, tier: PricingTier, reputation: f64) -> TokenAmount {
    let reputation = reputation.clamp(-1.0, 1.0);
    match tier {
        PricingTier::Free => TokenAmount(0),
        PricingTier::Colaborador => {
            let base_rate = 0.10;
            let reputation_factor = 1.0 + reputation.max(0.0);
            let raw_price = size as f64 * base_rate * reputation_factor;
            // Minimum price of 1 token for non-free tiers if size > 0
            let final_price = if size > 0 {
                (raw_price.round() as u64).max(1)
            } else {
                0
            };
            TokenAmount(final_price)
        }
        PricingTier::ColaboradorPlus => {
            let base_rate = 0.25;
            let reputation_factor = 1.0 + reputation.max(0.0) * 1.5;
            let raw_price = size as f64 * base_rate * reputation_factor;
            // Minimum price of 1 token for non-free tiers if size > 0
            let final_price = if size > 0 {
                (raw_price.round() as u64).max(1)
            } else {
                0
            };
            TokenAmount(final_price)
        }
    }
}

/// Calculates the reputation boost for a provider based on their staked $SWAL amount.
/// Staking provides a reputation boost up to a maximum boost of +0.50.
pub fn calculate_reputation_boost(staked_amount: u64) -> f64 {
    // 1000 staked tokens yields maximum boost of 0.50 (linear scaling up to 1000)

    (staked_amount as f64 / 2000.0).min(0.50)
}

/// Applies a reputation boost from staked $SWAL to a provider's base reputation.
/// Clamps the final reputation in the range `[-1.0, 1.0]`.
pub fn boost_reputation(base_reputation: f64, staked_amount: u64) -> f64 {
    let boost = calculate_reputation_boost(staked_amount);
    (base_reputation + boost).clamp(-1.0, 1.0)
}

/// Computes the revenue split for a sale.
/// The provider receives 90% of the price, and the platform receives 10%.
///
/// To prevent rounding errors from losing tokens, if price > 0,
/// the platform receives at least 1 token, and the provider receives the rest.
pub fn calculate_revenue_share(price: TokenAmount) -> (TokenAmount, TokenAmount) {
    if price.0 == 0 {
        return (TokenAmount(0), TokenAmount(0));
    }
    let platform = (price.0 as f64 * 0.10).round() as u64;
    let platform = platform.clamp(1, price.0);
    let provider = price.0 - platform;
    (TokenAmount(provider), TokenAmount(platform))
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
        let verified_price = oracle
            .get_price(&dataset_id, QualityLevel::Verified)
            .unwrap();
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

    #[test]
    fn test_pricing_tier_serialization() {
        let free = PricingTier::Free;
        let colaborador = PricingTier::Colaborador;
        let plus = PricingTier::ColaboradorPlus;

        let s_free = serde_json::to_string(&free).unwrap();
        let s_colaborador = serde_json::to_string(&colaborador).unwrap();
        let s_plus = serde_json::to_string(&plus).unwrap();

        assert_eq!(s_free, "\"Free\"");
        assert_eq!(s_colaborador, "\"Colaborador\"");
        assert_eq!(s_plus, "\"Colaborador+\"");

        let d_free: PricingTier = serde_json::from_str("\"Free\"").unwrap();
        let d_colaborador: PricingTier = serde_json::from_str("\"Colaborador\"").unwrap();
        let d_plus: PricingTier = serde_json::from_str("\"Colaborador+\"").unwrap();

        assert_eq!(d_free, PricingTier::Free);
        assert_eq!(d_colaborador, PricingTier::Colaborador);
        assert_eq!(d_plus, PricingTier::ColaboradorPlus);
    }

    #[test]
    fn test_calculate_price_scenarios() {
        // Free tier is always 0
        assert_eq!(calculate_price(100, PricingTier::Free, 0.5), TokenAmount(0));
        assert_eq!(
            calculate_price(100, PricingTier::Free, -0.5),
            TokenAmount(0)
        );

        // Colaborador: base rate 0.10. size 100 -> base 10.0
        // Rep 0.0 -> factor 1.0 -> 10 tokens
        assert_eq!(
            calculate_price(100, PricingTier::Colaborador, 0.0),
            TokenAmount(10)
        );
        // Rep 1.0 -> factor 2.0 -> 20 tokens
        assert_eq!(
            calculate_price(100, PricingTier::Colaborador, 1.0),
            TokenAmount(20)
        );
        // Rep -0.5 -> max(0, -0.5) = 0 -> factor 1.0 -> 10 tokens
        assert_eq!(
            calculate_price(100, PricingTier::Colaborador, -0.5),
            TokenAmount(10)
        );

        // Colaborador+: base rate 0.25. size 100 -> base 25.0
        // Rep 0.0 -> factor 1.0 -> 25 tokens
        assert_eq!(
            calculate_price(100, PricingTier::ColaboradorPlus, 0.0),
            TokenAmount(25)
        );
        // Rep 1.0 -> factor 1.0 + 1.0 * 1.5 = 2.5 -> 25.0 * 2.5 = 62.5 -> rounds to 63 tokens
        assert_eq!(
            calculate_price(100, PricingTier::ColaboradorPlus, 1.0),
            TokenAmount(63)
        );

        // Size 0 yields 0 tokens
        assert_eq!(
            calculate_price(0, PricingTier::Colaborador, 1.0),
            TokenAmount(0)
        );
    }

    #[test]
    fn test_reputation_boosts() {
        // Boost is capped at 0.50
        assert_eq!(calculate_reputation_boost(0), 0.0);
        assert_eq!(calculate_reputation_boost(500), 0.25);
        assert_eq!(calculate_reputation_boost(1000), 0.50);
        assert_eq!(calculate_reputation_boost(5000), 0.50); // capped

        // Base reputation = 0.20, staked = 500 (+0.25 boost) -> 0.45
        let boosted = boost_reputation(0.20, 500);
        assert!((boosted - 0.45).abs() < 1e-6);

        // Clamping to max 1.0
        let boosted_max = boost_reputation(0.80, 1000); // 0.80 + 0.50 = 1.30 -> capped to 1.0
        assert_eq!(boosted_max, 1.0);

        // Clamping to min -1.0
        let boosted_min = boost_reputation(-1.5, 0); // -1.5 -> capped to -1.0
        assert_eq!(boosted_min, -1.0);
    }

    #[test]
    fn test_revenue_share_splits() {
        // Sale of 0 tokens
        let (prov, plat) = calculate_revenue_share(TokenAmount(0));
        assert_eq!(prov, TokenAmount(0));
        assert_eq!(plat, TokenAmount(0));

        // Sale of 1 token
        // Platform gets 1 token, provider gets 0
        let (prov, plat) = calculate_revenue_share(TokenAmount(1));
        assert_eq!(plat, TokenAmount(1));
        assert_eq!(prov, TokenAmount(0));

        // Sale of 10 tokens
        // 10% platform = 1 token, 90% provider = 9 tokens
        let (prov, plat) = calculate_revenue_share(TokenAmount(10));
        assert_eq!(plat, TokenAmount(1));
        assert_eq!(prov, TokenAmount(9));

        // Sale of 100 tokens
        // 10% platform = 10 tokens, 90% provider = 90 tokens
        let (prov, plat) = calculate_revenue_share(TokenAmount(100));
        assert_eq!(plat, TokenAmount(10));
        assert_eq!(prov, TokenAmount(90));

        // Sale of 45 tokens
        // 10% platform = 4.5 -> rounds to 5 tokens, 90% provider = 40 tokens
        let (prov, plat) = calculate_revenue_share(TokenAmount(45));
        assert_eq!(plat, TokenAmount(5));
        assert_eq!(prov, TokenAmount(40));
        assert_eq!(prov.0 + plat.0, 45);
    }
}
