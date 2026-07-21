//! Memory priority and scoring system.
//!
//! Defines memory priority levels and scoring algorithms used
//! by the memory manager to determine which memories to retain,
//! consolidate, or evict during maintenance cycles.

use serde::{Deserialize, Serialize};

/// Memory priority levels - determines retention policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPriority {
    /// BELA's profile, client data, key decisions - NEVER evict
    Critical = 0,
    /// Project status, technical decisions - very long retention
    High = 1,
    /// Operations, cron jobs, monitoring - standard retention
    Medium = 2,
    /// Raw logs, temporary data - short retention
    Low = 3,
    /// Can be forgotten immediately after TTL
    Ephemeral = 4,
}

impl MemoryPriority {
    /// From metadata.
    pub fn from_metadata(metadata: &serde_json::Value) -> Self {
        metadata
            .get("memory_priority")
            .and_then(|v| v.as_str())
            .and_then(|s| match s {
                "critical" => Some(Self::Critical),
                "high" => Some(Self::High),
                "medium" => Some(Self::Medium),
                "low" => Some(Self::Low),
                "ephemeral" => Some(Self::Ephemeral),
                _ => None,
            })
            .unwrap_or(Self::Medium)
    }

    /// As str.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Ephemeral => "ephemeral",
        }
    }

    /// Returns base decay factor for this priority (higher = decay slower)
    pub fn decay_base(&self) -> f32 {
        match self {
            Self::Critical => 1.0,  // No decay
            Self::High => 0.98,     // 2% decay per day
            Self::Medium => 0.95,   // 5% decay per day
            Self::Low => 0.85,      // 15% decay per day
            Self::Ephemeral => 0.5, // 50% decay per day
        }
    }

    /// Maximum age in days before eviction candidate
    pub fn max_age_days(&self) -> f64 {
        match self {
            Self::Critical => 365.0 * 10.0, // 10 years
            Self::High => 365.0,            // 1 year
            Self::Medium => 90.0,           // 90 days
            Self::Low => 14.0,              // 14 days
            Self::Ephemeral => 1.0,         // 1 day
        }
    }

    /// Minimum relevance score before eviction
    pub fn min_relevance(&self) -> f32 {
        match self {
            Self::Critical => 0.0, // Never evict based on relevance
            Self::High => 0.1,
            Self::Medium => 0.2,
            Self::Low => 0.3,
            Self::Ephemeral => 0.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_from_metadata() {
        let critical_meta = serde_json::json!({"memory_priority": "critical"});
        assert_eq!(
            MemoryPriority::from_metadata(&critical_meta),
            MemoryPriority::Critical
        );

        let default_meta = serde_json::json!({});
        assert_eq!(
            MemoryPriority::from_metadata(&default_meta),
            MemoryPriority::Medium
        );
    }

    #[test]
    fn test_decay_calculation() {
        // Critical should not decay
        assert!((MemoryPriority::Critical.decay_base() - 1.0).abs() < 0.001);

        // Ephemeral decays fast
        assert!(MemoryPriority::Ephemeral.decay_base() < 0.6);
    }
}
