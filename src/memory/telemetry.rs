//! Navigation Telemetry Metrics for HORMER
//!
//! Tracks node visit frequency, path lengths, and provides
//! hotspot analysis for the memory graph navigation system.
//!
//! This module is independent and can be wired into any navigation
//! system (HORMER, QmdMemory, or CLI).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex;

/// Visit statistics for a single node/document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisitInfo {
    pub count: u64,
    pub last_accessed: SystemTime,
    pub first_accessed: SystemTime,
}

/// Summary of navigation telemetry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySummary {
    pub total_visits: u64,
    pub unique_nodes: usize,
    pub avg_path_length: f64,
    pub total_paths: u64,
    pub hotspots: Vec<(String, VisitInfo)>,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub avg_retrieval_latency_ms: f64,
    pub nav_score_histogram: [u64; 10],
}

/// Core telemetry tracker for navigation operations.
///
/// `NavTelemetry` tracks how the agent explores the belief graph,
/// identifying frequently visited nodes ("hotspots") and monitoring
/// average navigation path lengths.
#[derive(Debug)]
pub struct NavTelemetry {
    visited: Arc<Mutex<HashMap<String, VisitInfo>>>,
    path_count: Arc<AtomicU64>,
    total_path_length: Arc<AtomicU64>,
    avg_path_length: Arc<Mutex<f64>>,
    cache_hits: Arc<AtomicU64>,
    cache_misses: Arc<AtomicU64>,
    total_latency_ms: Arc<AtomicU64>,
    latency_samples: Arc<AtomicU64>,
    nav_score_histogram: Arc<Mutex<[u64; 10]>>,
}

impl NavTelemetry {
    /// Creates a new telemetry tracker with empty state.
    pub fn new() -> Self {
        Self {
            visited: Arc::new(Mutex::new(HashMap::new())),
            path_count: Arc::new(AtomicU64::new(0)),
            total_path_length: Arc::new(AtomicU64::new(0)),
            avg_path_length: Arc::new(Mutex::new(0.0)),
            cache_hits: Arc::new(AtomicU64::new(0)),
            cache_misses: Arc::new(AtomicU64::new(0)),
            total_latency_ms: Arc::new(AtomicU64::new(0)),
            latency_samples: Arc::new(AtomicU64::new(0)),
            nav_score_histogram: Arc::new(Mutex::new([0; 10])),
        }
    }

    /// Records a visit to a node identified by its node_id.
    /// Thread-safe — uses internal Mutex + Atomic operations.
    pub async fn record_visit(&self, node_id: &str) {
        let now = SystemTime::now();
        let mut visited = self.visited.lock().await;

        let entry = visited.entry(node_id.to_string()).or_insert(VisitInfo {
            count: 0,
            last_accessed: now,
            first_accessed: now,
        });
        entry.count = entry.count.saturating_add(1);
        entry.last_accessed = now;
    }

    /// Records a completed navigation path of a given length.
    /// Updates running average using atomic operations.
    pub fn record_path(&self, path_len: usize) {
        self.path_count.fetch_add(1, Ordering::Relaxed);
        self.total_path_length
            .fetch_add(path_len as u64, Ordering::Relaxed);
    }

    /// Returns the top N most visited nodes (hotspots).
    pub async fn get_hotspots(&self, top_n: usize) -> Vec<(String, VisitInfo)> {
        let visited = self.visited.lock().await;
        let mut sorted: Vec<(String, VisitInfo)> = visited
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        sorted.sort_by(|a, b| {
            b.1.count
                .cmp(&a.1.count)
                .then_with(|| b.1.last_accessed.cmp(&a.1.last_accessed))
        });

        sorted.into_iter().take(top_n).collect()
    }

