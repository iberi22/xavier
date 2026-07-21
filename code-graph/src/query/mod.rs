//! Query engine for code graph

pub mod tests;

use crate::db::CodeGraphDB;
use crate::error::Result;
use crate::types::{
    CodeEdge, ComplexityHotspot, EdgeType, HubNode, QueryResult, Symbol, SymbolKind,
};
use std::collections::HashMap;
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Trait that abstracts search, symbol lookup, and relationship retrieval.
pub trait QueryBridge: Send + Sync {
    /// Search for symbols by name
    fn search(&self, query: &str, limit: usize) -> Result<QueryResult>;

    /// Retrieve a symbol by its stable_id
    fn get_symbol(&self, stable_id: &str) -> Result<Option<Symbol>>;

    /// Get relations (outgoing edges) starting from a symbol
    fn get_relations(&self, stable_id: &str, edge_type: Option<EdgeType>, limit: usize) -> Result<Vec<CodeEdge>>;

    /// Get relations (incoming edges) pointing to a symbol
    fn get_relations_to(&self, _stable_id: &str, _edge_type: Option<EdgeType>, _limit: usize) -> Result<Vec<CodeEdge>> {
        Ok(vec![])
    }

    /// Retrieve statistics for the index
    fn stats(&self) -> Result<crate::types::IndexStats> {
        Ok(crate::types::IndexStats {
            total_files: 0,
            total_symbols: 0,
            total_imports: 0,
            languages: vec![],
            duration_ms: 0,
        })
    }

    /// Retrieve hub nodes
    fn hubs(&self, _min_degree: u64, _limit: usize) -> Result<Vec<HubNode>> {
        Ok(vec![])
    }

    /// Retrieve complexity hotspots
    fn hotspots(&self, _min_complexity: f32, _limit: usize) -> Result<Vec<ComplexityHotspot>> {
        Ok(vec![])
    }
}

/// Default implementation of QueryBridge using the Rust SQLite-backed CodeGraphDB
pub struct RustQueryBridge {
    db: Arc<CodeGraphDB>,
}

impl RustQueryBridge {
    pub fn new(db: Arc<CodeGraphDB>) -> Self {
        Self { db }
    }
}

impl QueryBridge for RustQueryBridge {
    fn search(&self, query: &str, limit: usize) -> Result<QueryResult> {
        self.db.find_symbols(query, limit)
    }

    fn get_symbol(&self, stable_id: &str) -> Result<Option<Symbol>> {
        self.db.symbol_by_stable_id(stable_id)
    }

    fn get_relations(&self, stable_id: &str, edge_type: Option<EdgeType>, limit: usize) -> Result<Vec<CodeEdge>> {
        self.db.find_edges_from(stable_id, edge_type, limit)
    }

    fn get_relations_to(&self, stable_id: &str, edge_type: Option<EdgeType>, limit: usize) -> Result<Vec<CodeEdge>> {
        self.db.find_edges_to(stable_id, edge_type, limit)
    }

    fn stats(&self) -> Result<crate::types::IndexStats> {
        self.db.stats()
    }

    fn hubs(&self, min_degree: u64, limit: usize) -> Result<Vec<HubNode>> {
        self.db.hub_nodes(min_degree, limit)
    }

    fn hotspots(&self, min_complexity: f32, limit: usize) -> Result<Vec<ComplexityHotspot>> {
        self.db.complexity_hotspots(min_complexity, limit)
    }
}

/// Different symbol representation used in C codegraph schema.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CSymbol {
    pub uuid: String,
    pub name: String,
    pub c_kind: String, // e.g. "function_definition", "struct_specifier", "preproc_def", "global_variable"
    pub filepath: String,
    pub start_line: u32,
    pub end_line: u32,
    pub start_col: u32,
    pub end_col: u32,
    pub signature_text: Option<String>,
    pub parent_scope: Option<String>,
    pub cyclomatic_complexity: Option<f32>,
}

/// Richer relationship data used in C codegraph schema.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CRelation {
    pub from_uuid: String,
    pub to_uuid: String,
    pub rel_type: String, // e.g. "PointsTo", "MacroExpansion", "Includes", "Reads", "Writes", "Calls"
    pub file_path: String,
    pub line_num: u32,
    pub weight: f32,
}

