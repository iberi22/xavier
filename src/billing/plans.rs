//! Plan definitions and limits for Xavier billing tiers.

use serde::{Deserialize, Serialize};

/// Billing plan tiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Plan {
    /// Free tier - local only, no cloud features
    Free,
    /// Cloud tier - 1GB storage, 3 nodes
    Cloud,
}

impl Plan {
    /// Get plan from Stripe price ID
    pub fn from_price_id(price_id: &str) -> Option<Self> {
        let cloud_price = std::env::var("STRIPE_PRICE_CLOUD").ok()?;
        let cloud_price = std::env::var("STRIPE_PRICE_CLOUD").ok()?;

        if price_id == cloud_price {
            Some(Self::Cloud)
        } else {
            None
        }
    }

    /// Convert plan to Stripe price ID
    pub fn price_id(&self) -> Option<String> {
        match self {
            Self::Free => None,
            Self::Cloud => std::env::var("STRIPE_PRICE_CLOUD").ok(),
        }
    }

    /// Monthly price in cents
    pub fn monthly_price_cents(&self) -> u32 {
        match self {
            Self::Free => 0,
            Self::Cloud => 0,
        }
    }
}

impl std::fmt::Display for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Free => write!(f, "free"),
            Self::Cloud => write!(f, "cloud"),
        }
    }
}

/// Plan limits and features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanLimits {
    /// Maximum storage in GB (0 for unlimited)
    pub max_storage_gb: usize,
    /// Maximum number of nodes (0 for unlimited)
    pub max_nodes: usize,
    /// List of feature flags enabled for this plan
    pub features: Vec<String>,
}

impl PlanLimits {
    /// Get limits for a specific plan
    pub fn for_plan(plan: Plan) -> Self {
        match plan {
            Plan::Free => Self {
                max_storage_gb: 0,
                max_nodes: 0,
                features: vec![
                    "local_only".to_string(),
                    "basic_memory".to_string(),
                ],
            },
            Plan::Cloud => Self {
                max_storage_gb: 1,
                max_nodes: 3,
                features: vec![
                    "cloud_sync".to_string(),
                    "basic_memory".to_string(),
                    "api_access".to_string(),
                    "email_support".to_string(),
                ],
            },
        }
    }

    /// Check if a feature is enabled for this plan
    pub fn has_feature(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }
}

/// Current subscription status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionStatus {
    /// Current plan
    pub plan: Plan,
    /// Stripe customer ID
    pub stripe_customer_id: Option<String>,
    /// Stripe subscription ID
    pub stripe_subscription_id: Option<String>,
    /// Subscription status from Stripe
    pub subscription_status: String,
    /// Current period end timestamp
    pub current_period_end: Option<i64>,
    /// Whether the subscription is active
    pub is_active: bool,
    /// Plan limits
    pub limits: PlanLimits,
}

impl Default for SubscriptionStatus {
    fn default() -> Self {
        Self {
            plan: Plan::Free,
            stripe_customer_id: None,
            stripe_subscription_id: None,
            subscription_status: "none".to_string(),
            current_period_end: None,
            is_active: false,
            limits: PlanLimits::for_plan(Plan::Free),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_from_price_id() {
        // This test requires env vars to be set
        let result = Plan::from_price_id("price_nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_plan_limits() {
        let free_limits = PlanLimits::for_plan(Plan::Free);
        assert_eq!(free_limits.max_storage_gb, 0);
        assert!(free_limits.has_feature("local_only"));

        let cloud_limits = PlanLimits::for_plan(Plan::Cloud);
        assert_eq!(cloud_limits.max_storage_gb, 1);
        assert!(cloud_limits.has_feature("cloud_sync"));

    }

    #[test]
    fn test_subscription_status_default() {
        let status = SubscriptionStatus::default();
        assert_eq!(status.plan, Plan::Free);
        assert!(!status.is_active);
    }
}
