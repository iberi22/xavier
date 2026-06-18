//! Data Commons Telemetry Collector
//!
//! Collects, stores, and queries telemetry samples from the local Xavier node.
//! Samples are retained according to a configurable retention policy and can
//! be aggregated (count, min, max, avg, stddev) for monitoring and dashboards.
//!
//! # Architecture
//!
//! ```text
//! TelemetryCollector
//!   - HashMap<metric_name, Vec<TelemetrySample>>
//!        - TelemetrySample { timestamp, metric_name, value, labels }

use crate::mesh::NodeId;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

// ---------------------------------------------------------------------------
// RetentionPolicy
// ---------------------------------------------------------------------------

/// How long telemetry samples are kept before being eligible for eviction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RetentionPolicy {
    /// Keep samples for a fixed number of days.
    Standard { days: u64 },
    /// Keep samples for an extended number of days.
    Extended { days: u64 },
    /// Never evict samples.
    Infinite,
}

impl RetentionPolicy {
    /// Returns `true` if a sample with the given timestamp should be retained.
    pub fn should_retain(&self, sample_ts: i64) -> bool {
        match self {
            RetentionPolicy::Standard { days } | RetentionPolicy::Extended { days } => {
                let cutoff = Utc::now().timestamp() - (days * 86400) as i64;
                sample_ts >= cutoff
            }
            RetentionPolicy::Infinite => true,
        }
    }
}

// ---------------------------------------------------------------------------
// TelemetrySample
// ---------------------------------------------------------------------------

/// A single telemetry data point collected from the node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySample {
    /// Unix timestamp (seconds since epoch) when the sample was recorded.
    pub timestamp: i64,
    /// The name of the metric (e.g. "cpu_usage", "memory_bytes").
    pub metric_name: String,
    /// The numeric value of the metric.
    pub value: f64,
    /// Optional key-value labels for dimensional filtering.
    pub labels: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// TelemetryAggregate
// ---------------------------------------------------------------------------

/// Aggregated statistics computed over a set of telemetry samples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryAggregate {
    /// Number of samples in the window.
    pub count: u64,
    /// Minimum value observed.
    pub min: f64,
    /// Maximum value observed.
    pub max: f64,
    /// Arithmetic mean (average) of the values.
    pub avg: f64,
    /// Population standard deviation of the values.
    pub stddev: f64,
}

// ---------------------------------------------------------------------------
// TelemetryCollector
// ---------------------------------------------------------------------------

/// Collects, retains, and queries telemetry samples for a given node.
pub struct TelemetryCollector {
    /// The node this collector belongs to.
    node_id: NodeId,
    /// In-memory sample storage keyed by metric name.
    collector: Arc<Mutex<HashMap<String, Vec<TelemetrySample>>>>,
    /// Retention policy controlling eviction of old samples.
    retention_policy: RetentionPolicy,
    /// Timestamp when this collector was started.
    started_at: Instant,
}

