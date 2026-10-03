//! Pluggable training-data sources for `TrainingExporter` (design v2 sec 3.1).
//!
//! Every source yields instruction/response records
//! `{"instruction", "response", "metadata": {source, kind, domain, workspace, namespace, ..}}`
//! (telemetry keeps its original decrypted payload shape). The exporter composes the
//! sources requested, then applies privacy scrubbing, k-anonymity and the split.

use super::privacy::PrivacyLevel;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub mod challenge;
pub mod memory;
pub mod telemetry;

pub use challenge::ChallengeSource;
pub use memory::MemorySource;
pub use telemetry::TelemetrySource;

/// Per-export context handed to every source.
#[derive(Debug, Clone, Copy)]
pub struct SourceContext {
    pub seed: u64,
    pub privacy_level: PrivacyLevel,
}

/// Records produced by one source plus the accounting the audit needs.
#[derive(Debug, Default)]
pub struct SourceBatch {
    pub records: Vec<Value>,
    pub total_found: usize,
    pub excluded_no_consent: usize,
    pub excluded_revoked: usize,
    /// Other exclusions by reason (e.g. `memory:too_short`).
    pub excluded_other: BTreeMap<String, usize>,
    /// Hashed contributor ids (telemetry only).
    pub anonymized_sources: BTreeSet<String>,
}

impl SourceBatch {
    pub fn exclude(&mut self, reason: &str) {
        *self.excluded_other.entry(reason.to_string()).or_insert(0) += 1;
    }
}

#[async_trait]
pub trait TrainingSource: Send + Sync {
    /// Stable name used in requests, manifests and audits (e.g. `memory`).
    fn name(&self) -> &'static str;
    async fn collect(&self, ctx: &SourceContext) -> Result<SourceBatch, String>;
}

/// Build a record's string-only `metadata` object, skipping `None`/empty values.
pub(crate) fn metadata_object(fields: &[(&str, Option<&str>)]) -> Value {
    let mut map = serde_json::Map::new();
    for (key, value) in fields {
        if let Some(v) = value.filter(|v| !v.trim().is_empty()) {
            map.insert((*key).to_string(), Value::String(v.to_string()));
        }
    }
    Value::Object(map)
}
