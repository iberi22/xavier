// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Memory manager types and enums
//!
//! Primitive types used throughout the memory management system:
//! priority levels, quality scores, management actions, and configuration.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::memory::qmd_memory::MemoryDocument;

/// Memory priority levels - determines retention policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPriority {
    /// BELA's profile, client data, key decisions - NEVER evict
    Critical = 0,
    /// Project status, technical decisions - very long retention
    High = 1,
    /// Operations, cron jobs, monitoring - standard retention
    Medium = 2,
    /// Raw logs, temporary data - short retention
    Low = 3,
    /// Can be forgotten immediately after TTL
    Ephemeral = 4,
}

impl MemoryPriority {
    pub fn from_metadata(metadata: &serde_json::Value) -> Self {
        metadata
            .get("memory_priority")
            .and_then(|v| v.as_str())
            .and_then(|s| match s {
                "critical" => Some(Self::Critical),
                "high" => Some(Self::High),
                "medium" => Some(Self::Medium),
                "low" => Some(Self::Low),
                "ephemeral" => Some(Self::Ephemeral),
                _ => None,
            })
            .unwrap_or(Self::Medium)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Ephemeral => "ephemeral",
        }
    }

    /// Returns base decay factor for this priority (higher = decay slower)
    pub fn decay_base(&self) -> f32 {
        match self {
            Self::Critical => 1.0,  // No decay
            Self::High => 0.98,     // 2% decay per day
            Self::Medium => 0.95,   // 5% decay per day
            Self::Low => 0.85,      // 15% decay per day
            Self::Ephemeral => 0.5, // 50% decay per day
        }
    }

    /// Maximum age in days before eviction candidate
    pub fn max_age_days(&self) -> f64 {
        match self {
            Self::Critical => 365.0 * 10.0, // 10 years
            Self::High => 365.0,            // 1 year
            Self::Medium => 90.0,           // 90 days
            Self::Low => 14.0,              // 14 days
            Self::Ephemeral => 1.0,         // 1 day
        }
    }

    /// Minimum relevance score before eviction
    pub fn min_relevance(&self) -> f32 {
        match self {
            Self::Critical => 0.0, // Never evict based on relevance
            Self::High => 0.1,
            Self::Medium => 0.2,
            Self::Low => 0.3,
            Self::Ephemeral => 0.5,
        }
    }
}

/// Memory Quality Score - composite score for retention decisions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryQuality {
    /// 0-1 based on access frequency and priority
    pub relevance_score: f32,
    /// 0-1 based on belief graph verifications
    pub accuracy_score: f32,
    /// 0-1 based on time since last access
    pub freshness_score: f32,
    /// 0-1 based on metadata completeness
    pub completeness_score: f32,
    /// Weighted composite score
    pub overall: f32,
}

impl MemoryQuality {
    /// Weights for composite score
    const RELEVANCE_WEIGHT: f32 = 0.40;
    const ACCURACY_WEIGHT: f32 = 0.25;
    const FRESHNESS_WEIGHT: f32 = 0.20;
    const COMPLETENESS_WEIGHT: f32 = 0.15;

    pub fn calculate(
        doc: &MemoryDocument,
        priority: MemoryPriority,
        access_count: usize,
        last_access: Option<DateTime<Utc>>,
        verified: bool,
    ) -> Self {
        // Relevance: access frequency + priority boost
        let base_relevance = (access_count as f32 * 0.1).min(1.0);
        let priority_boost = match priority {
            MemoryPriority::Critical => 1.0,
            MemoryPriority::High => 0.8,
            MemoryPriority::Medium => 0.6,
            MemoryPriority::Low => 0.4,
            MemoryPriority::Ephemeral => 0.2,
        };
        let relevance_score = (base_relevance * 0.6 + priority_boost * 0.4).min(1.0);

        // Accuracy: based on verification in belief graph and memory level
        let level_accuracy = match doc.level {
            crate::memory::schema::MemoryLevel::Belief => 1.0,
            crate::memory::schema::MemoryLevel::Extracted => 0.8,
            crate::memory::schema::MemoryLevel::Processed => 0.7,
            crate::memory::schema::MemoryLevel::Raw => 0.5,
        };
        let accuracy_score = if verified { 1.0 } else { level_accuracy };

        // Freshness: based on days since last access
        let freshness_score = if let Some(last) = last_access {
            let days_since = (Utc::now() - last).num_days() as f32;
            let max_days = priority.max_age_days() as f32;
            (1.0 - days_since / max_days).clamp(0.0, 1.0)
        } else {
            // No access record = assume fresh
            0.8
        };

        // Completeness: based on metadata fields
        let completeness_score = {
            let meta = &doc.metadata;
            let mut score = 0.0;
            let mut count = 0;
            for key in ["kind", "namespace", "provenance", "source_path"] {
                if meta.get(key).is_some() {
                    score += 1.0;
                }
                count += 1;
            }
            if count > 0 {
                score / count as f32
            } else {
                0.5
            }
        };

        let overall = Self::RELEVANCE_WEIGHT * relevance_score
            + Self::ACCURACY_WEIGHT * accuracy_score
            + Self::FRESHNESS_WEIGHT * freshness_score
            + Self::COMPLETENESS_WEIGHT * completeness_score;

        Self {
            relevance_score,
            accuracy_score,
            freshness_score,
            completeness_score,
            overall: overall.clamp(0.0, 1.0),
        }
    }
}

