//! Token budget + cheap pre-vector query (O5: `feat-cg-budget-query`).
//!
//! Ideas rewritten from `redhat-et/ripwire` (R3: compact bundles, shape vs
//! gate, `est_tokens` ≈ bytes/4) and `Graphify-Labs/graphify` (G6: vocab
//! expansion ≤12 tokens, BFS depth 3 vs DFS depth 6, `--budget`).
//! No upstream code. See `docs/EXTRACTION-RIPWIRE-GRAPHIFY.md` (R3, G6).
//!
//! Rules:
//! - Orientation answers carry signatures, never bodies; bodies are fetched
//!   on demand (`--expand` equivalent).
//! - Every answer declares `est_tokens`; over-budget answers gate with an
//!   explicit error, never truncate silently.
//! - Zero vocab overlap stops the query (no noise retrieval).

use crate::types::Symbol;
use std::collections::HashSet;

/// Tokenizer divisor: ~tokens ≈ bytes/4 (ripwire README measurement basis).
pub const CHARS_PER_TOKEN: usize = 4;

/// Maximum expanded query tokens (graphify `.vocab.txt` rule).
pub const MAX_QUERY_TOKENS: usize = 12;

/// Graphify traversal defaults: BFS "what is connected" vs DFS "how it reaches".
pub const BFS_DEFAULT_DEPTH: usize = 3;
pub const DFS_DEFAULT_DEPTH: usize = 6;

/// Minimum/maximum vocab token length (graphify rule: 3–30 chars).
pub const VOCAB_MIN_LEN: usize = 3;
pub const VOCAB_MAX_LEN: usize = 30;

/// Rough token estimate for a text: `ceil(bytes / 4)`, `0` for empty.
pub fn estimate_tokens(text: &str) -> u64 {
    if text.is_empty() {
        return 0;
    }
    text.len().div_ceil(CHARS_PER_TOKEN) as u64
}

/// Wire cost of one symbol row (name + signature + path + kind).
pub fn symbol_cost(sym: &Symbol) -> u64 {
    let kind = format!("{:?}", sym.kind);
    estimate_tokens(&sym.name)
        + estimate_tokens(sym.signature.as_deref().unwrap_or(""))
        + estimate_tokens(&sym.file_path)
        + estimate_tokens(&kind)
}

/// Traversal mode with graphify's depth semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalMode {
    /// "What is X connected to?" — broad, nearest first.
    Bfs,
    /// "How does X reach Y?" — one chain as deep as possible.
    Dfs,
}

impl TraversalMode {
    /// Default depth cap for the mode.
    pub fn default_depth(&self) -> usize {
        match self {
            Self::Bfs => BFS_DEFAULT_DEPTH,
            Self::Dfs => DFS_DEFAULT_DEPTH,
        }
    }
}

/// Spending gate: `check` admits a cost or refuses with the books attached.
#[derive(Debug, Clone)]
pub struct Budget {
    pub max_tokens: u64,
    pub used_tokens: u64,
}

/// Gate refusal: how much was allowed, spent, and rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetExceeded {
    pub max_tokens: u64,
    pub used_tokens: u64,
    pub rejected_cost: u64,
}

impl Budget {
    /// New gate with a hard ceiling.
    pub fn new(max_tokens: u64) -> Self {
        Self {
            max_tokens,
            used_tokens: 0,
        }
    }

    /// Admit `cost`, spending it, or refuse without spending.
    pub fn check(&mut self, cost: u64) -> Result<(), BudgetExceeded> {
        if self.used_tokens.saturating_add(cost) > self.max_tokens {
            return Err(BudgetExceeded {
                max_tokens: self.max_tokens,
                used_tokens: self.used_tokens,
                rejected_cost: cost,
            });
        }
        self.used_tokens = self.used_tokens.saturating_add(cost);
        Ok(())
    }

    /// Unspent tokens (saturates at zero).
    pub fn remaining(&self) -> u64 {
        self.max_tokens.saturating_sub(self.used_tokens)
    }
}

/// Expand a question against the repo vocabulary: lowercase alphanumeric
/// tokens kept **iff** present in `vocab`, deduplicated, order kept, capped
/// at [`MAX_QUERY_TOKENS`]. No stemming, no synonyms, no invented tokens —
/// an empty result means "stop, the corpus has no such vocabulary".
pub fn expand_query(question: &str, vocab: &HashSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let flush = |current: &mut String, out: &mut Vec<String>| {
        if !current.is_empty() {
            let tok = current.to_lowercase();
            if vocab.contains(&tok) && !out.contains(&tok) {
                out.push(tok);
            }
            current.clear();
        }
    };
    for ch in question.chars() {
        if ch.is_alphanumeric() {
            current.push(ch);
        } else {
            flush(&mut current, &mut out);
        }
        if out.len() >= MAX_QUERY_TOKENS {
            return out;
        }
    }
    flush(&mut current, &mut out);
    out.truncate(MAX_QUERY_TOKENS);
    out
}

