//! Prometheus metrics collector for telecom packets, latency & bandwidth (WAVE-27.18 / Issue #2396).
//!
//! Provides thread-safe, zero-lock atomic counters and latency tracking for
//! peer-to-peer frame transmission, bandwidth, and session handshakes.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Thread-safe Prometheus metrics collector for private telecom subsystem.
#[derive(Debug, Default)]
pub struct TelecomMetrics {
    packets_sent_total: AtomicU64,
    packets_received_total: AtomicU64,
    bytes_sent_total: AtomicU64,
    bytes_received_total: AtomicU64,
    handshakes_total: AtomicU64,
    handshakes_failed_total: AtomicU64,
    total_handshake_duration_ms: AtomicU64,
}

impl TelecomMetrics {
    /// Create a new collector with all counters initialized to zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a sent packet of the specified byte length.
    pub fn record_packet_sent(&self, byte_len: usize) {
        self.packets_sent_total.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent_total
            .fetch_add(byte_len as u64, Ordering::Relaxed);
    }

    /// Record a received packet of the specified byte length.
    pub fn record_packet_received(&self, byte_len: usize) {
        self.packets_received_total.fetch_add(1, Ordering::Relaxed);
        self.bytes_received_total
            .fetch_add(byte_len as u64, Ordering::Relaxed);
    }

    /// Record a completed handshake and its elapsed latency.
    pub fn record_handshake_duration(&self, duration: Duration) {
        self.handshakes_total.fetch_add(1, Ordering::Relaxed);
        self.total_handshake_duration_ms
            .fetch_add(duration.as_millis() as u64, Ordering::Relaxed);
    }

    /// Record a failed handshake attempt.
    pub fn record_handshake_failed(&self) {
        self.handshakes_failed_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Total number of packets sent across all peer sessions.
    pub fn packets_sent_total(&self) -> u64 {
        self.packets_sent_total.load(Ordering::Relaxed)
    }

    /// Total number of packets received across all peer sessions.
    pub fn packets_received_total(&self) -> u64 {
        self.packets_received_total.load(Ordering::Relaxed)
    }

    /// Total payload and framing bytes sent.
    pub fn bytes_sent_total(&self) -> u64 {
        self.bytes_sent_total.load(Ordering::Relaxed)
    }

    /// Total payload and framing bytes received.
    pub fn bytes_received_total(&self) -> u64 {
        self.bytes_received_total.load(Ordering::Relaxed)
    }

    /// Combined network bandwidth in bytes.
    pub fn bytes_total(&self) -> u64 {
        self.bytes_sent_total() + self.bytes_received_total()
    }

    /// Total successful session handshakes completed.
    pub fn handshakes_total(&self) -> u64 {
        self.handshakes_total.load(Ordering::Relaxed)
    }

    /// Average duration in seconds for completed handshakes.
    pub fn average_handshake_duration_seconds(&self) -> f64 {
        let count = self.handshakes_total();
        if count == 0 {
            0.0
        } else {
            let total_ms = self.total_handshake_duration_ms.load(Ordering::Relaxed);
            (total_ms as f64 / count as f64) / 1000.0
        }
    }

    /// Render standard Prometheus text exposition format.
    pub fn export_prometheus_text(&self) -> String {
        format!(
            "# HELP telecom_packets_sent_total Total telecom packets sent\n\
             # TYPE telecom_packets_sent_total counter\n\
             telecom_packets_sent_total {}\n\
             # HELP telecom_packets_received_total Total telecom packets received\n\
             # TYPE telecom_packets_received_total counter\n\
             telecom_packets_received_total {}\n\
             # HELP telecom_bytes_total Total telecom payload and frame bytes transmitted and received\n\
             # TYPE telecom_bytes_total counter\n\
             telecom_bytes_total {}\n\
             # HELP telecom_handshake_duration_seconds Average duration of completed P2P handshakes in seconds\n\
             # TYPE telecom_handshake_duration_seconds gauge\n\
             telecom_handshake_duration_seconds {:.6}\n\
             # HELP telecom_handshakes_failed_total Total failed handshake attempts\n\
             # TYPE telecom_handshakes_failed_total counter\n\
             telecom_handshakes_failed_total {}\n",
            self.packets_sent_total(),
            self.packets_received_total(),
            self.bytes_total(),
            self.average_handshake_duration_seconds(),
            self.handshakes_failed_total.load(Ordering::Relaxed)
        )
    }
}

/// Global shared singleton reference for convenience.
static GLOBAL_METRICS: std::sync::LazyLock<Arc<TelecomMetrics>> =
    std::sync::LazyLock::new(|| Arc::new(TelecomMetrics::new()));

/// Access the global singleton `TelecomMetrics` instance.
pub fn global() -> Arc<TelecomMetrics> {
    GLOBAL_METRICS.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_packets_and_bytes() {
        let metrics = TelecomMetrics::new();
        assert_eq!(metrics.packets_sent_total(), 0);
        assert_eq!(metrics.packets_received_total(), 0);
        assert_eq!(metrics.bytes_total(), 0);

        metrics.record_packet_sent(512);
        metrics.record_packet_sent(1024);
        metrics.record_packet_received(256);

        assert_eq!(metrics.packets_sent_total(), 2);
        assert_eq!(metrics.packets_received_total(), 1);
        assert_eq!(metrics.bytes_sent_total(), 1536);
        assert_eq!(metrics.bytes_received_total(), 256);
        assert_eq!(metrics.bytes_total(), 1792);
    }

    #[test]
    fn test_record_handshake_duration() {
        let metrics = TelecomMetrics::new();
        assert_eq!(metrics.average_handshake_duration_seconds(), 0.0);

        metrics.record_handshake_duration(Duration::from_millis(50));
        metrics.record_handshake_duration(Duration::from_millis(150));

        assert_eq!(metrics.handshakes_total(), 2);
        let avg = metrics.average_handshake_duration_seconds();
        // (50 + 150) / 2 = 100ms = 0.1s
        assert!((avg - 0.1).abs() < 1e-4);

        metrics.record_handshake_failed();
        assert_eq!(metrics.handshakes_failed_total.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_export_prometheus_text() {
        let metrics = TelecomMetrics::new();
        metrics.record_packet_sent(100);
        metrics.record_packet_received(200);
        metrics.record_handshake_duration(Duration::from_millis(250));

        let text = metrics.export_prometheus_text();
        assert!(text.contains("telecom_packets_sent_total 1"));
        assert!(text.contains("telecom_packets_received_total 1"));
        assert!(text.contains("telecom_bytes_total 300"));
        assert!(text.contains("telecom_handshake_duration_seconds 0.250000"));
    }
}
