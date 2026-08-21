//! Proof-of-Contribution Reward Tracker for Data Nodes.
//!
//! Tracks contributions made by data nodes to the Maloca mesh and computes
//! rewards based on accumulated points. Integrates with the consent registry
//! so that only nodes with active consent earn rewards.
//!
//! Endpoints:
//!   POST   /maloca/rewards/contribute  — record a contribution
//!   GET    /maloca/rewards/{node_id}   — query rewards for a node
//!   GET    /maloca/rewards/leaderboard  — ranked leaderboard
//!
//! See issue #1467.

use anyhow::{bail, Result};
use chrono::Utc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::maloca::data_node::{ConsentBody, ConsentRegistry, ConsentScope};

// ---------------------------------------------------------------------------
// ContributionType
// ---------------------------------------------------------------------------

/// The type of contribution a data node has made.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ContributionType {
    /// Node shared telemetry data.
    Telemetry,
    /// Node shared training data.
    TrainingData,
    /// Node validated another node's contribution.
    Validation,
    /// Node participated in governance (voting, proposals).
    Governance,
    /// Node contributed compute resources.
    Compute,
    /// Custom contribution type for extensibility.
    Custom(String),
}

impl ContributionType {
    /// Default point value for each contribution type.
    pub fn default_points(&self) -> u64 {
        match self {
            Self::Telemetry => 10,
            Self::TrainingData => 50,
            Self::Validation => 25,
            Self::Governance => 15,
            Self::Compute => 40,
            Self::Custom(_) => 5,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Telemetry => "telemetry",
            Self::TrainingData => "training_data",
            Self::Validation => "validation",
            Self::Governance => "governance",
            Self::Compute => "compute",
            Self::Custom(s) => s.as_str(),
        }
    }
}

// ---------------------------------------------------------------------------
// ContributionRecord
// ---------------------------------------------------------------------------

/// A single contribution made by a data node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContributionRecord {
    /// Unique identifier of this contribution.
    pub record_id: String,
    /// Identifier of the contributing node.
    pub node_id: String,
    /// Type of contribution.
    pub contribution_type: ContributionType,
    /// ISO-8601 timestamp of when the contribution was recorded.
    pub timestamp: String,
    /// Points earned for this contribution.
    pub points: u64,
}

impl ContributionRecord {
    /// Create a new contribution record with auto-generated id and timestamp.
    pub fn new(
        node_id: impl Into<String>,
        contribution_type: ContributionType,
        points: u64,
    ) -> Self {
        Self {
            record_id: format!("contrib_{}", uuid::Uuid::new_v4().simple()),
            node_id: node_id.into(),
            contribution_type,
            timestamp: Utc::now().to_rfc3339(),
            points,
        }
    }
}

// ---------------------------------------------------------------------------
// ContributionBody (POST body)
// ---------------------------------------------------------------------------

/// POST body for recording a contribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionBody {
    pub node_id: String,
    pub contribution_type: ContributionType,
    /// Optional custom points override. If None, uses the default for the type.
    #[serde(default)]
    pub points: Option<u64>,
}

// ---------------------------------------------------------------------------
// LeaderboardEntry
// ---------------------------------------------------------------------------

/// A single entry in the contribution leaderboard.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeaderboardEntry {
    pub rank: u32,
    pub node_id: String,
    pub total_points: u64,
    pub contribution_count: u64,
}

// ---------------------------------------------------------------------------
// NodeRewards
// ---------------------------------------------------------------------------

/// Aggregated reward information for a single node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeRewards {
    pub node_id: String,
    pub total_points: u64,
    pub contribution_count: u64,
    pub contributions: Vec<ContributionRecord>,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RewardState {
    records: Vec<ContributionRecord>,
}

// ---------------------------------------------------------------------------
// RewardTracker
// ---------------------------------------------------------------------------

/// Thread-safe reward tracker backed by a JSON file.
///
/// Integrates with a `ConsentRegistry` — contributions from nodes without
/// active consent are rejected.
pub struct RewardTracker {
    inner: RwLock<RewardState>,
    path: PathBuf,
    consent: Arc<ConsentRegistry>,
}

