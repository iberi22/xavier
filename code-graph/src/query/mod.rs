//! Query engine for code graph

pub mod tests;

use crate::confidence::{suggest_similar, Confidence, ResponseMeta};
use crate::db::CodeGraphDB;
use crate::error::{GraphError, Result};
use crate::types::{
    CodeEdge, ComplexityHotspot, EdgeType, HubNode, MemorySymbolLink, QueryResult, Symbol,
    SymbolEmbedder, SymbolKind,
};
use std::collections::HashMap;
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// O6 (ripwire R4): a symbol with its structural centrality score.
#[derive(Debug, Clone)]
pub struct RankedSymbol {
    pub symbol: Symbol,
    pub score: f64,
}

/// O6 router verdict over a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteMode {
    /// Exact name hit: answered directly.
    ExactName,
    /// No exact hit: FTS candidates ordered by graph centrality.
    GraphRanked,
}

/// O6 (US-111/112): routed orientation answer with margin verdict and
/// declared cost. `confident == false` reads as a starting point.
#[derive(Debug, Clone)]
pub struct RouteReport {
    pub hits: Vec<RankedSymbol>,
    pub mode: RouteMode,
    pub margin_pct: f64,
    pub confident: bool,
    pub meta: ResponseMeta,
}

/// O7 (ripwire R5 `--edit-check`): contract comparison outcome. No
/// `Compatible` verdict exists — absence of detected change is not proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractStatus {
    /// Same identity and fingerprint.
    Unchanged,
    /// Different name/kind: a different contract altogether.
    NewSymbol,
    /// Same identity, different fingerprint with both signatures attached.
    ContractChange {
        was: Option<String>,
        now: Option<String>,
    },
}

/// O7 (ripwire R5 `--safe-delete`): named risk, never a go/no-go verdict.
/// There is deliberately no `Safe`/`Unsafe` variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteRisk {
    /// Nothing references the symbol.
    NoneFound,
    /// Referenced and every reach is test-covered.
    UsesExist,
    /// Some reach has no covering test.
    UntestedRadius,
}

/// O7: safe-delete composition (callers + reach + coverage).
#[derive(Debug, Clone)]
pub struct DeleteReport {
    pub target: Symbol,
    /// Transitive reach size (blast).
    pub impact_reaches: usize,
    /// Direct (depth-1) references by role.
    pub uses: Vec<BlastHit>,
    /// Whether any indexed test exercises the target itself.
    pub tested_self: bool,
    pub radius_tested: usize,
    pub radius_untested: usize,
    pub risk: DeleteRisk,
}

/// O7 (ripwire R5 `--verify`): three-valued claim verdict with evidence.
/// A graph zero never refutes — only a complete scan does.
#[derive(Debug, Clone)]
pub enum VerifyVerdict {
    /// Proven, with `file:line` (or equivalent) evidence attached.
    Confirmed { evidence: String },
    /// Disproven by complete evidence (`complete` is always true here;
    /// partial evidence yields `NotEstablished` instead).
    Refuted { complete: bool },
    /// The index cannot prove it; `limit` names the floor.
    NotEstablished { limit: String },
}

/// O7 (G7 rationale): a `NOTE:`/`WHY:`/`HACK:` comment node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RationaleKind {
    Note,
    Why,
    Hack,
}

/// O7: rationale tag with best-effort symbol link.
#[derive(Debug, Clone)]
pub struct RationaleTag {
    pub file: String,
    pub line: u32,
    pub tag: RationaleKind,
    pub text: String,
    /// Nearest symbol at or above the tag line, if any.
    pub symbol: Option<String>,
}

/// O3 (`feat-cg-blast-testgate`, ripwire R2): one reach of a blast radius.
///
/// `edge` names the role of the reference (call/read separates by edge
/// kind, never by guessing data-flow a call graph cannot see); `confidence`
/// classifies the edge weight on the O1 rubric.
#[derive(Debug, Clone)]
pub struct BlastHit {
    pub symbol: Symbol,
    pub depth: usize,
    pub edge: EdgeType,
    pub confidence: Confidence,
}

/// O3: transitive reach set plus honesty metadata. `meta.floor` is always
/// true — dynamic dispatch, callbacks and macros are invisible to the index.
#[derive(Debug, Clone)]
pub struct BlastReport {
    pub hits: Vec<BlastHit>,
    pub meta: ResponseMeta,
}

/// O3 (ripwire R2 `--test-gate`): tests that exercise the change plus the
/// non-test reach no test covers. `run` commands are deliberately absent:
/// runner derivation is CLI-level and never invented here.
#[derive(Debug, Clone, Default)]
pub struct TestGate {
    /// Changed symbols as resolved (seeds).
    pub changed: Vec<Symbol>,
    /// Covering tests (union), carrying their real indexed file paths.
    pub tests_to_run: Vec<Symbol>,
    /// Non-test reach no indexed test covers (informational).
    pub untested: Vec<BlastHit>,
    /// Changed symbols no indexed test exercises — the gate obligation.
    pub uncovered_changed: Vec<Symbol>,
}

impl TestGate {
    /// True when some changed symbol lacks test coverage (gate must fail).
    ///
    /// Scoped to seeds on purpose (deviation from ripwire's whole-radius
    /// gate, documented in the O3 spec): it answers "does a test exercise
    /// my change?" Whole-radius gating arrives with CLI situ baselines.
    pub fn has_obligation(&self) -> bool {
        !self.uncovered_changed.is_empty()
    }
}

/// O3: a symbol counts as test code by path (`test(s)/` segment, `test_*`
/// / `*_test` stem) or by `test_*` name. Conservative by design: unknown
/// harnesses are plain code until proven test (honesty over recall).
pub fn is_test_symbol(sym: &Symbol) -> bool {
    if sym.name.starts_with("test_") {
        return true;
    }
    let path = sym.file_path.replace('\\', "/");
    if path.split('/').any(|seg| seg == "test" || seg == "tests") {
        return true;
    }
    let file = path.rsplit('/').next().unwrap_or("");
    let stem = file.rsplit('.').nth(1).unwrap_or(file);
    stem.starts_with("test_") || stem.ends_with("_test")
}

/// Edge kinds that count as "reaching" for blast purposes. Structural
/// container edges (`Contains`, `Defines`, …) are excluded; file/module
/// pseudo-nodes are skipped at walk time like in [`QueryEngine::blast_radius`].
const BLAST_EDGE_TYPES: [EdgeType; 6] = [
    EdgeType::Calls,
    EdgeType::Uses,
    EdgeType::References,
    EdgeType::Imports,
    EdgeType::Extends,
    EdgeType::Implements,
];