impl CSymbol {
    pub fn to_rust_symbol(&self) -> Symbol {
        let kind = match self.c_kind.as_str() {
            "function_definition" | "function" => SymbolKind::Function,
            "struct_specifier" | "struct" => SymbolKind::Struct,
            "enum_specifier" | "enum" => SymbolKind::Enum,
            "preproc_def" | "macro" => SymbolKind::Constant,
            "preproc_include" | "include" => SymbolKind::Import,
            "global_variable" | "global" => SymbolKind::Variable,
            "typedef" => SymbolKind::TypeAlias,
            "union_specifier" | "union" => SymbolKind::Struct,
            _ => SymbolKind::Symbol,
        };

        Symbol {
            id: None,
            stable_id: Some(self.uuid.clone()),
            name: self.name.clone(),
            kind,
            lang: crate::types::Language::C,
            file_path: self.filepath.clone(),
            start_line: self.start_line,
            end_line: self.end_line,
            start_col: self.start_col,
            end_col: self.end_col,
            signature: self.signature_text.clone(),
            parent: self.parent_scope.clone(),
            complexity: self.cyclomatic_complexity,
        }
    }
}

impl CRelation {
    pub fn to_rust_edge(&self) -> CodeEdge {
        let edge_type = match self.rel_type.as_str() {
            "Calls" => EdgeType::Calls,
            "Defines" => EdgeType::Defines,
            "Uses" => EdgeType::Uses,
            "Imports" => EdgeType::Imports,
            "Exports" => EdgeType::Exports,
            "Contains" => EdgeType::Contains,
            "References" => EdgeType::References,
            "Extends" => EdgeType::Extends,
            "Implements" => EdgeType::Implements,
            "TypeOf" => EdgeType::TypeOf,
            "Returns" => EdgeType::Returns,
            "Instantiates" => EdgeType::Instantiates,
            "Overrides" => EdgeType::Overrides,
            "Decorates" => EdgeType::Decorates,
            "PointsTo" => EdgeType::PointsTo,
            "MacroExpansion" => EdgeType::MacroExpansion,
            "Includes" => EdgeType::Includes,
            "Reads" => EdgeType::Reads,
            "Writes" => EdgeType::Writes,
            _ => EdgeType::References,
        };

        CodeEdge {
            id: None,
            from_symbol: self.from_uuid.clone(),
            to_symbol: self.to_uuid.clone(),
            edge_type,
            file_path: self.file_path.clone(),
            line: self.line_num,
            confidence: self.weight,
            metadata: None,
        }
    }
}

/// QueryBridge implementation for C backend that normalizes in-memory CSymbols and CRelations
pub struct CQueryBridge {
    pub symbols: Vec<Symbol>,
    pub edges: Vec<CodeEdge>,
}

impl CQueryBridge {
    pub fn new(c_symbols: Vec<CSymbol>, c_relations: Vec<CRelation>) -> Self {
        let symbols = c_symbols.into_iter().map(|s| s.to_rust_symbol()).collect();
        let edges = c_relations.into_iter().map(|r| r.to_rust_edge()).collect();
        Self { symbols, edges }
    }
}

impl QueryBridge for CQueryBridge {
    fn search(&self, query: &str, limit: usize) -> Result<QueryResult> {
        let query_lower = query.to_lowercase();
        let mut matched: Vec<Symbol> = self.symbols
            .iter()
            .filter(|s| {
                if query_lower.is_empty() {
                    true
                } else {
                    s.name.to_lowercase().contains(&query_lower) ||
                    s.file_path.to_lowercase().contains(&query_lower)
                }
            })
            .cloned()
            .collect();

        let total = matched.len();
        matched.truncate(limit);

        Ok(QueryResult {
            symbols: matched,
            total,
            query_time_ms: 0,
        })
    }

    fn get_symbol(&self, stable_id: &str) -> Result<Option<Symbol>> {
        let found = self.symbols
            .iter()
            .find(|s| s.stable_id.as_deref() == Some(stable_id))
            .cloned();
        Ok(found)
    }

    fn get_relations(&self, stable_id: &str, edge_type: Option<EdgeType>, limit: usize) -> Result<Vec<CodeEdge>> {
        let mut matched: Vec<CodeEdge> = self.edges
            .iter()
            .filter(|e| {
                e.from_symbol == stable_id &&
                edge_type.as_ref().map_or(true, |et| &e.edge_type == et)
            })
            .cloned()
            .collect();

        matched.truncate(limit);
        Ok(matched)
    }

    fn get_relations_to(&self, stable_id: &str, edge_type: Option<EdgeType>, limit: usize) -> Result<Vec<CodeEdge>> {
        let mut matched: Vec<CodeEdge> = self.edges
            .iter()
            .filter(|e| {
                e.to_symbol == stable_id &&
                edge_type.as_ref().map_or(true, |et| &e.edge_type == et)
            })
            .cloned()
            .collect();

        matched.truncate(limit);
        Ok(matched)
    }

    fn stats(&self) -> Result<crate::types::IndexStats> {
        let mut files = HashSet::new();
        let mut total_imports = 0;
        for s in &self.symbols {
            files.insert(s.file_path.clone());
            if s.kind == SymbolKind::Import {
                total_imports += 1;
            }
        }
        for e in &self.edges {
            files.insert(e.file_path.clone());
        }

        Ok(crate::types::IndexStats {
            total_files: files.len() as u64,
            total_symbols: self.symbols.len() as u64,
            total_imports,
            languages: vec![crate::types::LanguageCount {
                lang: crate::types::Language::C,
                count: self.symbols.len() as u64,
            }],
            duration_ms: 0,
        })
    }