impl TelemetryCollector {
    /// Create a new `TelemetryCollector` for the given `node_id` with a
    /// standard 30-day retention policy.
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            collector: Arc::new(Mutex::new(HashMap::new())),
            retention_policy: RetentionPolicy::Standard { days: 30 },
            started_at: Instant::now(),
        }
    }

    /// Set a custom retention policy.
    pub fn set_retention_policy(&mut self, policy: RetentionPolicy) {
        self.retention_policy = policy;
    }

    /// Record a new telemetry sample.
    ///
    /// The timestamp is automatically set to the current system time.
    pub fn record(&self, name: &str, value: f64, labels: HashMap<String, String>) {
        let sample = TelemetrySample {
            timestamp: Utc::now().timestamp(),
            metric_name: name.to_string(),
            value,
            labels,
        };

        let mut store = self.collector.lock().expect("telemetry lock poisoned");
        let samples = store.entry(name.to_string()).or_default();
        samples.push(sample);
    }

    /// Query samples for a given metric recorded since the specified timestamp.
    pub fn query(&self, name: &str, since: i64) -> Vec<TelemetrySample> {
        let store = self.collector.lock().expect("telemetry lock poisoned");

        if let Some(samples) = store.get(name) {
            samples
                .iter()
                .filter(|s| s.timestamp >= since)
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Compute aggregate statistics for a metric over samples recorded since
    /// the specified timestamp. Returns `None` if no samples match.
    pub fn aggregate(&self, name: &str, since: i64) -> Option<TelemetryAggregate> {
        let samples = self.query(name, since);
        if samples.is_empty() {
            return None;
        }

        let count = samples.len() as u64;
        let mut min = f64::MAX;
        let mut max = f64::MIN;
        let mut sum = 0.0;

        for s in &samples {
            if s.value < min {
                min = s.value;
            }
            if s.value > max {
                max = s.value;
            }
            sum += s.value;
        }

        let avg = sum / count as f64;

        // Population standard deviation
        let variance = samples
            .iter()
            .map(|s| (s.value - avg).powi(2))
            .sum::<f64>()
            / count as f64;
        let stddev = variance.sqrt();

        Some(TelemetryAggregate {
            count,
            min,
            max,
            avg,
            stddev,
        })
    }

    /// Evict samples that fall outside the retention policy.
    pub fn evict_expired(&self) {
        let cutoff = match &self.retention_policy {
            RetentionPolicy::Infinite => return,
            RetentionPolicy::Standard { days } | RetentionPolicy::Extended { days } => {
                Utc::now().timestamp() - (days * 86400) as i64
            }
        };

        let mut store = self.collector.lock().expect("telemetry lock poisoned");
        store.retain(|_, samples| {
            samples.retain(|s| s.timestamp >= cutoff);
            !samples.is_empty()
        });
    }

    /// Returns the node ID this collector belongs to.
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// Returns the retention policy in use.
    pub fn retention_policy(&self) -> &RetentionPolicy {
        &self.retention_policy
    }

    /// Returns the uptime of this collector.
    pub fn uptime(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_query() {
        let node_id = NodeId("xv1-test-node".to_string());
        let collector = TelemetryCollector::new(node_id);

        let mut labels = HashMap::new();
        labels.insert("host".to_string(), "editor-one".to_string());

        collector.record("cpu_temp", 72.5, labels.clone());
        collector.record("cpu_temp", 73.1, labels.clone());

        let results = collector.query("cpu_temp", 0);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].value, 72.5);
        assert_eq!(results[1].value, 73.1);
    }

    #[test]
    fn test_aggregate() {
        let node_id = NodeId("xv1-test-node".to_string());
        let collector = TelemetryCollector::new(node_id);

        let labels = HashMap::new();
        collector.record("latency_ms", 10.0, labels.clone());
        collector.record("latency_ms", 20.0, labels.clone());
        collector.record("latency_ms", 30.0, labels.clone());

        let agg = collector.aggregate("latency_ms", 0).unwrap();
        assert_eq!(agg.count, 3);
        assert_eq!(agg.min, 10.0);
        assert_eq!(agg.max, 30.0);
        assert_eq!(agg.avg, 20.0);
    }

    #[test]
    fn test_none_for_empty_metric() {
        let node_id = NodeId("xv1-test-node".to_string());
        let collector = TelemetryCollector::new(node_id);
        assert!(collector.aggregate("nonexistent", 0).is_none());
    }

    #[test]
    fn test_retention_policy_standard() {
        let policy = RetentionPolicy::Standard { days: 7 };
        let recent = Utc::now().timestamp();
        let old = recent - (8 * 86400);
        assert!(policy.should_retain(recent));
        assert!(!policy.should_retain(old));
    }

    #[test]
    fn test_retention_policy_infinite() {
        let policy = RetentionPolicy::Infinite;
        assert!(policy.should_retain(0));
        assert!(policy.should_retain(i64::MIN));
    }
}
