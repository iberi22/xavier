//! Unified confidence rubric + labeled honesty (O1: `feat-cg-honest-confidence`).
//!
//! Ideas rewritten from `redhat-et/ripwire` (floors, caps, refuse-vs-empty,
//! `METHODOLOGY.md` honesty rules) and `Graphify-Labs/graphify`
//! (`how-it-works.md#Confidence tagging` discrete rubric). No upstream code.
//! See `docs/EXTRACTION-RIPWIRE-GRAPHIFY.md` (R1, G1) and ADR-032.
//!
//! Rules:
//! - `EXTRACTED = 1.0`: explicit in source (import, definition, exact call).
//! - `INFERRED ∈ {0.95, 0.85, 0.75, 0.65, 0.55}`: evidence tiers, never `0.5`.
//! - `AMBUGUOUS < 0.4`: uncertain, surfaced — never omitted, never a silent zero.
//! - Counts that cannot be totals are labeled `floor`; `0` means "none found".

/// Discrete INFERRED rungs plus the EXTRACTED top (Graphify rubric).
pub const RUBRIC_RUNGS: [f32; 6] = [1.0, 0.95, 0.85, 0.75, 0.65, 0.55];

/// Scores strictly below this ceiling are `Ambiguous`, never `Inferred`.
pub const AMBIGUOUS_CEILING: f32 = 0.4;

/// Unified confidence label for any scored graph fact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Confidence {
    /// Explicit in source. Always `1.0`.
    Extracted,
    /// Reasonable inference pinned to a discrete rubric rung.
    Inferred { score: f32 },
    /// Uncertain. Surfaced for review, never silently dropped.
    Ambiguous { score: f32 },
}

impl Confidence {
    /// Classify a raw score into the rubric. Snaps to the nearest rung,
    /// ties rounding down (conservative); anything below
    /// [`AMBIGUOUS_CEILING`] is `Ambiguous`.
    pub fn classify(score: f32) -> Self {
        if score >= 1.0 {
            return Self::Extracted;
        }
        if score < AMBIGUOUS_CEILING {
            return Self::Ambiguous { score };
        }
        Self::Inferred {
            score: normalize_score(score),
        }
    }

    /// Numeric value of the label (`1.0` for `Extracted`).
    pub fn value(&self) -> f32 {
        match *self {
            Self::Extracted => 1.0,
            Self::Inferred { score } | Self::Ambiguous { score } => score,
        }
    }

    /// Wire tag: `"EXTRACTED"`, `"INFERRED"` or `"AMBIGUOUS"`.
    pub fn tag(&self) -> &'static str {
        match *self {
            Self::Extracted => "EXTRACTED",
            Self::Inferred { .. } => "INFERRED",
            Self::Ambiguous { .. } => "AMBIGUOUS",
        }
    }
}

/// Snap a legacy free-form score onto the discrete rubric.
///
/// Documented backfill mapping (ADR-032): `ImportMap 0.95→0.95`,
/// `SameModule 0.90→0.85` (tie goes down), `ImportMapSuffix 0.85→0.85`,
/// `Imports 0.80→0.75` (tie goes down), `UniqueName 0.75→0.75`,
/// `SuffixMatch 0.55→0.55`, `Fuzzy ≤0.40→Ambiguous`.
pub fn normalize_score(score: f32) -> f32 {
    if score >= 1.0 {
        return 1.0;
    }
    // Walk rungs top-down: a score lands on the first rung whose midpoint
    // boundary it strictly exceeds, so exact midpoints fall DOWN
    // (conservative tie-break, ADR-032).
    for pair in RUBRIC_RUNGS.windows(2) {
        let (hi, lo) = (pair[0], pair[1]);
        if score > (hi + lo) / 2.0 {
            return hi;
        }
    }
    *RUBRIC_RUNGS.last().unwrap_or(&0.55)
}

/// Honesty metadata every graph answer must carry (ripwire R1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseMeta {
    /// Total rows known (may itself be a floor when `floor` is set).
    pub total: usize,
    /// Rows actually returned.
    pub shown: usize,
    /// True when the answer was cut to fit (`shown < total`).
    pub capped: bool,
    /// True when counts are a lower bound, never a total ("none found" ≠ "none exists").
    pub floor: bool,
    /// Number of ambiguous (guessed) edges in the answer.
    pub amb: u32,
    /// File extensions skipped during indexing (unindexed languages).
    pub unindexed: Vec<String>,
    /// O5: declared wire cost in tokens (`est_tokens` ≈ bytes/4).
    /// `None` means "not measured" — never silently zero.
    pub est_tokens: Option<u64>,
}

