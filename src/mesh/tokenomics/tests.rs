#[cfg(test)]
mod tokenomics_tests {
    use crate::mesh::node::NodeId;
    use crate::mesh::tokenomics::economy::{CircuitBreakerLevel, EconomyEngine};
    use crate::mesh::tokenomics::rewards::{ContributionType, RewardEngine};
    use crate::mesh::tokenomics::vesting::VestingEngine;
    use crate::mesh::tokenomics::wallet::{InvestmentTier, VestingState, Wallet};
    use chrono::{Duration, Utc};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_progressive_apy_rewards() {
        let node_id = NodeId::parse("xv1-testprogressive001").unwrap();
        let wallet = Arc::new(Mutex::new(Wallet::new(node_id)));
        let engine = RewardEngine::new(wallet.clone());

        // Base Tier (5% APY -> 1.0x multiplier)
        let contrib = ContributionType::DataValidated { records: 10 };
        let reward_base = engine.calculate_reward(&contrib, InvestmentTier::Base);
        assert_eq!(reward_base, 50);

        // Sovereign Tier (40% APY -> 8.0x multiplier)
        let reward_sovereign = engine.calculate_reward(&contrib, InvestmentTier::Sovereign);
        assert_eq!(reward_sovereign, 400); // 50 * 8
    }

    #[test]
    fn test_vesting_calculation() {
        let _now = Utc::now().timestamp();
        let mut state = VestingState {
            tier: InvestmentTier::Bronze, // 2m cliff, 2m 50%, 4m 100%
            amount_total: 1000,
            amount_released: 0,
            start_timestamp: (Utc::now() - Duration::days(65)).timestamp(), // ~2 months ago
            last_claim_timestamp: 0,
        };

        // At 2 months, 50% should be releasable
        let releasable = VestingEngine::calculate_releasable(&state);
        assert_eq!(releasable, 500);

        // At 4 months, 100% should be releasable
        state.start_timestamp = (Utc::now() - Duration::days(125)).timestamp();
        let releasable_full = VestingEngine::calculate_releasable(&state);
        assert_eq!(releasable_full, 1000);
    }

    #[test]
    fn test_bonding_curve_purchase() {
        let economy = EconomyEngine::new();
        // supply: 1M, reserve: 250k, ratio: 0.25, deposit: 10k
        let supply = 1_000_000.0;
        let reserve = 250_000.0;
        let deposit = 10_000.0;

        let tokens = economy.calculate_purchase_return(supply, reserve, deposit);
        assert!(tokens > 0.0);
        println!("Tokens received for 10k deposit: {}", tokens);
    }

    #[test]
    fn test_circuit_breakers() {
        let economy = EconomyEngine::new();
        assert_eq!(
            economy.check_circuit_breakers(0.10),
            CircuitBreakerLevel::None
        );
        assert_eq!(
            economy.check_circuit_breakers(0.16),
            CircuitBreakerLevel::Level1
        );
        assert_eq!(
            economy.check_circuit_breakers(0.30),
            CircuitBreakerLevel::Level2
        );
        assert_eq!(
            economy.check_circuit_breakers(0.45),
            CircuitBreakerLevel::Level3
        );
    }
}
