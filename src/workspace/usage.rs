//! Workspace usage tracking and analytics
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use super::config::{EmbeddingProviderMode, PlanTier, SyncPolicy};
use crate::agents::router::RouteCategory;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UsageCategory {
    Read,
    Write,
    Sync,
    AgentRun,
    Code,
    Account,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageCountersSnapshot {
    pub category: UsageCategory,
    pub requests: u64,
    pub units: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceUsageSnapshot {
    pub workspace_id: String,
    pub plan: PlanTier,
    pub document_count: usize,
    pub storage_bytes_used: u64,
    pub storage_bytes_limit: Option<u64>,
    pub storage_bytes_remaining: Option<u64>,
    pub requests_used: usize,
    pub request_limit: Option<usize>,
    pub request_units_used: u64,
    pub request_unit_limit: Option<u64>,
    pub sync_policy: SyncPolicy,
    pub counters: Vec<UsageCountersSnapshot>,
    pub optimization: OptimizationUsageSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelCallSnapshot {
    pub model: String,
    pub calls: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OptimizationUsageSnapshot {
    pub router_direct_count: u64,
    pub router_retrieved_count: u64,
    pub router_complex_count: u64,
    pub semantic_cache_hits: u64,
    pub semantic_cache_misses: u64,
    pub llm_calls: u64,
    pub llm_calls_by_model: Vec<ModelCallSnapshot>,
}

#[derive(Debug, Clone, Copy)]
pub struct UsageEvent {
    pub category: UsageCategory,
    pub units: u64,
}

impl UsageEvent {
    /// From request.
    pub fn from_request(method: &str, path: &str) -> Self {
        match (method, path) {
            ("GET", "/v1/account/usage")
            | ("GET", "/v1/account/limits")
            | ("GET", "/v1/sync/policies")
            | ("GET", "/v1/providers/embeddings/status") => Self {
                category: UsageCategory::Account,
                units: 1,
            },
            ("POST", "/memory/add") | ("POST", "/memory/delete") | ("POST", "/memory/reset") => {
                Self {
                    category: UsageCategory::Write,
                    units: 2,
                }
            }
            ("POST", "/memory/consolidate") | ("POST", "/memory/reflect") => Self {
                category: UsageCategory::Write,
                units: 3,
            },
            ("POST", "/memory/search")
            | ("POST", "/memory/hybrid-search")
            | ("POST", "/memory/hybrid")
            | ("POST", "/memory/query")
            | ("POST", "/memory/graph/hops")
            | ("GET", "/memory/graph") => Self {
                category: UsageCategory::Read,
                units: 1,
            },
            ("POST", "/agents/run") => Self {
                category: UsageCategory::AgentRun,
                units: 10,
            },
            ("POST", "/sync") => Self {
                category: UsageCategory::Sync,
                units: 5,
            },
            ("POST", "/code/scan") => Self {
                category: UsageCategory::Code,
                units: 4,
            },
            ("POST", "/code/find")
            | ("GET", "/code/stats")
            | ("POST", "/code/dependencies")
            | ("POST", "/code/reverse-dependencies")
            | ("POST", "/code/call-chain")
            | ("GET", "/code/hubs")
            | ("GET", "/code/hotspots") => Self {
                category: UsageCategory::Code,
                units: 1,
            },
            _ => {
                let category = if method == "GET" {
                    UsageCategory::Read
                } else {
                    UsageCategory::Other
                };
                Self { category, units: 1 }
            }
        }
    }
}

pub struct UsageCounter {
    pub requests: AtomicU64,
    pub units: AtomicU64,
}

impl UsageCounter {
    /// New.
    pub fn new() -> Self {
        Self {
            requests: AtomicU64::new(0),
            units: AtomicU64::new(0),
        }
    }
    /// Add.
    pub fn add(&self, units: u64) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        self.units.fetch_add(units, Ordering::Relaxed);
    }
    /// Snapshot.
    pub fn snapshot(&self, category: UsageCategory) -> UsageCountersSnapshot {
        UsageCountersSnapshot {
            category,
            requests: self.requests.load(Ordering::Relaxed),
            units: self.units.load(Ordering::Relaxed),
        }
    }
}

pub struct UsageMetrics {
    pub total_units: AtomicU64,
    pub counters: HashMap<UsageCategory, UsageCounter>,
}

impl UsageMetrics {
    /// New.
    pub fn new() -> Self {
        let counters = [
            UsageCategory::Read,
            UsageCategory::Write,
            UsageCategory::Sync,
            UsageCategory::AgentRun,
            UsageCategory::Code,
            UsageCategory::Account,
            UsageCategory::Other,
        ]
        .into_iter()
        .map(|category| (category, UsageCounter::new()))
        .collect();
        Self {
            total_units: AtomicU64::new(0),
            counters,
        }
    }
    /// Record.
    pub fn record(&self, event: UsageEvent) {
        self.total_units.fetch_add(event.units, Ordering::Relaxed);
        if let Some(counter) = self.counters.get(&event.category) {
            counter.add(event.units);
        }
    }
    /// Total units.
    pub fn total_units(&self) -> u64 {
        self.total_units.load(Ordering::Relaxed)
    }
    /// Hydrate.
    pub fn hydrate(&self, total_units: u64, counters: &[UsageCountersSnapshot]) {
        self.total_units.store(total_units, Ordering::Relaxed);
        for snapshot in counters {
            if let Some(counter) = self.counters.get(&snapshot.category) {
                counter.requests.store(snapshot.requests, Ordering::Relaxed);
                counter.units.store(snapshot.units, Ordering::Relaxed);
            }
        }
    }
    /// Snapshots.
    pub fn snapshots(&self) -> Vec<UsageCountersSnapshot> {
        let mut counters: Vec<_> = self
            .counters
            .iter()
            .map(|(category, counter)| counter.snapshot(*category))
            .collect();
        counters.sort_by_key(|entry| match entry.category {
            UsageCategory::Read => 0,
            UsageCategory::Write => 1,
            UsageCategory::Sync => 2,
            UsageCategory::AgentRun => 3,
            UsageCategory::Code => 4,
            UsageCategory::Account => 5,
            UsageCategory::Other => 6,
        });
        counters
    }
}

pub struct OptimizationMetrics {
    pub router_direct_count: AtomicU64,
    pub router_retrieved_count: AtomicU64,
    pub router_complex_count: AtomicU64,
    pub semantic_cache_hits: AtomicU64,
    pub semantic_cache_misses: AtomicU64,
    pub llm_calls: AtomicU64,
    pub llm_calls_by_model: RwLock<HashMap<String, u64>>,
}

impl OptimizationMetrics {
    /// New.
    pub fn new() -> Self {
        Self {
            router_direct_count: AtomicU64::new(0),
            router_retrieved_count: AtomicU64::new(0),
            router_complex_count: AtomicU64::new(0),
            semantic_cache_hits: AtomicU64::new(0),
            semantic_cache_misses: AtomicU64::new(0),
            llm_calls: AtomicU64::new(0),
            llm_calls_by_model: RwLock::new(HashMap::new()),
        }
    }
    /// Record.
    pub async fn record(
        &self,
        route_category: RouteCategory,
        semantic_cache_hit: bool,
        llm_used: bool,
        model: Option<&str>,
    ) {
        match route_category {
            RouteCategory::Direct => {
                self.router_direct_count.fetch_add(1, Ordering::Relaxed);
            }
            RouteCategory::Retrieved => {
                self.router_retrieved_count.fetch_add(1, Ordering::Relaxed);
            }
            RouteCategory::Complex => {
                self.router_complex_count.fetch_add(1, Ordering::Relaxed);
            }
        }
        if semantic_cache_hit {
            self.semantic_cache_hits.fetch_add(1, Ordering::Relaxed);
        } else if route_category != RouteCategory::Direct {
            self.semantic_cache_misses.fetch_add(1, Ordering::Relaxed);
        }
        if llm_used {
            self.llm_calls.fetch_add(1, Ordering::Relaxed);
            if let Some(model) = model.filter(|value| !value.trim().is_empty()) {
                let mut calls = self.llm_calls_by_model.write().await;
                *calls.entry(model.to_string()).or_insert(0) += 1;
            }
        }
    }
    /// Hydrate.
    pub async fn hydrate(&self, snapshot: &OptimizationUsageSnapshot) {
        self.router_direct_count
            .store(snapshot.router_direct_count, Ordering::Relaxed);
        self.router_retrieved_count
            .store(snapshot.router_retrieved_count, Ordering::Relaxed);
        self.router_complex_count
            .store(snapshot.router_complex_count, Ordering::Relaxed);
        self.semantic_cache_hits
            .store(snapshot.semantic_cache_hits, Ordering::Relaxed);
        self.semantic_cache_misses
            .store(snapshot.semantic_cache_misses, Ordering::Relaxed);
        self.llm_calls.store(snapshot.llm_calls, Ordering::Relaxed);
        let mut model_calls = self.llm_calls_by_model.write().await;
        model_calls.clear();
        for entry in &snapshot.llm_calls_by_model {
            model_calls.insert(entry.model.clone(), entry.calls);
        }
    }
    /// Snapshot.
    pub async fn snapshot(&self) -> OptimizationUsageSnapshot {
        let mut llm_calls_by_model = self
            .llm_calls_by_model
            .read()
            .await
            .iter()
            .map(|(model, calls)| ModelCallSnapshot {
                model: model.clone(),
                calls: *calls,
            })
            .collect::<Vec<_>>();
        llm_calls_by_model.sort_by(|left, right| left.model.cmp(&right.model));
        OptimizationUsageSnapshot {
            router_direct_count: self.router_direct_count.load(Ordering::Relaxed),
            router_retrieved_count: self.router_retrieved_count.load(Ordering::Relaxed),
            router_complex_count: self.router_complex_count.load(Ordering::Relaxed),
            semantic_cache_hits: self.semantic_cache_hits.load(Ordering::Relaxed),
            semantic_cache_misses: self.semantic_cache_misses.load(Ordering::Relaxed),
            llm_calls: self.llm_calls.load(Ordering::Relaxed),
            llm_calls_by_model,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceLimitsSnapshot {
    pub workspace_id: String,
    pub plan: PlanTier,
    pub storage_limit_bytes: Option<u64>,
    pub request_limit: Option<usize>,
    pub request_unit_limit: u64,
    pub embedding_provider_mode: EmbeddingProviderMode,
    pub managed_google_embeddings: bool,
    pub sync_policy: SyncPolicy,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncPolicySnapshot {
    pub workspace_id: String,
    pub current: SyncPolicy,
    pub supported: Vec<SyncPolicy>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingProviderSnapshot {
    pub workspace_id: String,
    pub mode: EmbeddingProviderMode,
    pub managed_google_embeddings: bool,
    pub configured_model: Option<String>,
    pub configured_url: Option<String>,
    pub configured: bool,
    pub available: bool,
    pub last_error: Option<String>,
}
impl Default for UsageCounter {
    fn default() -> Self {
        Self::new()
    }
}
impl Default for UsageMetrics {
    fn default() -> Self {
        Self::new()
    }
}
impl Default for OptimizationMetrics {
    fn default() -> Self {
        Self::new()
    }
}