/// O7: contract fingerprint — kind + whitespace-erased signature + arity.
/// Whitespace-insensitive on purpose: formatting is not contract.
pub fn contract_fingerprint(sym: &Symbol) -> String {
    let sig: String = sym
        .signature
        .as_deref()
        .unwrap_or("")
        .split_whitespace()
        .collect();
    format!(
        "{:?}|{}|{}",
        sym.kind,
        sig,
        signature_arity(sym.signature.as_deref().unwrap_or(""))
    )
}

/// Arity of the first parenthesized group (0 when absent/empty).
fn signature_arity(sig: &str) -> usize {
    let start = match sig.find('(') {
        Some(i) => i + 1,
        None => return 0,
    };
    let mut depth = 1usize;
    let mut commas = 0usize;
    let mut any = false;
    for ch in sig[start..].chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            ',' if depth == 1 => commas += 1,
            c if !c.is_whitespace() => {
                let _ = c;
                any = true;
            }
            _ => {}
        }
    }
    if !any {
        0
    } else {
        commas + 1
    }
}

/// O7 (ripwire R5 `--edit-check` core): compare two versions of one symbol.
/// Different name/kind means different identity (`NewSymbol`); anything
/// subtler must show both signatures (`ContractChange`).
pub fn compare_contract(old: &Symbol, new: &Symbol) -> ContractStatus {
    if old.name != new.name || old.kind != new.kind {
        return ContractStatus::NewSymbol;
    }
    if contract_fingerprint(old) == contract_fingerprint(new) {
        ContractStatus::Unchanged
    } else {
        ContractStatus::ContractChange {
            was: old.signature.clone(),
            now: new.signature.clone(),
        }
    }
}

/// Resolve a stable id to its file; pseudo-nodes yield `None`.
fn file_of_symbol(db: &CodeGraphDB, stable_id: &str) -> Result<Option<String>> {
    Ok(db.symbol_by_stable_id(stable_id)?.map(|s| s.file_path))
}

/// Provenance of a cycle edge resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleEdgeProvenance {
    /// Resolved via typed module import resolution (ImportMap / ImportMapSuffix strategy).
    TypedImport,
    /// Co-location / bare file-level matching fallback.
    FileFallback,
}

/// A simple cycle with edge provenance disclosure (Section B honest disclosure).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleReport {
    pub cycle: Vec<String>,
    pub provenance: CycleEdgeProvenance,
}

/// Dogfood ceiling (review 2026-09-18): dense file graphs enumerate
/// hundreds of thousands of simple cycles. Output is capped, shortest
/// first, deterministically — budgets beat completeness here (O5).
pub const MAX_CYCLES: usize = 512;

/// Bounded simple-cycle DFS over the file graph.
#[allow(clippy::too_many_arguments)]
fn dfs_cycles(
    adj: &std::collections::HashMap<String, Vec<String>>,
    start: &str,
    current: &str,
    max_len: usize,
    cap: usize,
    path: &mut Vec<String>,
    seen: &mut std::collections::HashSet<Vec<String>>,
    cycles: &mut Vec<Vec<String>>,
) {
    if path.len() > max_len || cycles.len() >= cap {
        return;
    }
    let Some(targets) = adj.get(current) else {
        return;
    };
    for next in targets {
        if cycles.len() >= cap {
            break;
        }
        if next == start && path.len() >= 2 {
            let mut cycle = path.clone();
            let min_pos = cycle
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.cmp(b.1))
                .map(|(i, _)| i)
                .unwrap_or(0);
            cycle.rotate_left(min_pos);
            if seen.insert(cycle.clone()) {
                cycles.push(cycle);
            }
        } else if !path.contains(next) && path.len() < max_len {
            path.push(next.clone());
            dfs_cycles(adj, start, next, max_len, cap, path, seen, cycles);
            path.pop();
        }
    }
}

/// O7 (G7 rationale): full-line `NOTE:`/`WHY:`/`HACK:` markers after a `#`
/// or `//` comment prefix (case-sensitive, graphify convention). Trailing
/// inline comments are ignored on purpose: attribution must be explicit.
pub fn extract_rationale(source: &str, file: &str) -> Vec<RationaleTag> {
    const MARKERS: [(&str, RationaleKind); 3] = [
        ("NOTE:", RationaleKind::Note),
        ("WHY:", RationaleKind::Why),
        ("HACK:", RationaleKind::Hack),
    ];
    let mut out = Vec::new();
    for (idx, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        let body = trimmed
            .strip_prefix('#')
            .or_else(|| trimmed.strip_prefix("//"))
            .map(str::trim_start)
            .unwrap_or(trimmed);
        // Only full-line markers: the body must START with the marker.
        let is_code = !(trimmed.starts_with('#') || trimmed.starts_with("//"));
        if is_code {
            continue;
        }
        for (marker, kind) in MARKERS {
            if let Some(rest) = body.strip_prefix(marker) {
                out.push(RationaleTag {
                    file: file.to_string(),
                    line: (idx + 1) as u32,
                    tag: kind,
                    text: rest.trim().to_string(),
                    symbol: None,
                });
                break;
            }
        }
    }
    out
}

/// Simple in-memory cache for query results
pub struct QueryCache {
    cache: RwLock<HashMap<String, (Instant, QueryResult)>>,
    ttl: Duration,
    max_entries: usize,
}

impl QueryCache {
    pub fn new(ttl_secs: u64, max_entries: usize) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_secs),
            max_entries,
        }
    }

    /// Get cached result if still valid
    pub fn get(&self, query: &str) -> Option<QueryResult> {
        let cache = self.cache.read().expect("RwLock not poisoned");
        cache.get(query).and_then(|(time, result)| {
            if time.elapsed() < self.ttl {
                Some(result.clone())
            } else {
                None
            }
        })
    }

    /// Store result in cache
    pub fn set(&self, query: String, result: QueryResult) {
        let mut cache = self.cache.write().expect("RwLock not poisoned");

        // Evict old entries if at capacity
        if cache.len() >= self.max_entries {
            let now = Instant::now();
            cache.retain(|_, (time, _)| now.duration_since(*time) < self.ttl);

            // If still at capacity, remove oldest
            if cache.len() >= self.max_entries {
                if let Some(oldest) = cache
                    .iter()
                    .min_by_key(|(_, (time, _))| *time)
                    .map(|(k, _)| k.clone())
                {
                    cache.remove(&oldest);
                }
            }
        }

        cache.insert(query, (Instant::now(), result));
    }

    /// Clear all cached entries
    pub fn clear(&self) {
        self.cache.write().expect("RwLock not poisoned").clear();
    }

    /// Get cache statistics
    pub fn stats(&self) -> (usize, usize) {
        let cache = self.cache.read().expect("RwLock not poisoned");
        let valid = cache
            .iter()
            .filter(|(_, (time, _))| time.elapsed() < self.ttl)
            .count();
        (valid, cache.len())
    }
}

pub struct QueryEngine {
    db: Arc<CodeGraphDB>,
    cache: Option<Arc<QueryCache>>,
}

