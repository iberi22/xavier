//! Cross-cycle tuning history persistence.
//!
//! `regen tune` measures retrieval quality and proposes a better RRF config, but
//! without history each run is an island — there is no baseline to detect drift
//! against and no way to see how the recommendation evolved. This module persists
//! tuning proposals to `<repo>/.xavier/tuning-history.json` so that:
//!
//! - On the next `regen benchmark`, the last recorded proposal's metrics can be
//!   loaded and used as the baseline for `detect_recall_drift`.
//! - `regen history` can print the recent proposals.
//!
//! The on-disk format is a JSON object `{"entries":[…]}` where each entry pairs a
//! RFC3339 timestamp with the measured baseline metrics and the resulting
//! `TuningProposal`. Appending is read-modify-write; the file is small (one entry
//! per tune run) so this is cheap and keeps the format human-readable.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::eval::RetrievalMetrics;
use super::tuner::TuningProposal;

/// Default file name for the tuning history, relative to the repo's `.xavier` dir.
pub const HISTORY_FILENAME: &str = "tuning-history.json";

/// One recorded tuning pass: when it ran, the baseline it measured, and the
/// proposal it produced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// RFC3339 timestamp of the run that produced this entry.
    pub timestamp: String,
    /// Baseline recall metrics captured during the run (used for drift
    /// comparison on subsequent runs).
    pub baseline: RetrievalMetrics,
    /// The tuning proposal recommended during the run.
    pub proposal: TuningProposal,
}

/// The persisted history file: a versioned list of entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TuningHistory {
    /// Schema version, to allow future format migrations.
    pub version: u32,
    /// Newest entries are appended at the end.
    pub entries: Vec<HistoryEntry>,
}

impl TuningHistory {
    /// Create an empty history (version 1).
    pub fn new() -> Self {
        Self {
            version: 1,
            entries: Vec::new(),
        }
    }

    /// Append a new entry and return the updated length.
    pub fn push(&mut self, entry: HistoryEntry) -> usize {
        self.entries.push(entry);
        self.entries.len()
    }

    /// The most recent entry, if any. This is the baseline used for drift
    /// detection on the next run.
    pub fn last(&self) -> Option<&HistoryEntry> {
        self.entries.last()
    }

    /// The last `n` entries, newest last (chronological order). If fewer than
    /// `n` exist, all entries are returned.
    pub fn last_n(&self, n: usize) -> &[HistoryEntry] {
        let start = self.entries.len().saturating_sub(n);
        &self.entries[start..]
    }
}

/// Resolve the history file path from a `.xavier` directory, creating the
/// directory if missing.
pub fn ensure_history_path(xavier_dir: &Path) -> Result<PathBuf> {
    if !xavier_dir.exists() {
        std::fs::create_dir_all(xavier_dir)
            .with_context(|| format!("create {}", xavier_dir.display()))?;
    }
    Ok(xavier_dir.join(HISTORY_FILENAME))
}

/// Load the tuning history from disk. A missing file is treated as an empty
/// history (not an error), so first-run callers get a clean slate.
pub fn load(path: &Path) -> Result<TuningHistory> {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            if content.trim().is_empty() {
                return Ok(TuningHistory::new());
            }
            let history: TuningHistory = serde_json::from_str(&content)
                .with_context(|| format!("parse tuning history at {}", path.display()))?;
            Ok(history)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TuningHistory::new()),
        Err(e) => Err(anyhow::anyhow!(
            "read tuning history {}: {}",
            path.display(),
            e
        )),
    }
}