    fn hubs(&self, min_degree: u64, limit: usize) -> Result<Vec<HubNode>> {
        let mut hubs = Vec::new();
        for s in &self.symbols {
            let stable_id = s.stable_id.as_deref().unwrap_or("");
            if stable_id.is_empty() {
                continue;
            }
            let incoming = self.edges.iter().filter(|e| e.to_symbol == stable_id).count() as u64;
            let outgoing = self.edges.iter().filter(|e| e.from_symbol == stable_id).count() as u64;
            let total = incoming + outgoing;
            if total >= min_degree {
                hubs.push(HubNode {
                    symbol: s.clone(),
                    incoming,
                    outgoing,
                    total,
                });
            }
        }
        hubs.sort_by(|a, b| b.total.cmp(&a.total));
        hubs.truncate(limit);
        Ok(hubs)
    }

    fn hotspots(&self, min_complexity: f32, limit: usize) -> Result<Vec<ComplexityHotspot>> {
        let mut hotspots = Vec::new();
        for s in &self.symbols {
            let complexity = s.complexity.unwrap_or(0.0);
            if complexity >= min_complexity {
                let stable_id = s.stable_id.as_deref().unwrap_or("");
                if stable_id.is_empty() {
                    continue;
                }
                let incoming = self.edges.iter().filter(|e| e.to_symbol == stable_id).count() as u64;
                let outgoing = self.edges.iter().filter(|e| e.from_symbol == stable_id).count() as u64;
                let risk_score = complexity * (incoming as f32 + 1.0);
                hotspots.push(ComplexityHotspot {
                    symbol: s.clone(),
                    incoming,
                    outgoing,
                    risk_score,
                });
            }
        }
        hotspots.sort_by(|a, b| {
            b.risk_score.partial_cmp(&a.risk_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.symbol.complexity.partial_cmp(&a.symbol.complexity).unwrap_or(std::cmp::Ordering::Equal))
        });
        hotspots.truncate(limit);
        Ok(hotspots)
    }
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
    bridge: Option<Box<dyn QueryBridge>>,
}

impl QueryEngine {
    pub fn new(db: Arc<CodeGraphDB>) -> Self {
        Self {
            db,
            cache: None,
            bridge: None,
        }
    }

    /// Create with cache
    pub fn with_cache(db: Arc<CodeGraphDB>, ttl_secs: u64, max_entries: usize) -> Self {
        Self {
            db,
            cache: Some(Arc::new(QueryCache::new(ttl_secs, max_entries))),
            bridge: None,
        }
    }

    /// Create a QueryEngine that delegates to a specific QueryBridge
    pub fn with_bridge(db: Arc<CodeGraphDB>, bridge: Box<dyn QueryBridge>) -> Self {
        Self {
            db,
            cache: None,
            bridge: Some(bridge),
        }
    }

    /// Set a custom QueryBridge on the QueryEngine
    pub fn set_bridge(&mut self, bridge: Box<dyn QueryBridge>) {
        self.bridge = Some(bridge);
    }

    /// Search for symbols by name (with caching)
    pub fn search(&self, query: &str, limit: usize) -> Result<QueryResult> {
        // Try cache first
        if let Some(ref cache) = self.cache {
            if let Some(result) = cache.get(query) {
                return Ok(result);
            }
        }

        // Query database or bridge
        let result = if let Some(ref bridge) = self.bridge {
            bridge.search(query, limit)?
        } else {
            self.db.find_symbols(query, limit)?
        };

        // Store in cache
        if let Some(ref cache) = self.cache {
            cache.set(query.to_string(), result.clone());
        }

        Ok(result)
    }

    /// Find all functions
    pub fn functions(&self, limit: usize) -> Result<Vec<Symbol>> {
        if let Some(ref bridge) = self.bridge {
            let res = bridge.search("", usize::MAX)?;
            let mut matched: Vec<Symbol> = res.symbols.into_iter().filter(|s| s.kind == SymbolKind::Function).collect();
            matched.truncate(limit);
            Ok(matched)
        } else {
            self.db.find_by_kind(SymbolKind::Function, limit)
        }
    }

    /// Find all structs
    pub fn structs(&self, limit: usize) -> Result<Vec<Symbol>> {
        if let Some(ref bridge) = self.bridge {
            let res = bridge.search("", usize::MAX)?;
            let mut matched: Vec<Symbol> = res.symbols.into_iter().filter(|s| s.kind == SymbolKind::Struct).collect();
            matched.truncate(limit);
            Ok(matched)
        } else {
            self.db.find_by_kind(SymbolKind::Struct, limit)
        }
    }

