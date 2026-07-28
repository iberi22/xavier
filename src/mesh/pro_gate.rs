//! SWAL Pro gate — Pro = active node (heartbeat + identity), never Stripe.
//!
//! See `docs/SWAL/NODE_PRO_AND_INSTANCES.md`. This is the Xavier-side evaluator
//! for mesh/data-plane capability gating (DL-F1-04).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Product node status (aligned with `@swal/node`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeProStatus {
    Inactive,
    Starting,
    Active,
    Degraded,
}

impl NodeProStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Starting => "starting",
            Self::Active => "active",
            Self::Degraded => "degraded",
        }
    }
}

/// Inputs for evaluating Pro eligibility on the mesh data plane.
#[derive(Debug, Clone)]
pub struct ProGateInput {
    /// Cryptographic node identity is present (vault public or mesh keypair).
    pub identity_present: bool,
    /// Unix timestamp of last successful heartbeat (secs), if any.
    pub last_heartbeat_unix: Option<u64>,
    /// Max age for a heartbeat to count as fresh.
    pub heartbeat_ttl: Duration,
    /// Optional: local Xavier endpoint was reachable at last check.
    pub xavier_reachable: bool,
}

impl Default for ProGateInput {
    fn default() -> Self {
        Self {
            identity_present: false,
            last_heartbeat_unix: None,
            heartbeat_ttl: Duration::from_secs(5 * 60),
            xavier_reachable: false,
        }
    }
}

/// True only when status is `active` — never based on payment.
pub fn is_pro_enabled(status: NodeProStatus) -> bool {
    matches!(status, NodeProStatus::Active)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn heartbeat_fresh(last: Option<u64>, ttl: Duration, now: u64) -> bool {
    match last {
        Some(ts) if now >= ts => (now - ts) <= ttl.as_secs(),
        Some(_) => false, // clock skew: future timestamp → not fresh
        None => false,
    }
}

/// Evaluate Pro status from identity + heartbeat freshness.
pub fn evaluate_pro_status(input: &ProGateInput) -> NodeProStatus {
    if !input.identity_present {
        return NodeProStatus::Inactive;
    }

    let now = now_unix();
    let fresh = heartbeat_fresh(input.last_heartbeat_unix, input.heartbeat_ttl, now);

    if input.last_heartbeat_unix.is_none() {
        // Identity exists but node has not heartbeated yet.
        return NodeProStatus::Starting;
    }

    if fresh && input.xavier_reachable {
        NodeProStatus::Active
    } else if fresh {
        // Heartbeat ok but Xavier down → still network-capable but degraded.
        NodeProStatus::Degraded
    } else {
        NodeProStatus::Degraded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pro_only_when_active() {
        assert!(!is_pro_enabled(NodeProStatus::Inactive));
        assert!(!is_pro_enabled(NodeProStatus::Starting));
        assert!(!is_pro_enabled(NodeProStatus::Degraded));
        assert!(is_pro_enabled(NodeProStatus::Active));
    }

    #[test]
    fn no_identity_is_inactive() {
        let status = evaluate_pro_status(&ProGateInput::default());
        assert_eq!(status, NodeProStatus::Inactive);
        assert!(!is_pro_enabled(status));
    }

    #[test]
    fn identity_without_heartbeat_is_starting() {
        let status = evaluate_pro_status(&ProGateInput {
            identity_present: true,
            last_heartbeat_unix: None,
            heartbeat_ttl: Duration::from_secs(300),
            xavier_reachable: true,
        });
        assert_eq!(status, NodeProStatus::Starting);
        assert!(!is_pro_enabled(status));
    }

    #[test]
    fn fresh_heartbeat_and_xavier_is_active() {
        let now = now_unix();
        let status = evaluate_pro_status(&ProGateInput {
            identity_present: true,
            last_heartbeat_unix: Some(now),
            heartbeat_ttl: Duration::from_secs(300),
            xavier_reachable: true,
        });
        assert_eq!(status, NodeProStatus::Active);
        assert!(is_pro_enabled(status));
    }

    #[test]
    fn stale_heartbeat_is_degraded() {
        let now = now_unix();
        let status = evaluate_pro_status(&ProGateInput {
            identity_present: true,
            last_heartbeat_unix: Some(now.saturating_sub(10_000)),
            heartbeat_ttl: Duration::from_secs(60),
            xavier_reachable: true,
        });
        assert_eq!(status, NodeProStatus::Degraded);
        assert!(!is_pro_enabled(status));
    }
}