impl QueryEngine {
    pub fn new(db: Arc<CodeGraphDB>) -> Self {
        Self { db, cache: None }
    }

    /// Create with cache
    pub fn with_cache(db: Arc<CodeGraphDB>, ttl_secs: u64, max_entries: usize) -> Self {
        Self {
            db,
            cache: Some(Arc::new(QueryCache::new(ttl_secs, max_entries))),
        }
    }

    /// Search for symbols by name (with caching and strict narrow filter)
    pub fn search(&self, query: &str, limit: usize) -> Result<QueryResult> {
        // Try cache first
        if let Some(ref cache) = self.cache {
            if let Some(result) = cache.get(query) {
                return Ok(result);
            }
        }

        // Query database (over-fetch candidates from DB FTS / LIKE)
        let fetch_limit = if query.trim().is_empty() {
            limit
        } else {
            (limit * 10).max(100)
        };
        let mut result = self.db.find_symbols(query, fetch_limit)?;

        // Narrow filter gate: filter WHERE name LIKE '%query%' COLLATE NOCASE
        // or matching stable_id to drop generic false positives (e.g. imports)
        let q_trimmed = query.trim();
        if !q_trimmed.is_empty() {
            let q_lower = q_trimmed.to_lowercase();
            result.symbols.retain(|sym| {
                let name_matches = sym.name.to_lowercase().contains(&q_lower);
                let id_matches = sym
                    .stable_id
                    .as_deref()
                    .is_some_and(|id| id.to_lowercase().contains(&q_lower));
                name_matches || id_matches
            });
            result.symbols.truncate(limit);
            result.total = result.symbols.len();
        }

        // Store in cache
        if let Some(ref cache) = self.cache {
            cache.set(query.to_string(), result.clone());
        }

        Ok(result)
    }

    /// Find symbols by exact name match
    pub fn find_by_name(&self, name: &str, limit: usize) -> Result<Vec<Symbol>> {
        self.db.find_by_name(name, limit)
    }

    /// Find all functions
    pub fn functions(&self, limit: usize) -> Result<Vec<Symbol>> {
        self.db.find_by_kind(SymbolKind::Function, limit)
    }

    /// Find all structs
    pub fn structs(&self, limit: usize) -> Result<Vec<Symbol>> {
        self.db.find_by_kind(SymbolKind::Struct, limit)
    }

    /// Find all classes
    pub fn classes(&self, limit: usize) -> Result<Vec<Symbol>> {
        self.db.find_by_kind(SymbolKind::Class, limit)
    }

    /// Search by AST pattern (tree-sitter based)
    /// Supported patterns: "function_call", "struct_definition", "import", "method"
    pub fn search_by_pattern(&self, pattern: &str, limit: usize) -> Result<Vec<Symbol>> {
        // Map AST patterns to symbol kinds
        let kind = match pattern {
            "function_call" | "function_definition" => SymbolKind::Function,
            "struct_definition" | "struct" => SymbolKind::Struct,
            "class_definition" | "class" => SymbolKind::Class,
            "enum_definition" | "enum" => SymbolKind::Enum,
            "route_definition" | "route" | "http_route" => SymbolKind::Route,
            "module_definition" | "module" => SymbolKind::Module,
            "import" | "use_statement" => SymbolKind::Module, // Treat imports as modules
            _ => return Ok(vec![]),
        };

        self.db.find_by_kind(kind, limit)
    }

    /// Find all enums
    pub fn enums(&self, limit: usize) -> Result<Vec<Symbol>> {
        self.db.find_by_kind(SymbolKind::Enum, limit)
    }

    /// Find all HTTP routes
    pub fn routes(&self, limit: usize) -> Result<Vec<Symbol>> {
        self.db.find_by_kind(SymbolKind::Route, limit)
    }

    pub fn dependencies(
        &self,
        query: &str,
        edge_type: Option<EdgeType>,
        depth: usize,
        limit: usize,
    ) -> Result<Vec<CodeEdge>> {
        self.traverse(query, edge_type, depth, limit, false)
    }

    pub fn reverse_dependencies(
        &self,
        query: &str,
        edge_type: Option<EdgeType>,
        depth: usize,
        limit: usize,
    ) -> Result<Vec<CodeEdge>> {
        self.traverse(query, edge_type, depth, limit, true)
    }

    pub fn call_chain(&self, query: &str, depth: usize, limit: usize) -> Result<Vec<CodeEdge>> {
        self.dependencies(query, Some(EdgeType::Calls), depth, limit)
    }

    /// Calculate blast radius of a symbol using BFS on incoming Calls edges.
    ///
    /// Returns a list of tuple `(Symbol, usize)` where `usize` is the depth
    /// (1 for direct callers, 2 for callers of direct callers, etc.) up to `max_depth`.
    pub fn blast_radius(
        &self,
        symbol_name: &str,
        max_depth: usize,
    ) -> Result<Vec<(Symbol, usize)>> {
        let max_depth = max_depth.clamp(1, 8);

        let start_ids = self.resolve_start_ids(symbol_name)?;

        let mut visited: HashSet<String> = start_ids.iter().cloned().collect();
        let mut queue: VecDeque<(String, usize)> =
            start_ids.into_iter().map(|id| (id, 0)).collect();
        let mut results = Vec::new();

        while let Some((curr_id, curr_depth)) = queue.pop_front() {
            if curr_depth >= max_depth {
                continue;
            }

            let edges = self
                .db
                .find_edges_to(&curr_id, Some(EdgeType::Calls), 1000)?;
            for edge in edges {
                let caller_id = edge.from_symbol;
                if caller_id.starts_with("file:") || caller_id.starts_with("module:") {
                    continue;
                }
                if visited.insert(caller_id.clone()) {
                    let next_depth = curr_depth + 1;
                    if let Some(sym) = self.db.symbol_by_stable_id(&caller_id)? {
                        results.push((sym, next_depth));
                        queue.push_back((caller_id, next_depth));
                    }
                }
            }
        }

        Ok(results)
    }

