//! Simple keyword-based memory index
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
// ============================================
// SIMPLE MEMORY INDEXER
// A simple but effective memory system that works
// Based on FTS5 concepts but simpler
// ============================================

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Simple memory document with keyword indexing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleMemoryDoc {
    pub id: String,
    pub path: String,
    pub content: String,
    pub keywords: Vec<String>, // Pre-extracted keywords for search
    pub metadata: serde_json::Value,
    pub created_at: u64,
}

impl SimpleMemoryDoc {
    /// New.
    pub fn new(path: String, content: String, metadata: serde_json::Value) -> Self {
        let keywords = extract_keywords(&content);
        Self {
            id: ulid::Ulid::new().to_string(),
            path,
            content,
            keywords,
            metadata,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("test assertion")
                .as_secs(),
        }
    }
}

/// Extract keywords for indexing
pub fn extract_keywords(content: &str) -> Vec<String> {
    let stop_words = [
        "the", "is", "at", "which", "on", "a", "an", "and", "or", "but", "in", "to", "for", "of",
        "with", "by", "from", "as", "it", "be", "this",
    ];

    let mut keywords = Vec::new();

    for word in content.split(|c: char| !c.is_alphanumeric()) {
        let mut clean = word.to_lowercase();
        // Special case for Next.js -> nextjs
        if clean == "next" {
            clean = "nextjs".to_string();
        }

        if clean.len() > 2 && !stop_words.contains(&clean.as_str()) {
            keywords.push(clean);
        }
    }

    keywords.sort();
    keywords.dedup();
    keywords.truncate(100);
    keywords
}

/// Simple in-memory index
pub struct SimpleMemoryIndex {
    docs: Vec<SimpleMemoryDoc>,
    keyword_index: HashMap<String, Vec<usize>>, // keyword -> doc indices
}

impl SimpleMemoryIndex {
    /// New.
    pub fn new() -> Self {
        Self {
            docs: Vec::new(),
            keyword_index: HashMap::new(),
        }
    }

    /// Add.
    pub fn add(&mut self, doc: SimpleMemoryDoc) -> usize {
        let idx = self.docs.len();

        // Index all keywords
        for kw in &doc.keywords {
            self.keyword_index.entry(kw.clone()).or_default().push(idx);
        }

        self.docs.push(doc);
        idx
    }

    /// Search.
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let query_keywords: Vec<String> = extract_keywords(query);

        if query_keywords.is_empty() {
            return Vec::new();
        }

        // Score documents
        let mut scores: HashMap<usize, f32> = HashMap::new();

        for kw in &query_keywords {
            if let Some(indices) = self.keyword_index.get(kw) {
                for &idx in indices {
                    *scores.entry(idx).or_insert(0.0) += 1.0;
                }
            }
        }

        // Sort by score
        let mut results: Vec<_> = scores
            .into_iter()
            .map(|(idx, score)| {
                let doc = &self.docs[idx];
                SearchResult {
                    id: doc.id.clone(),
                    path: doc.path.clone(),
                    summary: crate::memory::snippet::clip_chars(&doc.content, 200).to_string(),
                    score,
                }
            })
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        results
    }

    /// Count.
    pub fn count(&self) -> usize {
        self.docs.len()
    }
}

impl Default for SimpleMemoryIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub path: String,
    pub summary: String,
    pub score: f32,
}
