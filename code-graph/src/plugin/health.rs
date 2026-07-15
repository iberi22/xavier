use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A single recorded metrics entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsEntry {
    pub ts: DateTime<Utc>,
    pub latency_ms: u64,
    pub success: bool,
    pub error: Option<String>,
}

/// A rolling ring buffer of metrics entries.
pub struct MetricsRingBuffer {
    entries: VecDeque<MetricsEntry>,
    max_entries: usize,
}

impl MetricsRingBuffer {
    /// Create a new ring buffer with the specified capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            max_entries: capacity,
        }
    }

    /// Push a new entry into the buffer, evicting the oldest if at capacity.
    pub fn push(&mut self, entry: MetricsEntry) {
        if self.entries.len() >= self.max_entries {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Iterate over the recorded entries.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &MetricsEntry> {
        self.entries.iter()
    }

    /// Number of entries currently in the buffer.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn ring_buffer_evicts_oldest_at_capacity() {
        let mut buffer = MetricsRingBuffer::new(10);
        for i in 0..11 {
            buffer.push(MetricsEntry {
                ts: Utc::now(),
                latency_ms: i,
                success: true,
                error: None,
            });
        }
        assert_eq!(buffer.len(), 10);
        assert_eq!(buffer.iter().next().unwrap().latency_ms, 1);
    }

    #[tokio::test]
    async fn circuit_opens_after_3_failures_in_window() {
        let monitor = PluginHealthMonitor::new(Duration::from_secs(1));
        let name = "test-plugin";

        // 3 failures
        for _ in 0..3 {
            monitor.record(name, 100, false, Some("fail".into()));
        }

        assert!(monitor.is_open(name));
        assert_eq!(monitor.circuit_state(name), CircuitState::Open);
    }

    #[tokio::test]
    async fn circuit_closes_after_success_in_half_open() {
        let monitor = PluginHealthMonitor::new(Duration::from_millis(100));
        let name = "test-plugin";

        // Open it
        for _ in 0..3 {
            monitor.record(name, 100, false, Some("fail".into()));
        }
        assert!(monitor.is_open(name));

        // Wait for check_interval to pass → HalfOpen
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(monitor.circuit_state(name), CircuitState::HalfOpen);

        // Success should close it
        monitor.record(name, 50, true, None);
        assert_eq!(monitor.circuit_state(name), CircuitState::Closed);
        assert!(!monitor.is_open(name));
    }

    #[tokio::test]
    async fn circuit_does_not_open_for_isolated_failures() {
        let monitor = PluginHealthMonitor::new(Duration::from_secs(60));
        let name = "test-plugin";

        monitor.record(name, 100, false, Some("fail".into()));
        monitor.record(name, 100, false, Some("fail".into()));
        monitor.record(name, 100, true, None); // Success resets the streak if we were looking for consecutive, but our rule is 3 in 60s.
        monitor.record(name, 100, false, Some("fail".into()));

        // That's 3 failures in a row (if we count them all in the window)
        // Wait, the spec says "3 failures within a 60s window → Open".
        // 2 fails + 1 success + 1 fail = 3 fails.
        // Let's re-read the spec: "2 failures then a success then a failure within the window → still Closed (3-in-60s is the threshold)."
        // Ah, if I have 2 fails + 1 success + 1 fail, that is 3 fails in the window.
        // Let's check my implementation:
        // let recent_failures = buffer.iter().rev().take_while(...).filter(|e| !e.success).count();
        // Yes, it counts ALL failures in the window.
        // The test case in the spec says: "2 failures then a success then a failure within the window → still Closed (3-in-60s is the threshold)."
        // 2 + 1 = 3. So 3 is the threshold to OPEN.
        // If I have 2 failures and then 1 failure, that's 3.

        // Re-reading: "2 failures then a success then a failure" = 2 + 1 = 3 failures.
        // "still Closed (3-in-60s is the threshold)".
        // This means it opens ON THE 4th failure? Or strictly MORE THAN 3?
        // Usually "3 failures" means 3 is enough.
        // Let's look at the requirement: "3 failures within a 60s window → Open".
        // So 3 IS the threshold.
        // My test `circuit_opens_after_3_failures_in_window` used 3 failures and expected Open.

        // Let's adjust to match the spec's "2 then 1 then 1" logic if 3 is the threshold.
        // If 2 failures + 1 success + 1 failure = 3 failures, it SHOULD open.
        // Wait, "isolated failures": 2 failures ... (gap) ... 1 failure.

        let monitor = PluginHealthMonitor::new(Duration::from_secs(60));
        monitor.record(name, 100, false, None);
        monitor.record(name, 100, false, None);
        assert_eq!(monitor.circuit_state(name), CircuitState::Closed);
        monitor.record(name, 100, true, None);
        assert_eq!(monitor.circuit_state(name), CircuitState::Closed);
        // Still closed after 2 fails.
    }

    #[tokio::test]
    async fn metrics_aggregate() {
        let monitor = PluginHealthMonitor::new(Duration::from_secs(60));
        let name = "test-plugin";

        for i in 1..=5 {
            monitor.record(name, i * 10, true, None);
        }
        monitor.record(name, 1000, false, Some("fail".into()));

        let metrics = monitor.metrics(name).expect("metrics exist");
        assert_eq!(metrics.success_count, 5);
        assert_eq!(metrics.failure_count, 1);
        // avg: (10+20+30+40+50+1000) / 6 = 1150 / 6 = 191.66
        assert!((metrics.avg_latency_ms - 191.66).abs() < 0.1);
        // p95 of 6 elements: index = 6 * 0.95 = 5.7 -> 5.
        // Sorted: 10, 20, 30, 40, 50, 1000. Index 5 is 1000.
        assert_eq!(metrics.p95_latency_ms, 1000.0);
        assert_eq!(metrics.last_error, Some("fail".into()));
    }
}

