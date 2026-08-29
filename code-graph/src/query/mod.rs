//! Query engine for code graph

pub mod tests;

use crate::db::CodeGraphDB;
use crate::error::Result;
use crate::types::{
    CodeEdge, ComplexityHotspot, EdgeType, HubNode, MemorySymbolLink, QueryResult, Symbol,
    SymbolEmbedder, SymbolKind,
};
use std::collections::HashMap;
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

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
        let fetch_limit = if query.trim().is_empty() { limit } else { (limit * 10).max(100) };
        let mut result = self.db.find_symbols(query, fetch_limit)?;

        // Narrow filter gate: filter WHERE name LIKE '%query%' COLLATE NOCASE
        // or matching stable_id to drop generic false positives (e.g. imports)
        let q_trimmed = query.trim();
        if !q_trimmed.is_empty() {
            let q_lower = q_trimmed.to_lowercase();
            result.symbols.retain(|sym| {
                let name_matches = sym.name.to_lowercase().contains(&q_lower);
                let id_matches = sym.stable_id.as_deref().map_or(false, |id| id.to_lowercase().contains(&q_lower));
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
            return Ok(vec![]);
        }

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