impl RewardTracker {
    /// Open (or create) the reward tracker at `<state_dir>/maloca/rewards.json`.
    pub fn open(state_dir: &Path, consent: Arc<ConsentRegistry>) -> Arc<Self> {
        let path = state_dir.join("maloca").join("rewards.json");
        let state = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or_default()
        } else {
            RewardState::default()
        };
        Arc::new(Self {
            inner: RwLock::new(state),
            path,
            consent,
        })
    }

    /// Create a tracker in the user's standard data directory.
    pub fn new_std(consent: Arc<ConsentRegistry>) -> Arc<Self> {
        let data_dir = dirs::data_local_dir()
            .or_else(|| dirs::home_dir())
            .unwrap_or_else(|| PathBuf::from("."));
        Self::open(&data_dir, consent)
    }

    /// Persist state to disk.
    fn persist(&self, state: &RewardState) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(raw) = serde_json::to_string_pretty(state) {
            let _ = std::fs::write(&self.path, raw);
        }
    }

    /// Record a contribution. Fails if the node does not have active consent.
    pub fn record_contribution(&self, body: ContributionBody) -> Result<ContributionRecord> {
        // Check that the node has active consent
        match self.consent.check(&body.node_id) {
            Ok(c) if c.consented => { /* good */ }
            Ok(_) => bail!(
                "node {} has revoked consent — contribution rejected",
                body.node_id
            ),
            Err(_) => bail!(
                "node {} has no consent record — register consent first",
                body.node_id
            ),
        }

        let points = body
            .points
            .unwrap_or_else(|| body.contribution_type.default_points());
        let record = ContributionRecord::new(&body.node_id, body.contribution_type, points);

        let mut state = self.inner.write();
        state.records.push(record.clone());
        self.persist(&state);
        Ok(record)
    }

    /// Get aggregated rewards for a specific node.
    pub fn get_rewards(&self, node_id: &str) -> Option<NodeRewards> {
        let state = self.inner.read();
        let contributions: Vec<ContributionRecord> = state
            .records
            .iter()
            .filter(|r| r.node_id == node_id)
            .cloned()
            .collect();

        if contributions.is_empty() {
            return None;
        }

        let total_points = contributions.iter().map(|r| r.points).sum();
        Some(NodeRewards {
            node_id: node_id.to_string(),
            total_points,
            contribution_count: contributions.len() as u64,
            contributions,
        })
    }

    /// Compute the leaderboard — nodes ranked by total points (descending).
    pub fn leaderboard(&self) -> Vec<LeaderboardEntry> {
        let state = self.inner.read();

        // Aggregate per-node totals
        let mut totals: HashMap<String, (u64, u64)> = HashMap::new(); // node_id → (points, count)
        for record in &state.records {
            let entry = totals.entry(record.node_id.clone()).or_insert((0, 0));
            entry.0 += record.points;
            entry.1 += 1;
        }

        // Sort by points descending, then by node_id for stability
        let mut sorted: Vec<_> = totals.into_iter().collect();
        sorted.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));

        sorted
            .into_iter()
            .enumerate()
            .map(|(i, (node_id, (points, count)))| LeaderboardEntry {
                rank: (i as u32) + 1,
                node_id,
                total_points: points,
                contribution_count: count,
            })
            .collect()
    }

    /// Get the total number of contribution records stored.
    pub fn record_count(&self) -> usize {
        self.inner.read().records.len()
    }

    /// Auto-register a node with consent if it doesn't exist yet.
    /// Returns `true` if the node was newly registered.
    pub fn ensure_consent(&self, node_id: &str) -> bool {
        if self.consent.check(node_id).is_ok() {
            false
        } else {
            self.consent.register(ConsentBody {
                node_id: node_id.to_string(),
                consented: true,
                scope: ConsentScope::Full,
            });
            true
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Create a test tracker with a temp directory and fresh consent registry.
    fn make_tracker() -> (Arc<RewardTracker>, Arc<ConsentRegistry>, TempDir) {
        let dir = TempDir::new().unwrap();
        let consent = ConsentRegistry::open(dir.path());
        let tracker = RewardTracker::open(dir.path(), consent.clone());
        (tracker, consent, dir)
    }

    /// Register a node with consent in the test registry.
    fn register_node(consent: &ConsentRegistry, node_id: &str) {
        consent.register(ConsentBody {
            node_id: node_id.to_string(),
            consented: true,
            scope: ConsentScope::Full,
        });
    }

    #[test]
    fn test_record_contribution_basic() {
        let (tracker, consent, _dir) = make_tracker();
        register_node(&consent, "node-alpha");

        let body = ContributionBody {
            node_id: "node-alpha".into(),
            contribution_type: ContributionType::Telemetry,
            points: None,
        };
        let record = tracker.record_contribution(body).unwrap();

        assert_eq!(record.node_id, "node-alpha");
        assert_eq!(record.contribution_type, ContributionType::Telemetry);
        assert_eq!(record.points, 10); // default for Telemetry
        assert!(record.record_id.starts_with("contrib_"));
    }

    #[test]
    fn test_record_contribution_custom_points() {
        let (tracker, consent, _dir) = make_tracker();
        register_node(&consent, "node-beta");

        let body = ContributionBody {
            node_id: "node-beta".into(),
            contribution_type: ContributionType::Custom("bug_fix".into()),
            points: Some(100),
        };
        let record = tracker.record_contribution(body).unwrap();

        assert_eq!(record.points, 100);
        assert_eq!(
            record.contribution_type,
            ContributionType::Custom("bug_fix".into())
        );
    }

    #[test]
    fn test_record_contribution_rejected_without_consent() {
        let (tracker, _consent, _dir) = make_tracker();

        let body = ContributionBody {
            node_id: "unknown-node".into(),
            contribution_type: ContributionType::Compute,
            points: None,
        };
        let result = tracker.record_contribution(body);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no consent record"));
    }

    #[test]
    fn test_record_contribution_rejected_revoked_consent() {
        let (tracker, consent, _dir) = make_tracker();
        register_node(&consent, "node-gamma");
        consent.revoke("node-gamma").unwrap();

        let body = ContributionBody {
            node_id: "node-gamma".into(),
            contribution_type: ContributionType::Governance,
            points: None,
        };
        let result = tracker.record_contribution(body);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("revoked consent"));
    }

    #[test]
    fn test_get_rewards_single_node() {
        let (tracker, consent, _dir) = make_tracker();
        register_node(&consent, "node-1");

        // Record 3 contributions
        for _ in 0..3 {
            tracker
                .record_contribution(ContributionBody {
                    node_id: "node-1".into(),
                    contribution_type: ContributionType::TrainingData,
                    points: None,
                })
                .unwrap();
        }

        let rewards = tracker.get_rewards("node-1").unwrap();
        assert_eq!(rewards.total_points, 150); // 3 × 50
        assert_eq!(rewards.contribution_count, 3);
        assert_eq!(rewards.contributions.len(), 3);
    }

    #[test]
    fn test_get_rewards_unknown_node() {
        let (tracker, _consent, _dir) = make_tracker();
        assert!(tracker.get_rewards("nonexistent").is_none());
    }

    #[test]
    fn test_leaderboard_ranking() {
        let (tracker, consent, _dir) = make_tracker();
        register_node(&consent, "alpha");
        register_node(&consent, "beta");
        register_node(&consent, "gamma");

        // alpha: 2 × TrainingData = 100 pts
        for _ in 0..2 {
            tracker
                .record_contribution(ContributionBody {
                    node_id: "alpha".into(),
                    contribution_type: ContributionType::TrainingData,
                    points: None,
                })
                .unwrap();
        }
        // beta: 1 × Compute = 40 pts
        tracker
            .record_contribution(ContributionBody {
                node_id: "beta".into(),
                contribution_type: ContributionType::Compute,
                points: None,
            })
            .unwrap();
        // gamma: 3 × Telemetry = 30 pts
        for _ in 0..3 {
            tracker
                .record_contribution(ContributionBody {
                    node_id: "gamma".into(),
                    contribution_type: ContributionType::Telemetry,
                    points: None,
                })
                .unwrap();
        }

        let lb = tracker.leaderboard();
        assert_eq!(lb.len(), 3);
        assert_eq!(lb[0].node_id, "alpha");
        assert_eq!(lb[0].total_points, 100);
        assert_eq!(lb[0].rank, 1);
        assert_eq!(lb[1].node_id, "beta");
        assert_eq!(lb[1].total_points, 40);
        assert_eq!(lb[1].rank, 2);
        assert_eq!(lb[2].node_id, "gamma");
        assert_eq!(lb[2].total_points, 30);
        assert_eq!(lb[2].rank, 3);
    }

    #[test]
    fn test_leaderboard_empty() {
        let (tracker, _consent, _dir) = make_tracker();
        let lb = tracker.leaderboard();
        assert!(lb.is_empty());
    }

    #[test]
    fn test_leaderboard_stable_sort() {
        let (tracker, consent, _dir) = make_tracker();
        register_node(&consent, "charlie");
        register_node(&consent, "bravo");
        register_node(&consent, "alpha");

        // All three get exactly 10 points
        for node in &["charlie", "bravo", "alpha"] {
            tracker
                .record_contribution(ContributionBody {
                    node_id: node.to_string(),
                    contribution_type: ContributionType::Telemetry,
                    points: Some(10),
                })
                .unwrap();
        }

        let lb = tracker.leaderboard();
        // Same points → alphabetical tiebreak
        assert_eq!(lb[0].node_id, "alpha");
        assert_eq!(lb[1].node_id, "bravo");
        assert_eq!(lb[2].node_id, "charlie");
    }

    #[test]
    fn test_ensure_consent_creates_new() {
        let (tracker, consent, _dir) = make_tracker();
        assert!(tracker.ensure_consent("new-node"));
        let c = consent.check("new-node").unwrap();
        assert!(c.consented);
        assert_eq!(c.scope, ConsentScope::Full);
    }

    #[test]
    fn test_ensure_consent_idempotent() {
        let (tracker, consent, _dir) = make_tracker();
        register_node(&consent, "existing");
        assert!(!tracker.ensure_consent("existing"));
    }

    #[test]
    fn test_record_count() {
        let (tracker, consent, _dir) = make_tracker();
        register_node(&consent, "node-x");

        assert_eq!(tracker.record_count(), 0);

        tracker
            .record_contribution(ContributionBody {
                node_id: "node-x".into(),
                contribution_type: ContributionType::Governance,
                points: None,
            })
            .unwrap();
        assert_eq!(tracker.record_count(), 1);

        tracker
            .record_contribution(ContributionBody {
                node_id: "node-x".into(),
                contribution_type: ContributionType::Compute,
                points: None,
            })
            .unwrap();
        assert_eq!(tracker.record_count(), 2);
    }

    #[test]
    fn test_contribution_type_default_points() {
        assert_eq!(ContributionType::Telemetry.default_points(), 10);
        assert_eq!(ContributionType::TrainingData.default_points(), 50);
        assert_eq!(ContributionType::Validation.default_points(), 25);
        assert_eq!(ContributionType::Governance.default_points(), 15);
        assert_eq!(ContributionType::Compute.default_points(), 40);
        assert_eq!(ContributionType::Custom("x".into()).default_points(), 5);
    }

    #[test]
    fn test_contribution_serialization_roundtrip() {
        let record = ContributionRecord::new("node-test", ContributionType::TrainingData, 75);
        let json = serde_json::to_string(&record).unwrap();
        let deserialized: ContributionRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(record, deserialized);
    }

    #[test]
    fn test_rewards_concurrent_contributions() {
        let (tracker, consent, _dir) = make_tracker();
        let num_nodes = 5;
        let contribs_per_node = 10;
        for i in 0..num_nodes {
            register_node(&consent, &format!("node-{i}"));
        }

        let handles: Vec<_> = (0..num_nodes)
            .map(|i| {
                let tracker = tracker.clone();
                std::thread::spawn(move || {
                    let node_id = format!("node-{i}");
                    for _ in 0..contribs_per_node {
                        tracker
                            .record_contribution(ContributionBody {
                                node_id: node_id.clone(),
                                contribution_type: ContributionType::Telemetry,
                                points: None,
                            })
                            .unwrap();
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(tracker.record_count(), num_nodes * contribs_per_node);
        for i in 0..num_nodes {
            let rewards = tracker.get_rewards(&format!("node-{i}")).unwrap();
            assert_eq!(rewards.contribution_count, contribs_per_node as u64);
            assert_eq!(rewards.total_points, (contribs_per_node * 10) as u64);
        }
    }

    #[test]
    fn test_rewards_leaderboard_pagination() {
        let (tracker, consent, _dir) = make_tracker();
        let node_count = 120;
        for i in 0..node_count {
            let node_id = format!("node-{:03}", i);
            register_node(&consent, &node_id);
            tracker
                .record_contribution(ContributionBody {
                    node_id,
                    contribution_type: ContributionType::Telemetry,
                    points: Some((i + 1) as u64 * 10),
                })
                .unwrap();
        }

        let lb = tracker.leaderboard();
        assert_eq!(lb.len(), node_count);
        for (idx, entry) in lb.iter().enumerate() {
            assert_eq!(entry.rank, (idx + 1) as u32);
            if idx > 0 {
                assert!(entry.total_points <= lb[idx - 1].total_points);
            }
        }
        assert_eq!(lb[0].node_id, "node-119");
        assert_eq!(lb[0].total_points, 1200);
        assert_eq!(lb[node_count - 1].node_id, "node-000");
        assert_eq!(lb[node_count - 1].total_points, 10);
    }

    #[test]
    fn test_rewards_point_overflow_protection() {
        let (tracker, consent, _dir) = make_tracker();
        register_node(&consent, "whale-node");

        let p1 = u64::MAX / 2;
        let p2 = u64::MAX / 3;

        tracker
            .record_contribution(ContributionBody {
                node_id: "whale-node".into(),
                contribution_type: ContributionType::Compute,
                points: Some(p1),
            })
            .unwrap();

        tracker
            .record_contribution(ContributionBody {
                node_id: "whale-node".into(),
                contribution_type: ContributionType::TrainingData,
                points: Some(p2),
            })
            .unwrap();

        let rewards = tracker.get_rewards("whale-node").unwrap();
        assert_eq!(rewards.total_points, p1 + p2);
        assert_eq!(rewards.contribution_count, 2);

        let lb = tracker.leaderboard();
        assert_eq!(lb.len(), 1);
        assert_eq!(lb[0].total_points, p1 + p2);
    }

    #[test]
    fn test_rewards_concurrent_consent_and_contribution() {
        let (tracker, consent, _dir) = make_tracker();
        let node_id = "race-node";

        let consent_clone = consent.clone();
        let consent_handle = std::thread::spawn(move || {
            for i in 0..50 {
                if i % 2 == 0 {
                    consent_clone.register(ConsentBody {
                        node_id: node_id.to_string(),
                        consented: true,
                        scope: ConsentScope::Full,
                    });
                } else {
                    let _ = consent_clone.revoke(node_id);
                }
                std::thread::yield_now();
            }
        });

        let tracker_clone = tracker.clone();
        let contrib_handle = std::thread::spawn(move || {
            let mut success_count = 0;
            let mut fail_count = 0;
            for _ in 0..100 {
                let res = tracker_clone.record_contribution(ContributionBody {
                    node_id: node_id.to_string(),
                    contribution_type: ContributionType::Telemetry,
                    points: None,
                });
                if res.is_ok() {
                    success_count += 1;
                } else {
                    fail_count += 1;
                }
            }
            (success_count, fail_count)
        });

        consent_handle.join().unwrap();
        let (successes, fails) = contrib_handle.join().unwrap();

        assert_eq!(successes + fails, 100);
        assert_eq!(tracker.record_count(), successes);
    }

    #[test]
    fn test_rewards_contribution_type_boundary() {
        let (tracker, consent, _dir) = make_tracker();
        register_node(&consent, "all-types-node");

        let types = vec![
            (ContributionType::Telemetry, "telemetry", 10),
            (ContributionType::TrainingData, "training_data", 50),
            (ContributionType::Validation, "validation", 25),
            (ContributionType::Governance, "governance", 15),
            (ContributionType::Compute, "compute", 40),
            (
                ContributionType::Custom("special_task".into()),
                "special_task",
                5,
            ),
        ];

        let expected_total: u64 = types.iter().map(|(_, _, p)| *p).sum();

        for (ctype, expected_str, expected_points) in &types {
            assert_eq!(ctype.as_str(), *expected_str);
            assert_eq!(ctype.default_points(), *expected_points);

            let record = tracker
                .record_contribution(ContributionBody {
                    node_id: "all-types-node".into(),
                    contribution_type: ctype.clone(),
                    points: None,
                })
                .unwrap();

            assert_eq!(&record.contribution_type, ctype);
            assert_eq!(record.points, *expected_points);
        }

        let rewards = tracker.get_rewards("all-types-node").unwrap();
        assert_eq!(rewards.contribution_count, types.len() as u64);
        assert_eq!(rewards.total_points, expected_total);
    }

    #[test]
    fn test_rewards_reward_calculation_accuracy() {
        let (tracker, consent, _dir) = make_tracker();
        register_node(&consent, "node-math-a");
        register_node(&consent, "node-math-b");
        register_node(&consent, "node-math-c");

        tracker
            .record_contribution(ContributionBody {
                node_id: "node-math-a".into(),
                contribution_type: ContributionType::Telemetry,
                points: None,
            })
            .unwrap();
        tracker
            .record_contribution(ContributionBody {
                node_id: "node-math-a".into(),
                contribution_type: ContributionType::TrainingData,
                points: None,
            })
            .unwrap();
        tracker
            .record_contribution(ContributionBody {
                node_id: "node-math-a".into(),
                contribution_type: ContributionType::Validation,
                points: None,
            })
            .unwrap();

        for p in [1, 99, 400] {
            tracker
                .record_contribution(ContributionBody {
                    node_id: "node-math-b".into(),
                    contribution_type: ContributionType::Compute,
                    points: Some(p),
                })
                .unwrap();
        }

        tracker
            .record_contribution(ContributionBody {
                node_id: "node-math-c".into(),
                contribution_type: ContributionType::Custom("a".into()),
                points: None,
            })
            .unwrap();
        tracker
            .record_contribution(ContributionBody {
                node_id: "node-math-c".into(),
                contribution_type: ContributionType::Custom("b".into()),
                points: Some(35),
            })
            .unwrap();

        let ra = tracker.get_rewards("node-math-a").unwrap();
        assert_eq!(ra.total_points, 85);
        assert_eq!(ra.contribution_count, 3);

        let rb = tracker.get_rewards("node-math-b").unwrap();
        assert_eq!(rb.total_points, 500);
        assert_eq!(rb.contribution_count, 3);

        let rc = tracker.get_rewards("node-math-c").unwrap();
        assert_eq!(rc.total_points, 40);
        assert_eq!(rc.contribution_count, 2);

        let lb = tracker.leaderboard();
        assert_eq!(lb.len(), 3);
        assert_eq!(lb[0].node_id, "node-math-b");
        assert_eq!(lb[0].total_points, 500);
        assert_eq!(lb[1].node_id, "node-math-a");
        assert_eq!(lb[1].total_points, 85);
        assert_eq!(lb[2].node_id, "node-math-c");
        assert_eq!(lb[2].total_points, 40);
    }

    #[test]
    fn test_rewards_empty_leaderboard_returns_empty() {
        let (tracker, consent, _dir) = make_tracker();
        register_node(&consent, "registered-but-no-contrib");

        let lb = tracker.leaderboard();
        assert!(lb.is_empty());

        assert!(tracker.get_rewards("registered-but-no-contrib").is_none());
        assert_eq!(tracker.record_count(), 0);
    }

    #[test]
    fn test_rewards_multiple_contribution_types_per_node() {
        let (tracker, consent, _dir) = make_tracker();
        let node_id = "multi-type-node";
        register_node(&consent, node_id);

        let types = vec![
            ContributionType::Telemetry,
            ContributionType::TrainingData,
            ContributionType::Telemetry,
            ContributionType::Governance,
            ContributionType::Compute,
            ContributionType::Custom("plugin".into()),
        ];

        for t in &types {
            tracker
                .record_contribution(ContributionBody {
                    node_id: node_id.into(),
                    contribution_type: t.clone(),
                    points: None,
                })
                .unwrap();
        }

        let rewards = tracker.get_rewards(node_id).unwrap();
        assert_eq!(rewards.contribution_count, types.len() as u64);
        assert_eq!(rewards.contributions.len(), types.len());

        for (i, expected_type) in types.iter().enumerate() {
            assert_eq!(&rewards.contributions[i].contribution_type, expected_type);
            assert_eq!(
                rewards.contributions[i].points,
                expected_type.default_points()
            );
        }

        let expected_sum: u64 = types.iter().map(|t| t.default_points()).sum();
        assert_eq!(rewards.total_points, expected_sum);
    }
}