/// Persist the full history to disk (pretty-printed for readability / git diffs).
pub fn save(path: &Path, history: &TuningHistory) -> Result<()> {
    let json = serde_json::to_string_pretty(history)
        .context("serialize tuning history")?;
    std::fs::write(path, json)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Append a single entry to the on-disk history: load, push, save. Returns the
/// updated history so callers (e.g. the CLI) can use it for reporting without a
/// second read.
pub fn append(path: &Path, entry: HistoryEntry) -> Result<TuningHistory> {
    let mut history = load(path)?;
    history.push(entry);
    save(path, &history)?;
    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retrieval::eval::{CaseResult, RetrievalMetrics};
    use crate::retrieval::tuner::{RetrievalConfig, TuningProposal};

    fn sample_metrics(recall: f64) -> RetrievalMetrics {
        let results = vec![
            CaseResult { case_id: "a".into(), hit: recall > 0.0, first_hit_rank: Some(1) },
            CaseResult { case_id: "b".into(), hit: recall > 0.5, first_hit_rank: Some(2) },
        ];
        let mut m = RetrievalMetrics::from_results("test", &results, 5);
        m.recall_at_k = recall;
        m
    }

    fn sample_proposal(delta: f64) -> TuningProposal {
        TuningProposal {
            config: RetrievalConfig::default(),
            score: 0.9,
            baseline_score: 0.9 - delta,
            delta,
            candidates_evaluated: 27,
        }
    }

    fn entry(ts: &str, recall: f64, delta: f64) -> HistoryEntry {
        HistoryEntry {
            timestamp: ts.into(),
            baseline: sample_metrics(recall),
            proposal: sample_proposal(delta),
        }
    }

    #[test]
    fn test_save_load_roundtrip_preserves_entries() {
        let tmp = tempfile::NamedTempFile::new().expect("create temp file");
        let path = tmp.path();
        let mut history = TuningHistory::new();
        history.push(entry("2026-07-01T00:00:00Z", 0.7, 0.01));
        history.push(entry("2026-07-02T00:00:00Z", 0.8, 0.02));
        save(path, &history).expect("save");
        let loaded = load(path).expect("load");
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.entries[0].timestamp, "2026-07-01T00:00:00Z");
        assert!((loaded.entries[1].baseline.recall_at_k - 0.8).abs() < 1e-9);
        // last() points at the most recent entry.
        assert_eq!(loaded.last().unwrap().timestamp, "2026-07-02T00:00:00Z");
    }

    #[test]
    fn test_append_grows_history_across_calls() {
        let tmp = tempfile::NamedTempFile::new().expect("create temp file");
        let path = tmp.path();
        // First run: no prior file -> empty history + one entry.
        let h1 = append(path, entry("2026-07-01T00:00:00Z", 0.6, 0.0)).expect("append 1");
        assert_eq!(h1.entries.len(), 1);
        // Second run: loads prior file, appends, grows to two entries.
        let h2 = append(path, entry("2026-07-02T00:00:00Z", 0.75, 0.05)).expect("append 2");
        assert_eq!(h2.entries.len(), 2);
        assert_eq!(h2.last().unwrap().timestamp, "2026-07-02T00:00:00Z");
        // Reload from disk to confirm persistence is durable.
        let reloaded = load(path).expect("reload");
        assert_eq!(reloaded.entries.len(), 2);
    }

    #[test]
    fn test_load_missing_file_returns_empty_history() {
        let path = Path::new("/tmp/xavier-tuning-history-does-not-exist-xyz.json");
        // Guard: make sure the file truly isn't there.
        let history = load(path).expect("missing file is not an error");
        assert!(history.entries.is_empty());
        // A clean slate is still a versioned (v1) history so callers can append.
        assert_eq!(history.version, 1);
    }

    #[test]
    fn test_last_n_returns_chronological_tail() {
        let mut history = TuningHistory::new();
        for i in 0..5 {
            history.push(entry(&format!("2026-07-0{i}T00:00:00Z"), 0.5, 0.0));
        }
        let tail = history.last_n(2);
        assert_eq!(tail.len(), 2);
        // Newest last.
        assert_eq!(tail[1].timestamp, "2026-07-04T00:00:00Z");
        // Asking for more than exists returns everything in order.
        assert_eq!(history.last_n(100).len(), 5);
    }
}
