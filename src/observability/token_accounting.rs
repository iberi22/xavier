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

impl TokenAccountingTracker {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn track(&self, session_id: String, original_tokens: usize, optimized_tokens: usize, model_price_per_1k: f32) {
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

pub static TRACKER: std::sync::LazyLock<TokenAccountingTracker> = std::sync::LazyLock::new(TokenAccountingTracker::new);
