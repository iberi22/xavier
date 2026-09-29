//! Page-range retrieval metrics.

/// A retrieved or gold span: an inclusive 1-based page range of one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub doc: String,
    pub start_page: u32,
    pub end_page: u32,
}

impl Span {
    pub fn new(doc: impl Into<String>, start_page: u32, end_page: u32) -> Self {
        Self {
            doc: doc.into(),
            start_page,
            end_page,
        }
    }

    /// Number of pages in the span.
    pub fn pages(&self) -> u32 {
        self.end_page.saturating_sub(self.start_page) + 1
    }
}

/// True when both spans are in the same document and share at least one page.
pub fn page_range_overlap(a: &Span, b: &Span) -> bool {
    a.doc == b.doc && a.start_page <= b.end_page && b.start_page <= a.end_page
}

/// 1-based rank of the first ranked span overlapping `gold`.
pub fn first_hit_rank(ranked: &[Span], gold: &Span) -> Option<usize> {
    ranked
        .iter()
        .position(|s| page_range_overlap(s, gold))
        .map(|i| i + 1)
}

/// Fraction of queries whose first hit is within the top `k`.
#[derive(Debug, Clone)]
pub struct HitAtK {
    pub k: usize,
    hits: usize,
    total: usize,
}

impl HitAtK {
    pub fn new(k: usize) -> Self {
        Self {
            k,
            hits: 0,
            total: 0,
        }
    }

    /// Record one query given the rank of its first hit (if any).
    pub fn add(&mut self, rank: Option<usize>) {
        self.total += 1;
        if rank.is_some_and(|r| r <= self.k) {
            self.hits += 1;
        }
    }

    pub fn value(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.hits as f64 / self.total as f64
        }
    }
}

/// Mean reciprocal rank of the first hit; a miss contributes 0.
#[derive(Debug, Clone, Default)]
pub struct Mrr {
    sum: f64,
    total: usize,
}

impl Mrr {
    pub fn add(&mut self, rank: Option<usize>) {
        self.total += 1;
        if let Some(r) = rank {
            self.sum += 1.0 / r as f64;
        }
    }

    pub fn value(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.sum / self.total as f64
        }
    }
}
