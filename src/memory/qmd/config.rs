//! QMD storage configuration
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use regex::Regex;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio::sync::RwLock as AsyncRwLock;

use crate::memory::qmd_memory::types::EmbeddingCacheEntry;

// ── Scoring constants ────────────────────────────────────────────────
pub const RRF_K: f32 = 60.0;
pub const KEYWORD_WEIGHT: f32 = 0.7;
pub const SEMANTIC_WEIGHT: f32 = 0.3;
pub const MAX_EXPANSIONS: usize = 4;
pub const MAX_MULTI_HOP_DEPTH: usize = 2;
pub const MAX_RERANK_CANDIDATES: usize = 32;

// ── Embedding cache ──────────────────────────────────────────────────
pub const EMBEDDING_CACHE_TTL_SECS: u64 = 3600; // 1 hour

pub static EMBEDDING_CACHE: LazyLock<Arc<AsyncRwLock<HashMap<String, EmbeddingCacheEntry>>>> =
    LazyLock::new(|| Arc::new(AsyncRwLock::new(HashMap::new())));

// ── Synonym map ──────────────────────────────────────────────────────
pub static SYNONYM_MAP: LazyLock<HashMap<&'static str, &'static [&'static str]>> =
    LazyLock::new(|| {
        HashMap::from([
            ("bug", &["issue", "error", "failure", "defect"][..]),
            ("cache", &["caching", "memoization", "store"][..]),
            ("fast", &["quick", "speed", "latency"][..]),
            ("memory", &["context", "retrieval", "knowledge"][..]),
            ("search", &["lookup", "find", "retrieve"][..]),
            ("vector", &["embedding", "semantic", "dense"][..]),
            ("query", &["question", "request", "prompt"][..]),
            ("reasoning", &["multi-hop", "inference", "analysis"][..]),
        ])
    });

// ── Regex patterns ───────────────────────────────────────────────────
pub static SPEAKER_COLON_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^([^:\s]+):\s*").expect("valid regex"));

pub static SPEAKER_BRACKET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\[([^]\s]+)\]").expect("valid regex"));

pub static SPEAKER_ROLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:Speaker|Person|Host|Guest|Interviewer|Interviewee|Moderator):\s*([A-Z][a-zA-Z]+)",
    )
    .expect("valid regex")
});

pub static QUERY_SPEAKER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:who|what|where|when|why|how|did|was|were)(?:\s+is|\s+did|\s+was|\s+were)?\s+([A-Z][a-zA-Z]+)",
    )
    .expect("valid regex")
});

pub static SHE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bshe\b").expect("valid regex"));

pub static HE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bhe\b").expect("valid regex"));

pub static DIA_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^([a-z]+\d+):0*([0-9]+)$").expect("valid regex"));

pub static LOCOMO_PATH_DIA_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(/)([a-z]+\d+):0*([0-9]+)([#/]|$)").expect("valid regex"));