impl ResponseMeta {
    /// Build metadata; `capped` is derived (`shown < total`), never set by hand.
    pub fn new(total: usize, shown: usize, floor: bool) -> Self {
        let shown = shown.min(total);
        Self {
            total,
            shown,
            capped: shown < total,
            floor,
            amb: 0,
            unindexed: Vec::new(),
            est_tokens: None,
        }
    }

    /// The truncation invariant: `shown <= total` and `capped == (shown < total)`.
    pub fn invariant_holds(&self) -> bool {
        self.shown <= self.total && self.capped == (self.shown < self.total)
    }
}

/// Did-you-mean over known names (edit similarity > 0.8, mirroring the
/// `CallResolver` fuzzy bar). Returns at most `max` suggestions, best first.
/// An empty candidate pool yields an empty list — never a guess.
pub fn suggest_similar(name: &str, candidates: &[String], max: usize) -> Vec<String> {
    if max == 0 || candidates.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, &String)> = candidates
        .iter()
        .map(|c| (edit_distance(name, c), c))
        .filter(|(dist, c)| {
            let max_len = name.len().max(c.len()) as f32;
            if max_len == 0.0 {
                return false;
            }
            // Same bar as CallResolver fuzzy: similarity > 0.8.
            1.0 - (*dist as f32 / max_len) > 0.8
        })
        .collect();
    scored.sort_by_key(|(dist, _)| *dist);
    scored
        .into_iter()
        .take(max)
        .map(|(_, c)| c.clone())
        .collect()
}

/// Plain Levenshtein distance over chars (local copy so the query layer
/// does not depend on indexer internals).
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            curr[j + 1] = (prev[j] + usize::from(ca != cb))
                .min(prev[j + 1] + 1)
                .min(curr[j] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_means_none_found_not_none_exists() {
        // A score of 0.0 carries no information: it must classify as
        // Ambiguous (unknown), never as a confident "nothing exists".
        let c = Confidence::classify(0.0);
        assert_eq!(c.tag(), "AMBIGUOUS");
        assert!(c.value() < AMBIGUOUS_CEILING);
    }

    #[test]
    fn test_confidence_rubric_mapping() {
        assert_eq!(Confidence::classify(1.0), Confidence::Extracted);
        assert_eq!(Confidence::classify(0.95).value(), 0.95);
        // Tie goes down: 0.90 (legacy SameModule) snaps to 0.85, not 0.95.
        assert_eq!(Confidence::classify(0.90).value(), 0.85);
        // Imports 0.80 snaps down to 0.75.
        assert_eq!(Confidence::classify(0.80).value(), 0.75);
        assert_eq!(Confidence::classify(0.75).value(), 0.75);
        assert_eq!(Confidence::classify(0.55).value(), 0.55);
        // Fuzzy leftovers fall below the ceiling.
        assert_eq!(Confidence::classify(0.38).tag(), "AMBIGUOUS");
    }

    #[test]
    fn test_truncation_discloses_caps() {
        let full = ResponseMeta::new(10, 10, false);
        assert!(!full.capped);
        assert!(full.invariant_holds());

        let cut = ResponseMeta::new(10, 4, false);
        assert!(cut.capped);
        assert!(cut.invariant_holds());
    }

    #[test]
    fn test_counts_carry_floor_flag() {
        // Dynamic dispatch / macros / callbacks are invisible to the index:
        // such counts are floors, and the flag must survive construction.
        let m = ResponseMeta::new(3, 3, true);
        assert!(m.floor);
        assert!(!m.capped);
        assert!(m.invariant_holds());
    }

    #[test]
    fn test_suggest_similar_ranks_best_first() {
        let pool = vec![
            "require_permission".to_string(),
            "require_permision".to_string(),
            "delete_memory_handler".to_string(),
        ];
        let got = suggest_similar("require_permisson", &pool, 3);
        assert!(!got.is_empty());
        assert_eq!(got[0], "require_permission");
        assert!(!got.contains(&"delete_memory_handler".to_string()));
    }

    #[test]
    fn test_suggest_similar_empty_pool_never_guesses() {
        let got = suggest_similar("anything", &[], 3);
        assert!(got.is_empty());
    }
}