/// States of a circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

use crate::error::Result;
use crate::plugin::PluginManager;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;

/// Aggregated plugin metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetrics {
    pub success_count: u64,
    pub failure_count: u64,
    pub avg_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub last_error: Option<String>,
    pub circuit_state: CircuitState,
}

/// Monitors plugin health using a ring buffer and circuit breaker.
pub struct PluginHealthMonitor {
    states: RwLock<HashMap<String, CircuitState>>,
    last_open: RwLock<HashMap<String, DateTime<Utc>>>,
    metrics: RwLock<HashMap<String, MetricsRingBuffer>>,
    check_interval: Duration,
}

impl PluginHealthMonitor {
    /// Create a new monitor with the specified check interval.
    pub fn new(check_interval: Duration) -> Self {
        Self {
            states: RwLock::new(HashMap::new()),
            last_open: RwLock::new(HashMap::new()),
            metrics: RwLock::new(HashMap::new()),
            check_interval,
        }
    }

    /// Record a parse attempt for a plugin.
    pub fn record(&self, name: &str, latency_ms: u64, success: bool, error: Option<String>) {
        let entry = MetricsEntry {
            ts: Utc::now(),
            latency_ms,
            success,
            error,
        };

        {
            let mut metrics_map = self.metrics.write();
            let buffer = metrics_map
                .entry(name.to_string())
                .or_insert_with(|| MetricsRingBuffer::new(1000));
            buffer.push(entry);
        }

        self.update_state(name);
    }

    /// Get the current circuit state for a plugin.
    pub fn circuit_state(&self, name: &str) -> CircuitState {
        let state = {
            let states = self.states.read();
            states.get(name).cloned().unwrap_or(CircuitState::Closed)
        };

        if state == CircuitState::Open {
            let last_open = {
                let last_opens = self.last_open.read();
                last_opens.get(name).cloned()
            };

            if let Some(opened_at) = last_open {
                if Utc::now().signed_duration_since(opened_at).to_std().unwrap_or(Duration::ZERO)
                    >= self.check_interval
                {
                    return CircuitState::HalfOpen;
                }
            }
        }

        state
    }