    /// Returns a summary of all telemetry data.
    pub async fn get_summary(&self) -> TelemetrySummary {
        let visited = self.visited.lock().await;
        let path_count = self.path_count.load(Ordering::Relaxed);
        let total_len = self.total_path_length.load(Ordering::Relaxed);
        let avg_len = if path_count > 0 {
            total_len as f64 / path_count as f64
        } else {
            0.0
        };

        let mut sorted: Vec<(String, VisitInfo)> = visited
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        sorted.sort_by(|a, b| {
            b.1.count
                .cmp(&a.1.count)
                .then_with(|| b.1.last_accessed.cmp(&a.1.last_accessed))
        });

        let latency_samples = self.latency_samples.load(Ordering::Relaxed);
        let total_latency = self.total_latency_ms.load(Ordering::Relaxed);
        let avg_latency = if latency_samples > 0 {
            total_latency as f64 / latency_samples as f64
        } else {
            0.0
        };

        TelemetrySummary {
            total_visits: visited.values().map(|v| v.count).sum(),
            unique_nodes: visited.len(),
            avg_path_length: avg_len,
            total_paths: path_count,
            hotspots: sorted.into_iter().take(10).collect(),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.cache_misses.load(Ordering::Relaxed),
            avg_retrieval_latency_ms: avg_latency,
            nav_score_histogram: *self.nav_score_histogram.lock().await,
        }
    }

    /// Records a cache hit.
    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a cache miss.
    pub fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Records the latency of a retrieval operation in milliseconds.
    pub fn record_latency(&self, latency_ms: u64) {
        self.total_latency_ms.fetch_add(latency_ms, Ordering::Relaxed);
        self.latency_samples.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a navigation score into the histogram.
    pub async fn record_nav_score(&self, score: f32) {
        let mut histogram = self.nav_score_histogram.lock().await;
        let bucket = (score * 10.0).floor() as usize;
        let bucket = bucket.min(9);
        histogram[bucket] = histogram[bucket].saturating_add(1);
    }

    /// Resets all telemetry data to zero.
    pub async fn reset(&self) {
        self.visited.lock().await.clear();
        self.path_count.store(0, Ordering::Relaxed);
        self.total_path_length.store(0, Ordering::Relaxed);
        *self.avg_path_length.lock().await = 0.0;
        self.cache_hits.store(0, Ordering::Relaxed);
        self.cache_misses.store(0, Ordering::Relaxed);
        self.total_latency_ms.store(0, Ordering::Relaxed);
        self.latency_samples.store(0, Ordering::Relaxed);
        let mut histogram = self.nav_score_histogram.lock().await;
        for i in 0..10 {
            histogram[i] = 0;
        }
    }

    /// Returns the total number of unique nodes visited.
    pub async fn unique_nodes(&self) -> usize {
        self.visited.lock().await.len()
    }

    /// Returns the total number of visit events recorded.
    pub async fn total_visits(&self) -> u64 {
        let visited = self.visited.lock().await;
        visited.values().map(|v| v.count).sum()
    }
}

impl Default for NavTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_record_visit() {
        let telemetry = NavTelemetry::new();
        telemetry.record_visit("node1").await;
        telemetry.record_visit("node1").await;
        telemetry.record_visit("node2").await;

        assert_eq!(telemetry.total_visits().await, 3);
        assert_eq!(telemetry.unique_nodes().await, 2);
    }

    #[tokio::test]
    async fn test_record_path() {
        let telemetry = NavTelemetry::new();
        telemetry.record_path(3);
        telemetry.record_path(5);
        telemetry.record_path(2);

        let summary = telemetry.get_summary().await;
        assert_eq!(summary.total_paths, 3);
        assert!((summary.avg_path_length - 3.333).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_hotspots() {
        let telemetry = NavTelemetry::new();
        telemetry.record_visit("hot").await;
        telemetry.record_visit("hot").await;
        telemetry.record_visit("hot").await;
        telemetry.record_visit("cold").await;

        let hotspots = telemetry.get_hotspots(5).await;
        assert_eq!(hotspots.first().unwrap().0, "hot");
        assert_eq!(hotspots.first().unwrap().1.count, 3);
    }

    #[tokio::test]
    async fn test_reset() {
        let telemetry = NavTelemetry::new();
        telemetry.record_visit("node").await;
        telemetry.record_path(5);

        telemetry.reset().await;
        assert_eq!(telemetry.total_visits().await, 0);
        assert_eq!(telemetry.unique_nodes().await, 0);
    }

    #[tokio::test]
    async fn test_summary_structure() {
        let telemetry = NavTelemetry::new();
        telemetry.record_visit("a").await;
        telemetry.record_path(4);

        let summary = telemetry.get_summary().await;
        assert_eq!(summary.total_visits, 1);
        assert_eq!(summary.unique_nodes, 1);
        assert_eq!(summary.total_paths, 1);
        assert!((summary.avg_path_length - 4.0).abs() < 0.001);
        assert_eq!(summary.hotspots.len(), 1);
    }
}
