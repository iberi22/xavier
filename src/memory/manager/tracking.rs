// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Memory tracking — access counts, creation times, and relevance scores.

use crate::memory::qmd_memory::MemoryDocument;
use chrono::Utc;

use super::core::MemoryManager;

impl MemoryManager {
    /// Record a memory access for tracking
    pub fn record_access(&self, doc_id: &str) {
        let mut counts = self
            .access_counts
            .lock()
            .expect("manager: access_counts lock poisoned");
        *counts.entry(doc_id.to_string()).or_insert(0) += 1;

        let mut times = self
            .last_access_times
            .lock()
            .expect("manager: last_access_times lock poisoned");
        times.insert(doc_id.to_string(), Utc::now());
    }

    /// Initialize tracking for a new document
    pub fn track_new_document(&self, doc: &MemoryDocument) {
        if let Some(id) = &doc.id {
            {
                let mut times = self
                    .created_times
                    .lock()
                    .expect("manager: created_times lock poisoned");
                times.insert(id.clone(), Utc::now());
            }

            let mut relevance = self
                .relevance_scores
                .lock()
                .expect("manager: relevance_scores lock poisoned");
            relevance.insert(id.clone(), 1.0); // Start at full relevance
        }
    }
}