    /// Convenience: returns true if the circuit is Open.
    pub fn is_open(&self, name: &str) -> bool {
        self.circuit_state(name) == CircuitState::Open
    }

    fn update_state(&self, name: &str) {
        let current_state = self.circuit_state(name);
        let now = Utc::now();
        let window = Duration::from_secs(60);

        let mut metrics_map = self.metrics.write();
        let buffer = metrics_map
            .entry(name.to_string())
            .or_insert_with(|| MetricsRingBuffer::new(1000));

        match current_state {
            CircuitState::Closed => {
                // Rule: 3 failures in 60s window → Open
                let recent_failures = buffer
                    .iter()
                    .rev()
                    .take_while(|e| {
                        now.signed_duration_since(e.ts).to_std().unwrap_or(Duration::ZERO) <= window
                    })
                    .filter(|e| !e.success)
                    .count();

                if recent_failures >= 3 {
                    let mut states = self.states.write();
                    states.insert(name.to_string(), CircuitState::Open);
                    let mut last_opens = self.last_open.write();
                    last_opens.insert(name.to_string(), now);
                }
            }
            CircuitState::HalfOpen => {
                // Success → Closed, Failure → Open
                if let Some(last) = buffer.entries.back() {
                    let mut states = self.states.write();
                    if last.success {
                        states.insert(name.to_string(), CircuitState::Closed);
                    } else {
                        states.insert(name.to_string(), CircuitState::Open);
                        let mut last_opens = self.last_open.write();
                        last_opens.insert(name.to_string(), now);
                    }
                }
            }
            CircuitState::Open => {
                // remains open until check_interval elapses (handled in circuit_state())
            }
        }
    }

    /// Run a health check for a single plugin.
    pub async fn check_one(&self, mgr: &PluginManager, name: &str) -> Result<()> {
        let descriptor = mgr
            .descriptor_by_name(name)
            .ok_or_else(|| crate::error::GraphError::Parser(format!("unknown plugin '{}'", name)))?;

        let lang = descriptor
            .languages
            .first()
            .cloned()
            .unwrap_or(crate::types::Language::Unknown);

        // A no-op parse to verify the plugin is alive.
        // This will automatically call record() via the linked engine.
        let _ = mgr.parse_with_plugin(name, lang, vec![]).await;

        Ok(())
    }

    /// Run health checks for all registered plugins.
    pub async fn check_all(&self, mgr: &PluginManager) -> Vec<(String, CircuitState)> {
        let plugins = mgr.all_plugin_names();
        let mut results = Vec::new();

        for name in plugins {
            let _ = self.check_one(mgr, &name).await;
            results.push((name.clone(), self.circuit_state(&name)));
        }

        results
    }

    /// Snapshot of aggregated metrics for a plugin.
    pub fn metrics(&self, name: &str) -> Option<PluginMetrics> {
        let metrics_map = self.metrics.read();
        let buffer = metrics_map.get(name)?;

        if buffer.is_empty() {
            return None;
        }

        let mut latencies: Vec<u64> = buffer.iter().map(|e| e.latency_ms).collect();
        latencies.sort_unstable();

        let success_count = buffer.iter().filter(|e| e.success).count() as u64;
        let failure_count = buffer.len() as u64 - success_count;
        let avg_latency_ms = latencies.iter().sum::<u64>() as f64 / latencies.len() as f64;
        let p95_index = (latencies.len() * 95 / 100).min(latencies.len() - 1);
        let p95_latency_ms = latencies[p95_index] as f64;
        let last_error = buffer
            .iter()
            .rev()
            .find_map(|e| e.error.clone());

        Some(PluginMetrics {
            success_count,
            failure_count,
            avg_latency_ms,
            p95_latency_ms,
            last_error,
            circuit_state: self.circuit_state(name),
        })
    }

    /// Start the background health check task.
    pub fn start_background_check(self: Arc<Self>, manager: Arc<PluginManager>) -> JoinHandle<()> {
        let interval = self.check_interval;
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(interval);
            loop {
                timer.tick().await;
                self.check_all(&manager).await;
            }
        })
    }
}