    /// Find all classes
    pub fn classes(&self, limit: usize) -> Result<Vec<Symbol>> {
        if let Some(ref bridge) = self.bridge {
            let res = bridge.search("", usize::MAX)?;
            let mut matched: Vec<Symbol> = res.symbols.into_iter().filter(|s| s.kind == SymbolKind::Class).collect();
            matched.truncate(limit);
            Ok(matched)
        } else {
            self.db.find_by_kind(SymbolKind::Class, limit)
        }
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
            "module_definition" | "module" => SymbolKind::Module,
            "import" | "use_statement" => SymbolKind::Module, // Treat imports as modules
            _ => return Ok(vec![]),
        };

        if let Some(ref bridge) = self.bridge {
            let res = bridge.search("", usize::MAX)?;
            let mut matched: Vec<Symbol> = res.symbols.into_iter().filter(|s| s.kind == kind).collect();
            matched.truncate(limit);
            Ok(matched)
        } else {
            self.db.find_by_kind(kind, limit)
        }
    }

    /// Find all enums
    pub fn enums(&self, limit: usize) -> Result<Vec<Symbol>> {
        if let Some(ref bridge) = self.bridge {
            let res = bridge.search("", usize::MAX)?;
            let mut matched: Vec<Symbol> = res.symbols.into_iter().filter(|s| s.kind == SymbolKind::Enum).collect();
            matched.truncate(limit);
            Ok(matched)
        } else {
            self.db.find_by_kind(SymbolKind::Enum, limit)
        }
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

    pub fn hubs(&self, min_degree: u64, limit: usize) -> Result<Vec<HubNode>> {
        if let Some(ref bridge) = self.bridge {
            bridge.hubs(min_degree, limit)
        } else {
            self.db.hub_nodes(min_degree, limit)
        }
    }

    pub fn hotspots(&self, min_complexity: f32, limit: usize) -> Result<Vec<ComplexityHotspot>> {
        if let Some(ref bridge) = self.bridge {
            bridge.hotspots(min_complexity, limit)
        } else {
            self.db.complexity_hotspots(min_complexity, limit)
        }
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
                if let Some(ref bridge) = self.bridge {
                    bridge.get_relations_to(&node, edge_type.clone(), max_edges)?
                } else {
                    self.db.find_edges_to(&node, edge_type.clone(), max_edges)?
                }
            } else {
                if let Some(ref bridge) = self.bridge {
                    bridge.get_relations(&node, edge_type.clone(), max_edges)?
                } else {
                    self.db
                        .find_edges_from(&node, edge_type.clone(), max_edges)?
                }
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
        let symbols = if let Some(ref bridge) = self.bridge {
            bridge.search(query, 1)?.symbols
        } else {
            self.db.find_symbols(query, 1)?.symbols
        };
        if let Some(symbol) = symbols.into_iter().next() {
            return Ok(symbol.stable_id);
        }
        Ok(None)
    }

    /// Find by file
    pub fn in_file(&self, file_path: &str) -> Result<Vec<Symbol>> {
        if let Some(ref bridge) = self.bridge {
            let res = bridge.search("", usize::MAX)?;
            let matched = res.symbols.into_iter().filter(|s| s.file_path == file_path).collect();
            Ok(matched)
        } else {
            self.db.find_by_file(file_path)
        }
    }

    /// Get all symbols of a specific language
    pub fn by_language(&self, lang: crate::types::Language, limit: usize) -> Result<Vec<Symbol>> {
        if let Some(ref bridge) = self.bridge {
            let res = bridge.search("", usize::MAX)?;
            let mut matched: Vec<Symbol> = res.symbols.into_iter().filter(|s| s.lang == lang).collect();
            matched.truncate(limit);
            Ok(matched)
        } else {
            Ok(vec![])
        }
    }

    /// Get indexing statistics
    pub fn stats(&self) -> Result<crate::types::IndexStats> {
        if let Some(ref bridge) = self.bridge {
            bridge.stats()
        } else {
            self.db.stats()
        }
    }

    /// Resolve a stable ID to a Symbol
    pub fn symbol_by_stable_id(&self, stable_id: &str) -> Result<Option<Symbol>> {
        self.db.symbol_by_stable_id(stable_id)
    }

    /// Resolve a symbol query (name or stable ID) to a stable ID
    pub fn resolve_symbol_id(&self, query: &str) -> Result<Option<String>> {
        if query.len() == 64 && query.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Ok(Some(query.to_string()));
        }
        if let Some(symbol) = self.db.find_symbols(query, 1)?.symbols.into_iter().next() {
            return Ok(symbol.stable_id);
        }
        Ok(None)
    }
}
