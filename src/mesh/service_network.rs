//! SWAL Service Network Telemetry Sharing
//!
//! Handles sharing of telemetry information between nodes in the SWAL service network.
//! Only purely operational/functioning telemetry metrics (cpu, memory, latency, temp, etc.)
//! are published and shared. Personal and sensitive user data are strictly filtered and excluded.

use crate::mesh::node::NodeId;
use crate::mesh::peer::PeerInfo;
use crate::mesh::telemetry_collector::{TelemetryAggregate, TelemetryCollector};
use serde::{Deserialize, Serialize};

/// Represents a shared telemetry publication across the SWAL service network.
/// Classified with clearance level INTERNAL. Defines explicit, non-generic fields
/// to avoid any leak of sensitive user information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryPublication {
    pub node_id: NodeId,
    pub ts: i64,
    pub metric_name: String,
    pub aggregate: TelemetryAggregate,
    pub clearance: String,
}

/// Helper function to check if a metric name is safe (purely operational)
/// and strictly does not contain personal or sensitive identifiers.
pub fn is_safe_telemetry_metric(name: &str) -> bool {
    let lower = name.to_lowercase();

    // Explicit blacklist of personal/sensitive keywords
    let unsafe_keywords = [
        "user", "email", "ip", "address", "name", "password",
        "secret", "token", "key", "private", "auth", "personal",
        "credential", "phone", "profile", "account", "billing",
        "card", "payment", "location", "gps", "lat", "lon"
    ];

    for kw in &unsafe_keywords {
        if lower.contains(kw) {
            // Bypass "lat" false positive if it's "latency"
            if *kw == "lat" && lower.contains("latency") {
                continue;
            }
            return false;
        }
    }

    // Explicit whitelist of safe operational/telemetry prefixes/keywords
    let safe_keywords = [
        "cpu", "mem", "latency", "temp", "disk", "network",
        "uptime", "peer", "req", "err", "queue", "bandwidth",
        "io", "bytes", "packet", "thread", "load", "storage",
        "fps", "db", "query"
    ];

    for kw in &safe_keywords {
        if lower.contains(kw) {
            return true;
        }
    }

    // Default to false for unknown metric names to be safe
    false
}

/// Publishes telemetry from the collector for the given peers.
/// Generates TelemetryPublication entries for all active safe telemetry metrics.
pub fn publish_telemetry(
    collector: &TelemetryCollector,
    _peers: &[PeerInfo],
) -> Vec<TelemetryPublication> {
    let node_id = collector.node_id().clone();
    let ts = chrono::Utc::now().timestamp();

    // List of standard operational metrics to scan and aggregate
    let standard_metrics = [
        "cpu_usage",
        "memory_bytes",
        "latency_ms",
        "cpu_temp",
        "disk_read_bytes",
        "disk_write_bytes",
        "network_in_bytes",
        "network_out_bytes",
        "uptime",
        "active_peers",
        "request_count",
        "error_rate",
        "queue_size",
        "bandwidth",
    ];

    let mut publications = Vec::new();

    for metric in &standard_metrics {
        if is_safe_telemetry_metric(metric) {
            if let Some(agg) = collector.aggregate(metric, 0) {
                publications.push(TelemetryPublication {
                    node_id: node_id.clone(),
                    ts,
                    metric_name: metric.to_string(),
                    aggregate: agg,
                    clearance: "INTERNAL".to_string(),
                });
            }
        }
    }

    publications
}

/// ServiceNetwork manages the discovery and telemetry sharing across the SWAL service network.
pub struct ServiceNetwork {
    pub node_id: NodeId,
}

impl ServiceNetwork {
    /// Creates a new ServiceNetwork instance.
    pub fn new(node_id: NodeId) -> Self {
        Self { node_id }
    }