/// Memory entry with metadata for management decisions
#[derive(Debug, Clone)]
pub struct ManagedMemory {
    pub doc: MemoryDocument,
    pub priority: MemoryPriority,
    pub quality: MemoryQuality,
    pub access_count: usize,
    pub last_access: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub size_bytes: u64,
}

/// Memory statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub total_documents: usize,
    pub total_size_bytes: u64,
    pub by_priority: HashMap<String, usize>,
    pub by_quality_bucket: HashMap<String, usize>,
    pub low_quality_count: usize,
    pub ephemeral_count: usize,
    pub decayed_count: usize,
}

/// Action taken by memory manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryManagementAction {
    Decayed {
        doc_id: String,
        old_relevance: f32,
        new_relevance: f32,
    },
    Consolidated {
        doc_ids: Vec<String>,
        into_doc_id: String,
    },
    Evicted {
        doc_id: String,
        reason: String,
        priority: String,
    },
    Compressed {
        doc_id: String,
        old_size: u64,
        new_size: u64,
    },
    Archived {
        doc_id: String,
        archive_path: String,
    },
    Promoted {
        doc_id: String,
        old_priority: String,
        new_priority: String,
    },
    Demoted {
        doc_id: String,
        old_priority: String,
        new_priority: String,
    },
}

/// Result of a management operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementResult {
    pub actions: Vec<MemoryManagementAction>,
    pub documents_affected: usize,
    pub bytes_freed: u64,
}

/// Configuration for memory manager
#[derive(Debug, Clone)]
pub struct MemoryManagerConfig {
    /// Maximum documents before eviction triggers
    pub max_documents: usize,
    /// Maximum storage bytes before eviction triggers
    pub max_storage_bytes: u64,
    /// Quality threshold below which documents are evicted
    pub quality_threshold: f32,
    /// Enable automatic decay
    pub auto_decay_enabled: bool,
    /// Enable automatic consolidation
    pub auto_consolidate_enabled: bool,
    /// Enable automatic eviction
    pub auto_evict_enabled: bool,
    /// Decay factor for all memories (can override per-priority)
    pub global_decay_factor: f32,
    /// Run auto-management every N hours
    pub auto_manage_interval_hours: u32,
    /// Compress memories larger than this size
    pub compression_threshold_bytes: usize,
}

impl Default for MemoryManagerConfig {
    fn default() -> Self {
        Self {
            max_documents: 10000,
            max_storage_bytes: 500 * 1024 * 1024, // 500MB
            quality_threshold: 0.25,
            auto_decay_enabled: true,
            auto_consolidate_enabled: true,
            auto_evict_enabled: true,
            global_decay_factor: 0.97,
            auto_manage_interval_hours: 24,
            compression_threshold_bytes: 2 * 1024, // 2KB
        }
    }
}

/// Legacy action types for backwards compatibility with existing code
#[derive(Debug, Clone)]
pub enum MemoryAction {
    Keep,
    Compress {
        doc_id: String,
        reason: String,
    },
    Delete {
        doc_id: String,
        reason: String,
    },
    Update {
        doc_id: String,
        new_content: String,
    },
    Consolidate {
        doc_ids: Vec<String>,
        reason: String,
    },
    Curate {
        doc_id: String,
    },
}
