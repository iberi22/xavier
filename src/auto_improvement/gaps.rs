use serde::{Deserialize, Serialize};
use super::benchmark::BenchmarkSnapshot;

/// A detected gap that could be improved
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gap {
    pub metric: String,
    pub current: f64,
    pub target: f64,
    pub gap_pct: f64,
    pub severity: GapSeverity,
    pub suggested_experiments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GapSeverity {
    Critical,
    Major,
    Minor,
}

/// Analyze gaps between current benchmarks and targets
pub fn analyze_gaps(current: &BenchmarkSnapshot, previous: Option<&BenchmarkSnapshot>) -> Vec<Gap> {
    let mut gaps = Vec::new();

    // Recall target: 70% (realistic benchmark baseline)
    let recall_target = 0.70;
    if current.recall_at_k > 0.0 && current.recall_at_k < recall_target {
        let gap = recall_target - current.recall_at_k;
        gaps.push(Gap {
            metric: "recall@k".to_string(),
            current: current.recall_at_k,
            target: recall_target,
            gap_pct: (gap / recall_target) * 100.0,
            severity: if gap > 0.3 {
                GapSeverity::Critical
            } else if gap > 0.15 {
                GapSeverity::Major
            } else {
                GapSeverity::Minor
            },
            suggested_experiments: vec![
                "Increase RRF k value".to_string(),
                "Adjust BM25 b parameter".to_string(),
                "Add query expansion".to_string(),
                "Increase embedding dimensions".to_string(),
            ],
        });
    }

    // Precision target: 60% (realistic baseline)
    let precision_target = 0.60;
    if current.precision > 0.0 && current.precision < precision_target {
        let gap = precision_target - current.precision;
        gaps.push(Gap {
            metric: "precision".to_string(),
            current: current.precision,
            target: precision_target,
            gap_pct: (gap / precision_target) * 100.0,
            severity: if gap > 0.25 {
                GapSeverity::Major
            } else {
                GapSeverity::Minor
            },
            suggested_experiments: vec![
                "Adjust relevance threshold".to_string(),
                "Enable entity extraction filter".to_string(),
                "Increase rerank depth".to_string(),
            ],
        });
    }

    // Latency target: < 200ms
    let latency_target = 200.0;
    if current.avg_latency_ms > 0.0 && current.avg_latency_ms > latency_target {
        let gap = current.avg_latency_ms - latency_target;
        gaps.push(Gap {
            metric: "avg_latency".to_string(),
            current: current.avg_latency_ms,
            target: latency_target,
            gap_pct: (gap / latency_target) * 100.0,
            severity: if gap > 500.0 {
                GapSeverity::Critical
            } else if gap > 100.0 {
                GapSeverity::Major
            } else {
                GapSeverity::Minor
            },
            suggested_experiments: vec![
                "Enable cache warming".to_string(),
                "Reduce embedding batch size".to_string(),
                "Optimize vector search index".to_string(),
            ],
        });
    }

    // Cache hit rate target: > 30%
    let cache_target = 30.0;
    if current.cache_hit_rate > 0.0 && current.cache_hit_rate < cache_target {
        gaps.push(Gap {
            metric: "cache_hit_rate".to_string(),
            current: current.cache_hit_rate,
            target: cache_target,
            gap_pct: ((cache_target - current.cache_hit_rate) / cache_target) * 100.0,
            severity: GapSeverity::Minor,
            suggested_experiments: vec![
                "Increase warmup top_k".to_string(),
                "Extend tracking period".to_string(),
            ],
        });
    }

    // DB integrity
    if !current.db_integrity_ok && current.total_documents > 0 {
        gaps.push(Gap {
            metric: "db_integrity".to_string(),
            current: 0.0,
            target: 1.0,
            gap_pct: 100.0,
            severity: GapSeverity::Critical,
            suggested_experiments: vec!["Run VACUUM".to_string(), "Rebuild indexes".to_string()],
        });
    }

    // Regression detection
    if let Some(prev) = previous {
        if current.recall_at_k > 0.0
            && prev.recall_at_k > 0.0
            && current.recall_at_k < prev.recall_at_k - 0.05
        {
            gaps.push(Gap {
                metric: "recall_regression".to_string(),
                current: current.recall_at_k,
                target: prev.recall_at_k,
                gap_pct: ((prev.recall_at_k - current.recall_at_k) / prev.recall_at_k) * 100.0,
                severity: GapSeverity::Critical,
                suggested_experiments: vec!["Rollback latest change".to_string()],
            });
        }
    }

    gaps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auto_improvement::benchmark::BenchmarkSnapshot;

    #[test]
    fn test_analyze_gaps_identifies_low_recall() {
        let current = BenchmarkSnapshot {
            timestamp_secs: 0,
            recall_at_k: 0.3,
            precision: 0.9,
            avg_latency_ms: 50.0,
            p99_latency_ms: 100.0,
            memory_hit_rate: 0.0,
            cache_hit_rate: 0.0,
            mesh_peers_reachable: 0,
            health_status: "healthy".to_string(),
            db_integrity_ok: true,
            total_documents: 100,
            test_iterations: 0,
        };
        let gaps = analyze_gaps(&current, None);
        assert!(gaps.iter().any(|g| g.metric == "recall@k"));
    }

    #[test]
    fn test_analyze_gaps_skips_when_above_target() {
        let current = BenchmarkSnapshot {
            recall_at_k: 0.95,
            precision: 0.95,
            avg_latency_ms: 50.0,
            p99_latency_ms: 100.0,
            ..Default::default()
        };
        let gaps = analyze_gaps(&current, None);
        assert!(gaps.iter().all(|g| g.metric != "recall@k"));
    }

    #[test]
    fn test_analyze_gaps_detects_regression() {
        let current = BenchmarkSnapshot {
            recall_at_k: 0.5,
            ..Default::default()
        };
        let previous = BenchmarkSnapshot {
            recall_at_k: 0.9,
            ..Default::default()
        };
        let gaps = analyze_gaps(&current, Some(&previous));
        assert!(gaps.iter().any(|g| g.metric == "recall_regression"));
    }

    #[test]
    fn test_analyze_gaps_handles_low_precision() {
        let current = BenchmarkSnapshot {
            precision: 0.2,
            ..Default::default()
        };
        let gaps = analyze_gaps(&current, None);
        assert!(gaps.iter().any(|g| g.metric == "precision"));
    }
}
