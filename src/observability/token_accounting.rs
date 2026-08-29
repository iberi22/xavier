//! Token accounting and savings tracker.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenAccountingEntry {
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
    pub original_tokens: usize,
    pub optimized_tokens: usize,
    pub cost_usd: f32,
    pub savings_usd: f32,
}

pub struct TokenAccountingTracker {
    entries: Arc<RwLock<Vec<TokenAccountingEntry>>>,
}

impl Default for TokenAccountingTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenAccountingTracker {
    /// New.
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Track.
    pub async fn track(
        &self,
        session_id: String,
        original_tokens: usize,
        optimized_tokens: usize,
        model_price_per_1k: f32,
    ) {
        // Honest cost calculation (original tokens come from host or estimated,
        // optimized tokens are already calculated via chars / 4 in restore tools)
        let cost_optimized = (optimized_tokens as f32 / 1000.0) * model_price_per_1k;
        let cost_original = (original_tokens as f32 / 1000.0) * model_price_per_1k;
        let savings = cost_original - cost_optimized;

        let entry = TokenAccountingEntry {
            session_id,
            timestamp: Utc::now(),
            original_tokens,
            optimized_tokens,
            cost_usd: cost_optimized,
            savings_usd: savings,
        };

        let mut entries = self.entries.write().await;
        entries.push(entry);
    }

    /// Get stats.
    pub async fn get_stats(&self) -> TokenStats {
        let entries = self.entries.read().await;
        let total_original: usize = entries.iter().map(|e| e.original_tokens).sum();
        let total_optimized: usize = entries.iter().map(|e| e.optimized_tokens).sum();
        let total_savings_usd: f32 = entries.iter().map(|e| e.savings_usd).sum();

        let savings_percentage = if total_original > 0 {
            (total_original as f32 - total_optimized as f32) / total_original as f32 * 100.0
        } else {
            0.0
        };

        TokenStats {
            total_original_tokens: total_original,
            total_optimized_tokens: total_optimized,
            total_savings_usd,
            savings_percentage,
            operation_count: entries.len(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TokenStats {
    pub total_original_tokens: usize,
    pub total_optimized_tokens: usize,
    pub total_savings_usd: f32,
    pub savings_percentage: f32,
    pub operation_count: usize,
}

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct SearchTokenStats {
    pub searches_total: AtomicU64,
    pub searches_snippet: AtomicU64,
    pub searches_full: AtomicU64,
    pub searches_ids: AtomicU64,
    pub bytes_snippet: AtomicU64,
    pub bytes_full: AtomicU64,
}

impl SearchTokenStats {
    pub fn record_search(&self, mode: &str, bytes: usize) {
        self.searches_total.fetch_add(1, Ordering::Relaxed);
        let b = bytes as u64;
        match mode {
            "snippet" => {
                self.searches_snippet.fetch_add(1, Ordering::Relaxed);
                self.bytes_snippet.fetch_add(b, Ordering::Relaxed);
            }
            "ids" => {
                self.searches_ids.fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                // "full" or default
                self.searches_full.fetch_add(1, Ordering::Relaxed);
                self.bytes_full.fetch_add(b, Ordering::Relaxed);
            }
        }
    }

    pub fn snapshot(&self) -> SearchTokenStatsSnapshot {
        let searches_total = self.searches_total.load(Ordering::Relaxed);
        let snippet = self.searches_snippet.load(Ordering::Relaxed);
        let full = self.searches_full.load(Ordering::Relaxed);
        let ids = self.searches_ids.load(Ordering::Relaxed);
        let bytes_snippet = self.bytes_snippet.load(Ordering::Relaxed);
        let bytes_full = self.bytes_full.load(Ordering::Relaxed);

        // Theoretical full bytes if all snippet searches had returned full payloads
        // (using average full payload bytes per search or 2.5x snippet factor)
        let est_saved_bytes = if full > 0 && bytes_full > 0 {
            let avg_full = bytes_full / full;
            snippet
                .saturating_mul(avg_full)
                .saturating_sub(bytes_snippet)
        } else {
            bytes_snippet.saturating_mul(2) // Heuristic ~60% savings
        };
        let est_tokens_saved = (est_saved_bytes / 4) as usize;

        let total_bytes = bytes_snippet + bytes_full;
        let saved_ratio = if total_bytes > 0 {
            est_saved_bytes as f64 / (total_bytes + est_saved_bytes) as f64
        } else {
            0.0
        };

        SearchTokenStatsSnapshot {
            searches_total,
            by_mode: SearchByMode { snippet, full, ids },
            bytes_returned: SearchBytesReturned {
                snippet: bytes_snippet,
                full: bytes_full,
            },
            est_tokens_saved,
            saved_ratio,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchTokenStatsSnapshot {
    pub searches_total: u64,
    pub by_mode: SearchByMode,
    pub bytes_returned: SearchBytesReturned,
    pub est_tokens_saved: usize,
    pub saved_ratio: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchByMode {
    pub snippet: u64,
    pub full: u64,
    pub ids: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchBytesReturned {
    pub snippet: u64,
    pub full: u64,
}

pub static SEARCH_STATS: std::sync::LazyLock<SearchTokenStats> =
    std::sync::LazyLock::new(SearchTokenStats::default);

pub static TRACKER: std::sync::LazyLock<TokenAccountingTracker> =
    std::sync::LazyLock::new(TokenAccountingTracker::new);
