//! Economy — Stable economy mechanics, bonding curve, and circuit breakers.

pub struct EconomyEngine {
    pub reserve_ratio: f64,                  // Target: 0.25 (25%)
    pub protocol_owned_liquidity_ratio: f64, // 0.20 (20%)
    pub burn_rate: f64,                      // 0.05 (5%)
    pub annual_inflation: f64,               // 0.02 (2% decreasing)
}

impl Default for EconomyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl EconomyEngine {
    /// New.
    pub fn new() -> Self {
        Self {
            reserve_ratio: 0.25,
            protocol_owned_liquidity_ratio: 0.20,
            burn_rate: 0.05,
            annual_inflation: 0.02,
        }
    }

    /// Calculates the purchase return based on an exponential smoothed bonding curve.
    /// Price = ReserveBalance / (Supply * ReserveRatio)
    pub fn calculate_purchase_return(
        &self,
        supply: f64,
        reserve_balance: f64,
        deposit_amount: f64,
    ) -> f64 {
        if self.reserve_ratio == 1.0 {
            return deposit_amount * (supply / reserve_balance);
        }

        // Bancor Formula: Supply * ((1 + Deposit / Reserve)^Ratio - 1)
        supply * ((1.0 + deposit_amount / reserve_balance).powf(self.reserve_ratio) - 1.0)
    }

    /// Evaluates if any circuit breakers should be triggered based on price drop.
    pub fn check_circuit_breakers(&self, price_drop_24h: f64) -> CircuitBreakerLevel {
        if price_drop_24h >= 0.40 {
            CircuitBreakerLevel::Level3
        } else if price_drop_24h >= 0.25 {
            CircuitBreakerLevel::Level2
        } else if price_drop_24h >= 0.15 {
            CircuitBreakerLevel::Level1
        } else {
            CircuitBreakerLevel::None
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum CircuitBreakerLevel {
    None,
    /// -15% Drop: 1h suspension
    Level1,
    /// -25% Drop: 6h suspension + increased burn
    Level2,
    /// -40% Drop: Total lockout, DAO intervention
    Level3,
}
