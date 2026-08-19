//! Gap Analyzer - Identifies performance gaps from real usage data

use crate::data_commons::telemetry_db::TelemetryDb;
use crate::observability::service_log::{LogLevel, ServiceLogStore};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Gap Report detailing identified performance issues
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapReport {
    pub timestamp: String,
    pub avg_latency_ms: u64,
    pub p95_latency_ms: u64,
    pub error_rate: f64,
    pub high_latency_endpoints: Vec<String>,
    pub recall_indicators: Vec<String>,
    pub critical_modules: Vec<String>,
}

pub struct GapAnalyzer {
    log_store: ServiceLogStore,
    telemetry_db: Option<TelemetryDb>,
}

impl GapAnalyzer {
    /// New.
    pub async fn new() -> Result<Self> {
        let log_store = ServiceLogStore::new().await?;
        let telemetry_db_path = std::env::var("XAVIER_TELEMETRY_DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("telemetry.sqlite3"));

        let telemetry_db = if telemetry_db_path.exists() {
            Some(TelemetryDb::new(telemetry_db_path)?)
        } else {
            None
        };

        Ok(Self {
            log_store,
            telemetry_db,
        })
    }

    /// Analyze gaps.
    pub async fn analyze_gaps(&self) -> Result<GapReport> {
        let stats = self.log_store.get_stats().await?;

        // Query recent errors to identify critical modules
        let mut critical_modules = Vec::new();
        let patterns = self.log_store.detect_patterns(60, 3).await?;
        for pattern in patterns {
            if pattern.level == LogLevel::Error && !critical_modules.contains(&pattern.module) {
                critical_modules.push(pattern.module);
            }
        }

        // Real latency analysis from logs
        let recent_logs = self.log_store.search_logs("latency_ms", 1000).await?;
        let latencies: Vec<u64> = recent_logs
            .iter()
            .filter_map(|l| {
                l.metadata
                    .as_ref()
                    .and_then(|m| m.get("latency_ms"))
                    .and_then(|v| v.as_u64())
            })
            .collect();

        let (avg_latency_ms, p95_latency_ms) = if !latencies.is_empty() {
            let mut sorted = latencies.clone();
            sorted.sort_unstable();
            let avg = latencies.iter().sum::<u64>() / latencies.len() as u64;
            let p95_idx = (latencies.len() * 95 / 100).min(latencies.len() - 1);
            (avg, sorted[p95_idx])
        } else {
            (0, 0)
        };

        let error_rate = if stats.total_entries > 0 {
            stats.errors_today as f64 / stats.total_entries as f64
        } else {
            0.0
        };

        let mut high_latency_endpoints = Vec::new();
        if p95_latency_ms > 1000 {
            // Identify which endpoints are slow
            for log in recent_logs {
                if let Some(latency) = log
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("latency_ms"))
                    .and_then(|v| v.as_u64())
                {
                    if latency > 1000 {
                        if let Some(path) = log
                            .metadata
                            .as_ref()
                            .and_then(|m| m.get("path"))
                            .and_then(|v| v.as_str())
                        {
                            if !high_latency_endpoints.contains(&path.to_string()) {
                                high_latency_endpoints.push(path.to_string());
                            }
                        }
                    }
                }
            }
        }

        let mut recall_indicators = Vec::new();
        if let Some(ref db) = self.telemetry_db {
            let logs = db.get_recent_logs(100)?;
            // Heuristic: if many logs have small payloads, it might indicate poor retrieval
            if logs
                .iter()
                .filter(|(_, payload, _, _, _)| payload.len() < 100)
                .count()
                > 20
            {
                recall_indicators.push("Low payload size in retrieval logs".to_string());
            }
        }

        Ok(GapReport {
            timestamp: chrono::Utc::now().to_rfc3339(),
            avg_latency_ms,
            p95_latency_ms,
            error_rate,
            high_latency_endpoints,
            recall_indicators,
            critical_modules,
        })
    }
}