    /// Resolve a selector to seed stable IDs, refusing unknown selectors
    /// with suggestions (O1 honesty) instead of a silent empty set.
    fn resolve_start_ids(&self, symbol_name: &str) -> Result<Vec<String>> {
        let mut start_ids = Vec::new();
        if symbol_name.len() == 64 && symbol_name.chars().all(|ch| ch.is_ascii_hexdigit()) {
            if let Some(sym) = self.db.symbol_by_stable_id(symbol_name)? {
                if let Some(id) = sym.stable_id {
                    start_ids.push(id);
                }
            }
        } else {
            let matches = self.db.find_by_name(symbol_name, 10)?;
            let matches = if matches.is_empty() {
                self.db.find_symbols(symbol_name, 10)?.symbols
            } else {
                matches
            };
            for sym in matches {
                if let Some(id) = sym.stable_id {
                    if !start_ids.contains(&id) {
                        start_ids.push(id);
                    }
                }
            }
        }

        if start_ids.is_empty() {
            // O1 honesty (US-101): refuse with suggestions instead of a
            // silent empty vec. Zero means "none found", never "none exists".
            let token = symbol_name
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|t| t.len() >= 3)
                .max_by_key(|t| t.len())
                .unwrap_or(symbol_name);
            // Prefix backoff: FTS matches `"probe"*`, so truncating a
            // mistyped token eventually prefix-matches the intended name.
            let mut pool: Vec<String> = Vec::new();
            let mut probe = token.to_string();
            while pool.is_empty() && probe.len() >= 4 {
                pool = self
                    .db
                    .find_symbols(&probe, 10)
                    .map(|r| r.symbols.into_iter().map(|s| s.name).collect())
                    .unwrap_or_default();
                probe.pop();
            }
            return Err(GraphError::UnknownSymbol {
                name: symbol_name.to_string(),
                suggestions: suggest_similar(symbol_name, &pool, 5),
            });
        }
        Ok(start_ids)
    }

    /// O3 (US-105): transitive reach with per-site roles and O1 confidence.
    ///
    /// Walks incoming reference edges ([`BLAST_EDGE_TYPES`]), not just
    /// `Calls`: a struct read or a header import never calls, but still
    /// breaks. Unknown selectors refuse like [`QueryEngine::blast_radius`].
    pub fn blast_radius_ex(&self, symbol_name: &str, max_depth: usize) -> Result<BlastReport> {
        let max_depth = max_depth.clamp(1, 8);
        let start_ids = self.resolve_start_ids(symbol_name)?;

        let mut visited: HashSet<String> = start_ids.iter().cloned().collect();
        let mut queue: VecDeque<(String, usize)> =
            start_ids.into_iter().map(|id| (id, 0)).collect();
        let mut hits = Vec::new();

        while let Some((curr_id, curr_depth)) = queue.pop_front() {
            if curr_depth >= max_depth {
                continue;
            }
            let edges = self.db.find_edges_to(&curr_id, None, 1000)?;
            for edge in edges {
                if !BLAST_EDGE_TYPES.contains(&edge.edge_type) {
                    continue;
                }
                let caller_id = edge.from_symbol;
                if caller_id.starts_with("file:") || caller_id.starts_with("module:") {
                    continue;
                }
                if visited.insert(caller_id.clone()) {
                    let next_depth = curr_depth + 1;
                    if let Some(sym) = self.db.symbol_by_stable_id(&caller_id)? {
                        hits.push(BlastHit {
                            symbol: sym,
                            depth: next_depth,
                            edge: edge.edge_type.clone(),
                            confidence: Confidence::classify(edge.confidence),
                        });
                        queue.push_back((caller_id, next_depth));
                    }
                }
            }
        }

        let amb = hits
            .iter()
            .filter(|h| matches!(h.confidence, Confidence::Ambiguous { .. }))
            .count() as u32;
        let mut meta = ResponseMeta::new(hits.len(), hits.len(), true);
        meta.amb = amb;
        // O5: every answer declares its wire cost.
        meta.est_tokens = Some(
            hits.iter()
                .map(|h| {
                    crate::budget::symbol_cost(&h.symbol)
                        + crate::budget::estimate_tokens(&format!("{:?}", h.edge))
                })
                .sum(),
        );
        Ok(BlastReport { hits, meta })
    }

    /// O3 (ripwire R2 `--affected`): indexed tests reaching `symbol_name`.
    pub fn tests_for(&self, symbol_name: &str, max_depth: usize) -> Result<Vec<Symbol>> {
        let report = self.blast_radius_ex(symbol_name, max_depth)?;
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for hit in report.hits {
            if !is_test_symbol(&hit.symbol) {
                continue;
            }
            let key = hit
                .symbol
                .stable_id
                .clone()
                .unwrap_or_else(|| hit.symbol.name.clone());
            if seen.insert(key) {
                out.push(hit.symbol);
            }
        }
        Ok(out)
    }

    /// O3 (US-106, ripwire R2 `--test-gate`): coverage gate over changed symbols.
    ///
    /// Cost is one BFS per radius member; documented, not hidden — gates run
    /// on diffs, not on whole repos.
    pub fn test_gate(&self, symbol_names: &[&str], max_depth: usize) -> Result<TestGate> {
        let mut gate = TestGate::default();
        let mut seen_tests = HashSet::new();
        let mut seen_untested = HashSet::new();
        for name in symbol_names {
            let seeds = self.resolve_start_ids(name)?;
            for id in &seeds {
                if let Some(sym) = self.db.symbol_by_stable_id(id)? {
                    gate.changed.push(sym);
                }
            }
            for test in self.tests_for(name, max_depth)? {
                let key = test.stable_id.clone().unwrap_or_else(|| test.name.clone());
                if seen_tests.insert(key) {
                    gate.tests_to_run.push(test);
                }
            }
            if self.tests_for(name, max_depth)?.is_empty() {
                let seed_names: Vec<&str> = gate.changed.iter().map(|s| s.name.as_str()).collect();
                if seed_names.contains(name) {
                    gate.uncovered_changed.extend(
                        gate.changed
                            .iter()
                            .filter(|s| s.name.as_str() == *name)
                            .cloned(),
                    );
                }
            }
            let report = self.blast_radius_ex(name, max_depth)?;
            for hit in report.hits {
                if is_test_symbol(&hit.symbol) {
                    continue;
                }
                if !self.tests_for(&hit.symbol.name, max_depth)?.is_empty() {
                    continue;
                }
                let key = hit
                    .symbol
                    .stable_id
                    .clone()
                    .unwrap_or_else(|| hit.symbol.name.clone());
                if seen_untested.insert(key) {
                    gate.untested.push(hit);
                }
            }
        }
        Ok(gate)
    }

    /// O3 situ core: symbols defined in the given files (feeds `git diff`
    /// file lists at the CLI layer).
    pub fn symbols_for_files(&self, files: &[&str]) -> Result<Vec<Symbol>> {
        let mut out = Vec::new();
        for file in files {
            out.extend(self.db.symbols_in_file(file)?);
        }
        Ok(out)
    }

    /// O7 (US-113): safe-delete composition — callers, transitive reach,
    /// per-hit coverage — with a named risk and deliberately no verdict.
    pub fn safe_delete(&self, symbol_name: &str, max_depth: usize) -> Result<DeleteReport> {
        let seeds = self.resolve_start_ids(symbol_name)?;
        let target = seeds
            .iter()
            .filter_map(|id| self.db.symbol_by_stable_id(id).ok().flatten())
            .next()
            .ok_or_else(|| GraphError::Query(format!("no symbol for '{symbol_name}'")))?;
        let report = self.blast_radius_ex(symbol_name, max_depth)?;
        let uses: Vec<BlastHit> = report
            .hits
            .iter()
            .filter(|h| h.depth == 1)
            .cloned()
            .collect();
        let mut radius_tested = 0usize;
        let mut radius_untested = 0usize;
        for hit in &report.hits {
            if self.tests_for(&hit.symbol.name, max_depth)?.is_empty() {
                radius_untested += 1;
            } else {
                radius_tested += 1;
            }
        }
        let tested_self = !self.tests_for(symbol_name, max_depth)?.is_empty();
        let risk = if report.hits.is_empty() {
            DeleteRisk::NoneFound
        } else if radius_untested > 0 {
            DeleteRisk::UntestedRadius
        } else {
            DeleteRisk::UsesExist
        };
        Ok(DeleteReport {
            target,
            impact_reaches: report.hits.len(),
            uses,
            tested_self,
            radius_tested,
            radius_untested,
            risk,
        })
    }

    /// O7 (ripwire R5 `--verify`): does `caller` call `callee`? A stored
    /// `Calls` edge confirms; anything else is not-established — a graph
    /// zero never refutes (dynamic dispatch is invisible).
    pub fn verify_calls(&self, caller: &str, callee: &str) -> Result<VerifyVerdict> {
        let from_ids = self.resolve_start_ids(caller)?;
        let to_ids: std::collections::HashSet<String> =
            self.resolve_start_ids(callee)?.into_iter().collect();
        for from in &from_ids {
            let edges = self.db.find_edges_from(from, Some(EdgeType::Calls), 1000)?;
            for edge in edges {
                if to_ids.contains(&edge.to_symbol) {
                    return Ok(VerifyVerdict::Confirmed {
                        evidence: format!("{}:{}", edge.file_path, edge.line),
                    });
                }
            }
        }
        Ok(VerifyVerdict::NotEstablished {
            limit: "call-graph-floor: dynamic dispatch, callbacks and macros are invisible to the index".to_string(),
        })
    }

    /// O7: does `file` define `symbol`? Confirmed with location; refuted
    /// only when the file is indexed (complete evidence); otherwise
    /// not-established.
    pub fn verify_defines(&self, file: &str, symbol: &str) -> Result<VerifyVerdict> {
        let syms = self.db.symbols_in_file(file)?;
        if let Some(hit) = syms.iter().find(|s| s.name == symbol) {
            return Ok(VerifyVerdict::Confirmed {
                evidence: format!("{}:{}", hit.file_path, hit.start_line),
            });
        }
        let meta = self.db.get_all_file_metadata()?;
        if meta.keys().any(|p| p == file) {
            return Ok(VerifyVerdict::Refuted { complete: true });
        }
        Ok(VerifyVerdict::NotEstablished {
            limit: format!("file '{file}' is not indexed"),
        })
    }

    /// O7 (G7 + O2): top hubs minus builtin globals (globals are never
    /// architecture signals).
    pub fn god_nodes(&self, limit: usize) -> Result<Vec<HubNode>> {
        use crate::language::LanguageRegistry;
        let limit = limit.clamp(1, 200);
        let registry = LanguageRegistry::with_builtins();
        let hubs = self.db.hub_nodes(1, limit * 4 + 8)?;
        Ok(hubs
            .into_iter()
            .filter(|h| !registry.is_builtin_global(&h.symbol.lang, &h.symbol.name))
            .take(limit)
            .collect())
    }

    /// O7 (G7 import-cycles): simple file cycles over `Calls` edges,
    /// bounded by `max_len`. Resolves cycle edges through typed module
    /// imports (`ImportMap` / `ImportMapSuffix`) with honest provenance disclosure.
    pub fn call_cycles_ex(&self, max_len: usize) -> Result<Vec<CycleReport>> {
        let max_len = max_len.clamp(2, 12);
        let mut adj: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut edge_provenance: std::collections::HashMap<(String, String), CycleEdgeProvenance> =
            std::collections::HashMap::new();

        for edge in self.db.get_all_edges()? {
            if edge.edge_type != EdgeType::Calls {
                continue;
            }
            let from_file = file_of_symbol(&self.db, &edge.from_symbol)?;
            let to_file = file_of_symbol(&self.db, &edge.to_symbol)?;
            let (Some(from_file), Some(to_file)) = (from_file, to_file) else {
                continue;
            };
            if from_file == to_file {
                continue;
            }

            let is_typed = edge
                .metadata
                .as_ref()
                .and_then(|m| m.get("strategy"))
                .and_then(|v| v.as_str())
                .map(|s| s == "ImportMap" || s == "ImportMapSuffix")
                .unwrap_or(false);

            let prov = if is_typed {
                CycleEdgeProvenance::TypedImport
            } else {
                CycleEdgeProvenance::FileFallback
            };

            let key = (from_file.clone(), to_file.clone());
            edge_provenance
                .entry(key)
                .and_modify(|p| {
                    if *p == CycleEdgeProvenance::FileFallback && is_typed {
                        *p = CycleEdgeProvenance::TypedImport;
                    }
                })
                .or_insert(prov);

            adj.entry(from_file.clone()).or_default().push(to_file);
        }

        for (from, targets) in adj.iter_mut() {
            targets.sort_by(|a, b| {
                let prov_a = edge_provenance
                    .get(&(from.clone(), a.clone()))
                    .copied()
                    .unwrap_or(CycleEdgeProvenance::FileFallback);
                let prov_b = edge_provenance
                    .get(&(from.clone(), b.clone()))
                    .copied()
                    .unwrap_or(CycleEdgeProvenance::FileFallback);
                let order_a = if prov_a == CycleEdgeProvenance::TypedImport {
                    0
                } else {
                    1
                };
                let order_b = if prov_b == CycleEdgeProvenance::TypedImport {
                    0
                } else {
                    1
                };
                order_a.cmp(&order_b).then_with(|| a.cmp(b))
            });
            targets.dedup();
        }

        let mut starts: Vec<String> = adj.keys().cloned().collect();
        starts.sort();
        let mut seen = std::collections::HashSet::new();
        let mut raw_cycles = Vec::new();
        for start in &starts {
            if raw_cycles.len() >= MAX_CYCLES {
                break;
            }
            let mut path = vec![start.clone()];
            dfs_cycles(
                &adj,
                start,
                start,
                max_len,
                MAX_CYCLES,
                &mut path,
                &mut seen,
                &mut raw_cycles,
            );
        }

        let mut reports: Vec<CycleReport> = raw_cycles
            .into_iter()
            .map(|cycle| {
                let mut cycle_prov = CycleEdgeProvenance::TypedImport;
                for i in 0..cycle.len() {
                    let from = &cycle[i];
                    let to = &cycle[(i + 1) % cycle.len()];
                    if let Some(&p) = edge_provenance.get(&(from.clone(), to.clone())) {
                        if p == CycleEdgeProvenance::FileFallback {
                            cycle_prov = CycleEdgeProvenance::FileFallback;
                            break;
                        }
                    } else {
                        cycle_prov = CycleEdgeProvenance::FileFallback;
                        break;
                    }
                }
                CycleReport {
                    cycle,
                    provenance: cycle_prov,
                }
            })
            .collect();

        // Shortest first, then typed before fallback, then lexicographic:
        // keeps the cheapest evidence deterministically.
        reports.sort_by(|a, b| {
            a.cycle.len().cmp(&b.cycle.len()).then_with(|| {
                let order_a = if a.provenance == CycleEdgeProvenance::TypedImport {
                    0
                } else {
                    1
                };
                let order_b = if b.provenance == CycleEdgeProvenance::TypedImport {
                    0
                } else {
                    1
                };
                order_a.cmp(&order_b).then_with(|| a.cycle.cmp(&b.cycle))
            })
        });
        reports.truncate(MAX_CYCLES);
        Ok(reports)
    }

    /// O7: simple file cycles over `Calls` edges, bounded by `max_len`.
    /// Backwards-compatible wrapper over [`call_cycles_ex`].
    pub fn call_cycles(&self, max_len: usize) -> Result<Vec<Vec<String>>> {
        let reports = self.call_cycles_ex(max_len)?;
        Ok(reports.into_iter().map(|r| r.cycle).collect())
    }

    /// O7 (G7 rationale): `NOTE:`/`WHY:`/`HACK:` tags in the target's file,
    /// each linked to its nearest symbol at or above the tag line.
    pub fn rationale_for(
        &self,
        root: &std::path::Path,
        symbol_name: &str,
    ) -> Result<Vec<RationaleTag>> {
        let seeds = self.resolve_start_ids(symbol_name)?;
        let target = seeds
            .iter()
            .filter_map(|id| self.db.symbol_by_stable_id(id).ok().flatten())
            .next()
            .ok_or_else(|| GraphError::Query(format!("no symbol for '{symbol_name}'")))?;
        let content =
            std::fs::read_to_string(root.join(&target.file_path)).map_err(GraphError::Io)?;
        let mut tags = extract_rationale(&content, &target.file_path);
        let siblings = self.db.symbols_in_file(&target.file_path)?;
        for tag in &mut tags {
            let mut best: Option<&Symbol> = None;
            for sym in &siblings {
                if sym.start_line <= tag.line
                    && best.map(|b| b.start_line <= sym.start_line).unwrap_or(true)
                {
                    best = Some(sym);
                }
            }
            tag.symbol = best.map(|s| s.name.clone());
        }
        Ok(tags)
    }

    /// O6: PageRank over the whole `Calls` graph (documented cost: one full
    /// pass per call — batch orientation queries, don't loop it per symbol).
    fn centrality_ranks(&self) -> Result<HashMap<String, f64>> {
        use crate::rank::{
            build_call_graph, pagerank, uniform_teleport, PAGERANK_ALPHA, PAGERANK_MAX_ITER,
            PAGERANK_TOL,
        };
        let triples: Vec<(String, String, f64)> = self
            .db
            .get_all_edges()?
            .iter()
            .filter(|e| e.edge_type == EdgeType::Calls)
            .filter(|e| {
                !(e.from_symbol.starts_with("file:")
                    || e.from_symbol.starts_with("module:")
                    || e.to_symbol.starts_with("file:")
                    || e.to_symbol.starts_with("module:"))
            })
            .map(|e| {
                (
                    e.from_symbol.clone(),
                    e.to_symbol.clone(),
                    e.confidence as f64,
                )
            })
            .collect();
        let graph = build_call_graph(&triples);
        let tele = uniform_teleport(&graph);
        Ok(pagerank(
            &graph,
            &tele,
            PAGERANK_ALPHA,
            PAGERANK_MAX_ITER,
            PAGERANK_TOL,
        ))
    }

    /// O6: most central symbols by PageRank (feeds O7 god-nodes).
    pub fn top_central_symbols(&self, limit: usize) -> Result<Vec<RankedSymbol>> {
        let limit = limit.clamp(1, 200);
        let ranks = self.centrality_ranks()?;
        let mut scored: Vec<RankedSymbol> = Vec::new();
        for (id, score) in &ranks {
            if let Some(symbol) = self.db.symbol_by_stable_id(id)? {
                scored.push(RankedSymbol {
                    symbol,
                    score: *score,
                });
            }
        }
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.symbol.name.cmp(&b.symbol.name))
        });
        scored.truncate(limit);
        Ok(scored)
    }

    /// O6 router (US-111/112): exact name answers directly; otherwise FTS
    /// candidates ordered by graph centrality with a head/margin verdict.
    /// Unresolvable queries refuse (O1) instead of returning noise.
    pub fn route_query(&self, query_text: &str, limit: usize) -> Result<RouteReport> {
        use crate::rank::ranking_margin;
        let limit = limit.clamp(1, 200);

        let exact = self.db.find_by_name(query_text, limit)?;
        if !exact.is_empty() {
            let hits: Vec<RankedSymbol> = exact
                .into_iter()
                .take(limit)
                .map(|symbol| RankedSymbol { symbol, score: 1.0 })
                .collect();
            let est = hits
                .iter()
                .map(|h| crate::budget::symbol_cost(&h.symbol))
                .sum();
            let mut meta = ResponseMeta::new(hits.len(), hits.len(), false);
            meta.est_tokens = Some(est);
            return Ok(RouteReport {
                hits,
                mode: RouteMode::ExactName,
                margin_pct: 100.0,
                confident: true,
                meta,
            });
        }

        let found = self.db.find_symbols(query_text, limit)?;
        if found.symbols.is_empty() {
            return Err(match self.resolve_start_ids(query_text) {
                Ok(_) => GraphError::Query(format!("no candidates for '{query_text}'")),
                Err(e) => e,
            });
        }
        let ranks = self.centrality_ranks()?;
        let mut hits: Vec<RankedSymbol> = found
            .symbols
            .into_iter()
            .map(|symbol| {
                let score = symbol
                    .stable_id
                    .as_ref()
                    .and_then(|id| ranks.get(id).copied())
                    .unwrap_or(0.0);
                RankedSymbol { symbol, score }
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.symbol.name.cmp(&b.symbol.name))
        });
        hits.truncate(limit);
        let scores: Vec<f64> = hits.iter().map(|h| h.score).collect();
        let verdict = ranking_margin(&scores);
        let est = hits
            .iter()
            .map(|h| crate::budget::symbol_cost(&h.symbol))
            .sum();
        // FTS + graph floors: counts are lower bounds (O1).
        let mut meta = ResponseMeta::new(hits.len(), hits.len(), true);
        meta.est_tokens = Some(est);
        Ok(RouteReport {
            hits,
            mode: RouteMode::GraphRanked,
            margin_pct: verdict.margin_pct,
            confident: verdict.confident,
            meta,
        })
    }

    pub fn hubs(&self, min_degree: u64, limit: usize) -> Result<Vec<HubNode>> {
        self.db.hub_nodes(min_degree, limit)
    }

    pub fn hotspots(&self, min_complexity: f32, limit: usize) -> Result<Vec<ComplexityHotspot>> {
        self.db.complexity_hotspots(min_complexity, limit)
    }

    fn traverse(
        &self,
        query: &str,
        edge_type: Option<EdgeType>,
        depth: usize,
        limit: usize,
        reverse: bool,
    ) -> Result<Vec<CodeEdge>> {
        let start = self.resolve_symbol_id(query)?;
        let Some(start) = start else {
            return Ok(vec![]);
        };

        let max_depth = depth.clamp(1, 8);
        let max_edges = limit.clamp(1, 1000);
        let mut queue = VecDeque::from([(start, 0usize)]);
        let mut seen_nodes = HashSet::new();
        let mut seen_edges = HashSet::new();
        let mut results = Vec::new();

        while let Some((node, current_depth)) = queue.pop_front() {
            if current_depth >= max_depth || results.len() >= max_edges {
                continue;
            }
            if !seen_nodes.insert((node.clone(), current_depth)) {
                continue;
            }

            let edges = if reverse {
                self.db.find_edges_to(&node, edge_type.clone(), max_edges)?
            } else {
                self.db
                    .find_edges_from(&node, edge_type.clone(), max_edges)?
            };

            for edge in edges {
                let edge_key = edge.id.unwrap_or_default();
                if !seen_edges.insert(edge_key) {
                    continue;
                }
                let next = if reverse {
                    edge.from_symbol.clone()
                } else {
                    edge.to_symbol.clone()
                };
                results.push(edge);
                if results.len() >= max_edges {
                    break;
                }
                if !next.starts_with("file:") && !next.starts_with("module:") {
                    queue.push_back((next, current_depth + 1));
                }
            }
        }

        Ok(results)
    }

    fn resolve_symbol_id(&self, query: &str) -> Result<Option<String>> {
        if query.len() == 64 && query.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Ok(Some(query.to_string()));
        }
        if let Some(symbol) = self.db.find_symbols(query, 1)?.symbols.into_iter().next() {
            return Ok(symbol.stable_id);
        }
        Ok(None)
    }

    /// Find by file
    pub fn in_file(&self, file_path: &str) -> Result<Vec<Symbol>> {
        self.db.find_by_file(file_path)
    }

    /// by_language query filter symbols WHERE lang = ? COLLATE NOCASE
    pub fn by_language(&self, lang: crate::types::Language, limit: usize) -> Result<Vec<Symbol>> {
        let all_symbols = self.db.get_all_symbols()?;
        let lang_str = lang.as_str().to_lowercase();
        let lang_db_str = lang.as_db_str().to_lowercase();
        let lang_debug_str = format!("{:?}", lang).to_lowercase();

        // Filter WHERE lang = ? COLLATE NOCASE
        let mut filtered: Vec<Symbol> = all_symbols
            .into_iter()
            .filter(|sym| {
                let sym_lang = sym.lang.as_str().to_lowercase();
                let sym_db_lang = sym.lang.as_db_str().to_lowercase();
                let sym_debug_lang = format!("{:?}", sym.lang).to_lowercase();
                sym_lang == lang_str
                    || sym_db_lang == lang_db_str
                    || sym_debug_lang == lang_debug_str
            })
            .collect();

        filtered.truncate(limit);
        Ok(filtered)
    }

    /// Get indexing statistics
    pub fn stats(&self) -> Result<crate::types::IndexStats> {
        self.db.stats()
    }
    pub fn memories_for_symbol(&self, symbol: &str) -> Result<Vec<MemorySymbolLink>> {
        self.db.find_memories_for_symbol(symbol, 100)
    }

    /// Get memories that mention a given symbol with limit
    pub fn memories_for_symbol_limit(
        &self,
        symbol: &str,
        limit: usize,
    ) -> Result<Vec<MemorySymbolLink>> {
        self.db.find_memories_for_symbol(symbol, limit)
    }

    /// Get symbols mentioned by a memory_id
    pub fn symbols_for_memory(&self, memory_id: &str) -> Result<Vec<Symbol>> {
        self.db.find_symbols_for_memory(memory_id)
    }

    /// Semantic search for symbols using cosine similarity over symbol embeddings
    pub async fn semantic_search(
        &self,
        query: &str,
        embedder: &dyn SymbolEmbedder,
        limit: usize,
    ) -> Result<QueryResult> {
        let start = Instant::now();
        let query_vector = embedder.embed(query).await?;
        if query_vector.is_empty() {
            return Ok(QueryResult {
                symbols: Vec::new(),
                total: 0,
                query_time_ms: start.elapsed().as_millis() as u64,
            });
        }

        let all_embeddings = self.db.get_all_symbol_embeddings()?;
        let mut scored_symbols: Vec<(f32, Symbol)> = Vec::new();

        for (stable_id, emb) in all_embeddings {
            if emb.len() != query_vector.len() {
                continue;
            }
            let sim = cosine_similarity(&query_vector, &emb);
            if let Ok(Some(symbol)) = self.db.symbol_by_stable_id(&stable_id) {
                scored_symbols.push((sim, symbol));
            }
        }

        scored_symbols.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored_symbols.truncate(limit);

        let symbols: Vec<Symbol> = scored_symbols.into_iter().map(|(_, sym)| sym).collect();
        let total = symbols.len();

        Ok(QueryResult {
            symbols,
            total,
            query_time_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Hybrid search combining BM25 (FTS5) and Semantic search using Reciprocal Rank Fusion (RRF)
    pub async fn hybrid_search(
        &self,
        query: &str,
        embedder: &dyn SymbolEmbedder,
        limit: usize,
    ) -> Result<QueryResult> {
        let start = Instant::now();
        let rrf_k = 60.0f64;

        // 1. BM25 results
        let bm25_res = self.search(query, limit * 2).unwrap_or(QueryResult {
            symbols: Vec::new(),
            total: 0,
            query_time_ms: 0,
        });

        // 2. Semantic search results
        let sem_res = self
            .semantic_search(query, embedder, limit * 2)
            .await
            .unwrap_or(QueryResult {
                symbols: Vec::new(),
                total: 0,
                query_time_ms: 0,
            });

        let mut rrf_scores: HashMap<String, (f64, Symbol)> = HashMap::new();

        for (rank, sym) in bm25_res.symbols.into_iter().enumerate() {
            let key = sym.stable_id.clone().unwrap_or_else(|| sym.name.clone());
            let score = 1.0 / (rrf_k + (rank as f64 + 1.0));
            rrf_scores.insert(key, (score, sym));
        }

        for (rank, sym) in sem_res.symbols.into_iter().enumerate() {
            let key = sym.stable_id.clone().unwrap_or_else(|| sym.name.clone());
            let score = 1.0 / (rrf_k + (rank as f64 + 1.0));
            rrf_scores
                .entry(key)
                .and_modify(|(s, _)| *s += score)
                .or_insert((score, sym));
        }

        let mut scored_list: Vec<(f64, Symbol)> = rrf_scores.into_values().collect();
        scored_list.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored_list.truncate(limit);

        let symbols: Vec<Symbol> = scored_list.into_iter().map(|(_, sym)| sym).collect();
        let total = symbols.len();

        Ok(QueryResult {
            symbols,
            total,
            query_time_ms: start.elapsed().as_millis() as u64,
        })
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|y| y * y).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

#[cfg(test)]
mod inline_typed_cycle_tests {
    use super::*;

    fn mk_sym(name: &str, file: &str) -> Symbol {
        Symbol {
            name: name.to_string(),
            kind: SymbolKind::Function,
            lang: crate::types::Language::Rust,
            file_path: file.to_string(),
            start_line: 1,
            end_line: 5,
            ..Default::default()
        }
    }

    #[test]
    fn test_typed_import_cycle_resolution() {
        let db = CodeGraphDB::in_memory().expect("in-memory db");
        let fa = mk_sym("fn_a", "/src/a.rs");
        let fb = mk_sym("fn_b", "/src/b.rs");
        db.insert_symbol(&fa).expect("insert fa");
        db.insert_symbol(&fb).expect("insert fb");

        let aid = db.find_by_name("fn_a", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();
        let bid = db.find_by_name("fn_b", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();

        // Edge A -> B via ImportMap strategy (typed import)
        db.insert_edge(&CodeEdge {
            id: None,
            from_symbol: aid.clone(),
            to_symbol: bid.clone(),
            edge_type: EdgeType::Calls,
            file_path: "/src/a.rs".to_string(),
            line: 2,
            confidence: 0.95,
            metadata: Some(serde_json::json!({ "strategy": "ImportMap" })),
        })
        .expect("edge ab");

        // Edge B -> A via ImportMapSuffix strategy (typed import)
        db.insert_edge(&CodeEdge {
            id: None,
            from_symbol: bid,
            to_symbol: aid,
            edge_type: EdgeType::Calls,
            file_path: "/src/b.rs".to_string(),
            line: 2,
            confidence: 0.85,
            metadata: Some(serde_json::json!({ "strategy": "ImportMapSuffix" })),
        })
        .expect("edge ba");

        let query = QueryEngine::new(Arc::new(db));
        let reports = query.call_cycles_ex(4).expect("cycles ex");

        assert!(!reports.is_empty(), "expected at least one cycle report");
        assert_eq!(
            reports[0].provenance,
            CycleEdgeProvenance::TypedImport,
            "expected TypedImport provenance"
        );
        assert_eq!(reports[0].cycle.len(), 2);
    }

    #[test]
    fn test_no_import_fallback_frozen() {
        let db = CodeGraphDB::in_memory().expect("in-memory db");
        let fa = mk_sym("fn_a", "/src/a.rs");
        let fb = mk_sym("fn_b", "/src/b.rs");
        db.insert_symbol(&fa).expect("insert fa");
        db.insert_symbol(&fb).expect("insert fb");

        let aid = db.find_by_name("fn_a", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();
        let bid = db.find_by_name("fn_b", 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap();

        // Edge A -> B without strategy metadata (fallback)
        db.insert_edge(&CodeEdge {
            id: None,
            from_symbol: aid.clone(),
            to_symbol: bid.clone(),
            edge_type: EdgeType::Calls,
            file_path: "/src/a.rs".to_string(),
            line: 2,
            confidence: 0.75,
            metadata: None,
        })
        .expect("edge ab");

        // Edge B -> A without strategy metadata (fallback)
        db.insert_edge(&CodeEdge {
            id: None,
            from_symbol: bid,
            to_symbol: aid,
            edge_type: EdgeType::Calls,
            file_path: "/src/b.rs".to_string(),
            line: 2,
            confidence: 0.75,
            metadata: None,
        })
        .expect("edge ba");

        let query = QueryEngine::new(Arc::new(db));
        let reports = query.call_cycles_ex(4).expect("cycles ex");
        let cycles_legacy = query.call_cycles(4).expect("cycles legacy");

        assert!(!reports.is_empty(), "expected cycle report");
        assert_eq!(
            reports[0].provenance,
            CycleEdgeProvenance::FileFallback,
            "expected FileFallback provenance"
        );
        assert_eq!(
            cycles_legacy,
            vec![reports[0].cycle.clone()],
            "legacy wrapper output must match"
        );
    }

    #[test]
    fn test_call_cycles_dense_cap_and_ordering() {
        let db = CodeGraphDB::in_memory().expect("in-memory db");
        for i in 0..10 {
            db.insert_symbol(&mk_sym(&format!("fn_{}", i), &format!("/src/mod_{}.rs", i)))
                .expect("sym");
        }
        let ids: Vec<String> = (0..10)
            .map(|i| {
                db.find_by_name(&format!("fn_{}", i), 1).unwrap()[0]
                    .stable_id
                    .clone()
                    .unwrap()
            })
            .collect();

        for (a, aid) in ids.iter().enumerate() {
            for (b, bid) in ids.iter().enumerate() {
                if a == b {
                    continue;
                }
                db.insert_edge(&CodeEdge {
                    id: None,
                    from_symbol: aid.clone(),
                    to_symbol: bid.clone(),
                    edge_type: EdgeType::Calls,
                    file_path: format!("/src/mod_{}.rs", a),
                    line: 2,
                    confidence: 0.95,
                    metadata: Some(serde_json::json!({ "strategy": "ImportMap" })),
                })
                .expect("edge");
            }
        }

        let start_time = std::time::Instant::now();
        let query = QueryEngine::new(Arc::new(db));
        let reports = query.call_cycles_ex(6).expect("call cycles ex");
        let elapsed = start_time.elapsed();

        assert!(
            reports.len() <= MAX_CYCLES,
            "must respect MAX_CYCLES cap of {}",
            MAX_CYCLES
        );
        assert!(
            elapsed.as_secs_f64() <= 4.0,
            "performance regression: expected <= 4.0s wall time, got {:.2}s",
            elapsed.as_secs_f64()
        );
        // Shortest-first ordering
        for window in reports.windows(2) {
            assert!(
                window[0].cycle.len() <= window[1].cycle.len(),
                "reports must be ordered shortest-first"
            );
        }
    }
}