/// Split one name into vocab tokens: full lowercase form plus snake parts
/// plus camel humps, filtered to [`VOCAB_MIN_LEN`]..=[`VOCAB_MAX_LEN`].
pub fn split_vocab_tokens(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |tok: String| {
        let tok = tok.to_lowercase();
        if tok.len() >= VOCAB_MIN_LEN && tok.len() <= VOCAB_MAX_LEN && !out.contains(&tok) {
            out.push(tok);
        }
    };
    push(name.to_string());
    // Snake parts.
    for part in name.split('_') {
        push(part.to_string());
    }
    // Camel humps: split before each uppercase letter (except the first char).
    let mut hump = String::new();
    for (i, ch) in name.char_indices() {
        if ch.is_uppercase() && i > 0 && !hump.is_empty() {
            push(hump.clone());
            hump.clear();
        }
        if ch.is_alphanumeric() {
            hump.push(ch);
        } else if !hump.is_empty() {
            push(hump.clone());
            hump.clear();
        }
    }
    if !hump.is_empty() {
        push(hump);
    }
    out
}

/// Build a vocab set from symbol names.
pub fn build_vocab(names: impl IntoIterator<Item = String>) -> HashSet<String> {
    let mut vocab = HashSet::new();
    for name in names {
        for tok in split_vocab_tokens(&name) {
            vocab.insert(tok);
        }
    }
    vocab
}

/// Compact a symbol list into a token budget: keep the head while each
/// row's [`symbol_cost`] fits; returns rows plus whether the tail was cut
/// (`capped` — always disclosed, never silent).
pub fn compact_to_budget(symbols: Vec<Symbol>, budget: &mut Budget) -> (Vec<Symbol>, bool) {
    let mut kept = Vec::new();
    for sym in symbols {
        let cost = symbol_cost(&sym);
        if budget.check(cost).is_ok() {
            kept.push(sym);
        } else {
            // First row that does not fit ends the bundle: the head stays
            // rank-ordered and the cut is disclosed, never half-fitted.
            return (kept, true);
        }
    }
    (kept, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Language, SymbolKind};

    fn sym(name: &str) -> Symbol {
        Symbol {
            name: name.to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "src/lib.rs".to_string(),
            start_line: 1,
            end_line: 5,
            signature: Some(format!("pub fn {}()", name)),
            ..Default::default()
        }
    }

    #[test]
    fn test_estimate_tokens_bytes_over_four() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn test_budget_gate_triggers_over_limit() {
        let mut b = Budget::new(10);
        assert!(b.check(6).is_ok());
        assert_eq!(b.remaining(), 4);
        let err = b.check(5).expect_err("over budget must refuse");
        assert_eq!(
            err,
            BudgetExceeded {
                max_tokens: 10,
                used_tokens: 6,
                rejected_cost: 5,
            }
        );
        // Refusals spend nothing.
        assert_eq!(b.remaining(), 4);
    }

    #[test]
    fn test_traversal_mode_default_depths() {
        assert_eq!(TraversalMode::Bfs.default_depth(), 3);
        assert_eq!(TraversalMode::Dfs.default_depth(), 6);
    }

    #[test]
    fn test_vocab_expansion_empty_query_stops() {
        let vocab = build_vocab(vec!["authentication".to_string(), "handler".to_string()]);
        // No shared vocabulary: the caller must stop, not retrieve noise.
        let got = expand_query("¿cómo funciona esto?", &vocab);
        assert!(got.is_empty());
        let got = expand_query("where is the Authentication HANDLER?", &vocab);
        assert_eq!(
            got,
            vec!["authentication".to_string(), "handler".to_string()]
        );
    }

    #[test]
    fn test_vocab_split_covers_snake_and_camel() {
        let vocab = build_vocab(vec![
            "require_permission".to_string(),
            "UserService".to_string(),
        ]);
        for tok in ["require", "permission", "user", "service"] {
            assert!(vocab.contains(tok), "missing {tok}");
        }
    }

    #[test]
    fn test_compact_to_budget_caps_and_reports() {
        let rows = vec![sym("a"), sym("bb"), sym("ccc")];
        let total: u64 = rows.iter().map(symbol_cost).sum();
        // Generous budget: everything fits, no cap.
        let mut big = Budget::new(total + 100);
        let (kept, capped) = compact_to_budget(rows.clone(), &mut big);
        assert_eq!(kept.len(), 3);
        assert!(!capped);
        // Tight budget: head fits, tail cut, cap disclosed.
        let first = symbol_cost(&rows[0]);
        let mut tight = Budget::new(first);
        let (kept, capped) = compact_to_budget(rows, &mut tight);
        assert_eq!(kept.len(), 1);
        assert!(capped);
    }
}