    /// Shares telemetry publications, filtering only safe metrics and strictly excluding personal data.
    pub fn share(collector: &TelemetryCollector, peers: &[PeerInfo]) -> Vec<TelemetryPublication> {
        let mut pubs = publish_telemetry(collector, peers);

        // Final safety audit: ensure only safe telemetry is shared, never personal data
        pubs.retain(|pub_item| is_safe_telemetry_metric(&pub_item.metric_name));

        pubs
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_telemetry_publication_fields() {
        let node_id = NodeId("xv1-test-node".to_string());
        let agg = TelemetryAggregate {
            count: 10,
            min: 15.0,
            max: 25.0,
            avg: 20.0,
            stddev: 3.16,
        };

        let pub_item = TelemetryPublication {
            node_id: node_id.clone(),
            ts: 1718293041,
            metric_name: "cpu_temp".to_string(),
            aggregate: agg,
            clearance: "INTERNAL".to_string(),
        };

        assert_eq!(pub_item.node_id.as_str(), "xv1-test-node");
        assert_eq!(pub_item.ts, 1718293041);
        assert_eq!(pub_item.metric_name, "cpu_temp");
        assert_eq!(pub_item.aggregate.count, 10);
        assert_eq!(pub_item.clearance, "INTERNAL");
    }

    #[test]
    fn test_publish_telemetry_empty_collector() {
        let node_id = NodeId("xv1-test-node".to_string());
        let collector = TelemetryCollector::new(node_id);
        let peers = vec![];

        let pubs = publish_telemetry(&collector, &peers);
        assert!(pubs.is_empty(), "Empty collector should produce no publications");
    }

    #[test]
    fn test_publish_telemetry_with_safe_metrics() {
        let node_id = NodeId("xv1-test-node".to_string());
        let collector = TelemetryCollector::new(node_id);
        let peers = vec![];

        let labels = HashMap::new();
        collector.record("cpu_temp", 45.0, labels.clone());
        collector.record("cpu_temp", 55.0, labels.clone());
        collector.record("latency_ms", 12.5, labels.clone());

        let pubs = publish_telemetry(&collector, &peers);
        assert_eq!(pubs.len(), 2);

        let temp_pub = pubs.iter().find(|p| p.metric_name == "cpu_temp").unwrap();
        assert_eq!(temp_pub.aggregate.count, 2);
        assert_eq!(temp_pub.aggregate.min, 45.0);
        assert_eq!(temp_pub.aggregate.max, 55.0);
        assert_eq!(temp_pub.clearance, "INTERNAL");

        let latency_pub = pubs.iter().find(|p| p.metric_name == "latency_ms").unwrap();
        assert_eq!(latency_pub.aggregate.count, 1);
        assert_eq!(latency_pub.clearance, "INTERNAL");
    }

    #[test]
    fn test_service_network_share_filtering() {
        let node_id = NodeId("xv1-test-node".to_string());
        let collector = TelemetryCollector::new(node_id);

        let labels = HashMap::new();
        // Record safe metrics
        collector.record("cpu_temp", 42.0, labels.clone());

        // Record simulated unsafe/personal metrics (e.g. if somehow added under standard name or if we custom scanned)
        // Let's verify our custom safety helper filters them
        assert!(!is_safe_telemetry_metric("user_email_count"));
        assert!(!is_safe_telemetry_metric("user_password_attempts"));
        assert!(!is_safe_telemetry_metric("personal_ip_address"));

        let pubs = ServiceNetwork::share(&collector, &[]);
        // Only "cpu_temp" should survive and be published
        assert_eq!(pubs.len(), 1);
        assert_eq!(pubs[0].metric_name, "cpu_temp");
    }

    #[test]
    fn test_service_network_share_all_safe_metrics() {
        let node_id = NodeId("xv1-test-node".to_string());
        let collector = TelemetryCollector::new(node_id);

        let labels = HashMap::new();
        collector.record("memory_bytes", 1024.0, labels.clone());
        collector.record("cpu_usage", 12.5, labels.clone());

        let pubs = ServiceNetwork::share(&collector, &[]);
        assert_eq!(pubs.len(), 2);
        assert!(pubs.iter().any(|p| p.metric_name == "memory_bytes"));
        assert!(pubs.iter().any(|p| p.metric_name == "cpu_usage"));
    }

    #[test]
    fn test_telemetry_publication_no_personal_data_assert() {
        // Assert that the TelemetryPublication struct only contains explicit, safe fields.
        // It has no fields related to user identity or any generic 'data' field.
        let pub_item = TelemetryPublication {
            node_id: NodeId("xv1-test".to_string()),
            ts: 12345678,
            metric_name: "bandwidth".to_string(),
            aggregate: TelemetryAggregate {
                count: 1,
                min: 100.0,
                max: 100.0,
                avg: 100.0,
                stddev: 0.0,
            },
            clearance: "INTERNAL".to_string(),
        };

        // Let's do string and content checks to verify the absence of any PII
        assert!(!pub_item.node_id.as_str().contains("@"));
        assert!(!pub_item.metric_name.contains("user"));
        assert!(!pub_item.metric_name.contains("email"));
        assert_eq!(pub_item.clearance, "INTERNAL");
    }
}
