//! Call-graph evidence: the direct callers and callees of the symbols a code
//! fragment covers, read from the code graph Xavier already indexes.

use std::collections::HashMap;
use std::path::{Component, Path};
use std::sync::Arc;

use anyhow::{bail, Result};
use code_graph::query::QueryEngine;
use code_graph::types::{CodeEdge, EdgeType, Language, Symbol};

use super::context::{Evidence, EvidenceKind, EvidenceSource, ExplainTarget};

/// Score of a direct caller. Callers rank above callees.
const CALLER_SCORE: f32 = 0.8;
/// Score of a direct callee.
const CALLEE_SCORE: f32 = 0.6;

/// Evidence source listing the callers and callees of the symbols inside the target range.
pub struct CodeGraphSource {
    engine: Arc<QueryEngine>,
    depth: usize,
}

impl CodeGraphSource {
    pub fn new(engine: Arc<QueryEngine>) -> Self {
        Self { engine, depth: 1 }
    }
}

impl EvidenceSource for CodeGraphSource {
    fn name(&self) -> &str {
        "codegraph"
    }

    fn collect(&self, target: &ExplainTarget, limit: usize) -> Result<Vec<Evidence>> {
        let file = index_path(&target.file)?;
        if limit == 0 {
            return Ok(vec![]);
        }

        let symbols = self.engine.in_file(&file)?;
        let chosen = choose_symbols(symbols, target);
        if chosen.is_empty() {
            return Ok(vec![]);
        }

        let mut languages: Vec<Language> = Vec::new();
        for symbol in &chosen {
            if !languages.contains(&symbol.lang) {
                languages.push(symbol.lang.clone());
            }
        }
        let names = names_by_id(&self.engine, &languages)?;

        // Callers first, then callees; `limit` caps both together.
        let mut out = Vec::new();
        for incoming in [true, false] {
            for symbol in &chosen {
                let room = limit.saturating_sub(out.len());
                if room == 0 {
                    return Ok(out);
                }
                let query = symbol
                    .stable_id
                    .clone()
                    .unwrap_or_else(|| symbol.name.clone());
                let edges = if incoming {
                    self.engine
                        .reverse_dependencies(&query, None, self.depth, room)?
                } else {
                    self.engine.dependencies(&query, None, self.depth, room)?
                };
                for edge in edges.into_iter().filter(|e| !is_structural(&e.edge_type)) {
                    if out.len() >= limit {
                        break;
                    }
                    out.push(edge_evidence(&edge, &names, incoming));
                }
            }
        }
        Ok(out)
    }
}

/// Symbols of the target file whose line span overlaps the target range.
/// A `target.symbol` name narrows the choice when a symbol carries that name.
fn choose_symbols(symbols: Vec<Symbol>, target: &ExplainTarget) -> Vec<Symbol> {
    let overlapping: Vec<Symbol> = symbols
        .into_iter()
        .filter(|s| s.start_line <= target.end_line && s.end_line >= target.start_line)
        .collect();
    match target.symbol.as_deref() {
        Some(name) if overlapping.iter().any(|s| s.name == name) => {
            overlapping.into_iter().filter(|s| s.name == name).collect()
        }
        _ => overlapping,
    }
}

/// Maps stable ids to symbol names for the given languages. The engine has no
/// stable-id lookup, so this lists each language's symbols once. Endpoints
/// outside those languages keep their stable id as the name.
fn names_by_id(engine: &QueryEngine, languages: &[Language]) -> Result<HashMap<String, String>> {
    let mut names = HashMap::new();
    for language in languages {
        for symbol in engine.by_language(language.clone(), usize::MAX)? {
            if let Some(id) = symbol.stable_id {
                names.entry(id).or_insert(symbol.name);
            }
        }
    }
    Ok(names)
}

fn edge_evidence(edge: &CodeEdge, names: &HashMap<String, String>, incoming: bool) -> Evidence {
    let name_of = |id: &str| names.get(id).cloned().unwrap_or_else(|| id.to_string());
    let from = name_of(&edge.from_symbol);
    let to = name_of(&edge.to_symbol);
    let (role, score, subject) = if incoming {
        ("caller", CALLER_SCORE, from.as_str())
    } else {
        ("callee", CALLEE_SCORE, to.as_str())
    };
    Evidence {
        kind: EvidenceKind::CallGraph,
        source_ref: format!("{role}:{subject}@{}:{}", edge.file_path, edge.line),
        text: format!("{from} -> {to} ({})", edge.edge_type.as_str()),
        score,
    }
}

fn is_structural(edge_type: &EdgeType) -> bool {
    matches!(edge_type, EdgeType::Contains | EdgeType::Defines)
}

/// The path as the indexer stores it: relative to the repo root, with forward slashes.
fn index_path(file: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for component in file.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => bail!("file must be relative to the repo: {}", file.display()),
        }
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use code_graph::db::CodeGraphDB;
    use code_graph::types::SymbolKind;

    use super::*;

    /// Index of `src/lib.rs` (and `src/util.rs` for the callee `helper`):
    ///
    /// ```text
    /// 2-6   handle    calls helper (line 4)
    /// 10-12 route     calls handle (line 11)
    /// 20-22 unrelated calls helper (line 21)
    /// ```
    fn fixture() -> (tempfile::TempDir, Arc<QueryEngine>) {
        let dir = tempfile::tempdir().unwrap();
        let db = CodeGraphDB::new(&dir.path().join("graph.db")).unwrap();
        for symbol in [
            sym("handle", "src/lib.rs", 2, 6),
            sym("route", "src/lib.rs", 10, 12),
            sym("unrelated", "src/lib.rs", 20, 22),
            sym("helper", "src/util.rs", 1, 3),
        ] {
            db.insert_symbol(&symbol).unwrap();
        }
        let handle = stable_id(&db, "handle");
        let route = stable_id(&db, "route");
        let unrelated = stable_id(&db, "unrelated");
        let helper = stable_id(&db, "helper");
        call(&db, &handle, &helper, "src/lib.rs", 4);
        call(&db, &route, &handle, "src/lib.rs", 11);
        call(&db, &unrelated, &helper, "src/lib.rs", 21);
        (dir, Arc::new(QueryEngine::new(Arc::new(db))))
    }

    fn sym(name: &str, file: &str, start_line: u32, end_line: u32) -> Symbol {
        Symbol {
            id: None,
            stable_id: None,
            name: name.to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: file.to_string(),
            start_line,
            end_line,
            start_col: 0,
            end_col: 0,
            signature: Some(format!("fn {name}()")),
            parent: None,
            complexity: None,
        }
    }

    fn stable_id(db: &CodeGraphDB, name: &str) -> String {
        db.find_by_name(name, 1).unwrap()[0]
            .stable_id
            .clone()
            .unwrap()
    }

    fn call(db: &CodeGraphDB, from: &str, to: &str, file: &str, line: u32) {
        db.insert_edge(&CodeEdge {
            id: None,
            from_symbol: from.to_string(),
            to_symbol: to.to_string(),
            edge_type: EdgeType::Calls,
            file_path: file.to_string(),
            line,
            confidence: 1.0,
            metadata: None,
        })
        .unwrap();
    }

    fn target(file: &str, start_line: u32, end_line: u32, symbol: Option<&str>) -> ExplainTarget {
        ExplainTarget {
            repo_root: PathBuf::from("/nonexistent-repo-root"),
            file: PathBuf::from(file),
            start_line,
            end_line,
            symbol: symbol.map(str::to_string),
        }
    }

    fn texts(evidence: &[Evidence]) -> Vec<String> {
        evidence.iter().map(|e| e.text.clone()).collect()
    }

    #[test]
    fn callers_and_callees_of_symbol_in_range_get_source_refs() {
        let (_dir, engine) = fixture();
        let source = CodeGraphSource::new(engine);

        let evidence = source
            .collect(&target("src/lib.rs", 3, 4, None), 10)
            .unwrap();

        assert_eq!(evidence.len(), 2);
        let caller = &evidence[0];
        assert_eq!(caller.kind, EvidenceKind::CallGraph);
        assert_eq!(caller.source_ref, "caller:route@src/lib.rs:11");
        assert_eq!(caller.text, "route -> handle (Calls)");
        let callee = &evidence[1];
        assert_eq!(callee.source_ref, "callee:helper@src/lib.rs:4");
        assert_eq!(callee.text, "handle -> helper (Calls)");
        assert!(caller.score > callee.score);
        assert!(callee.score < f32::MAX);
    }

    #[test]
    fn symbols_outside_line_range_are_ignored() {
        let (_dir, engine) = fixture();
        let source = CodeGraphSource::new(engine);

        let evidence = source
            .collect(&target("src/lib.rs", 3, 4, None), 10)
            .unwrap();

        let refs: Vec<&str> = evidence.iter().map(|e| e.source_ref.as_str()).collect();
        assert_eq!(
            refs,
            vec!["caller:route@src/lib.rs:11", "callee:helper@src/lib.rs:4"]
        );
        assert!(!texts(&evidence).iter().any(|t| t.starts_with("unrelated")));
    }

    #[test]
    fn empty_file_or_range_returns_ok_empty() {
        let (_dir, engine) = fixture();
        let source = CodeGraphSource::new(engine);

        let no_file = source
            .collect(&target("src/missing.rs", 1, 5, None), 10)
            .unwrap();
        let no_symbol = source
            .collect(&target("src/lib.rs", 50, 60, None), 10)
            .unwrap();

        assert!(no_file.is_empty());
        assert!(no_symbol.is_empty());
    }

    #[test]
    fn symbol_name_narrows_the_choice() {
        let (_dir, engine) = fixture();
        let source = CodeGraphSource::new(engine);

        let all = source
            .collect(&target("src/lib.rs", 1, 30, None), 10)
            .unwrap();
        let narrowed = source
            .collect(&target("src/lib.rs", 1, 30, Some("route")), 10)
            .unwrap();

        assert_eq!(
            texts(&narrowed),
            vec!["route -> handle (Calls)".to_string()]
        );
        assert!(texts(&all).len() > texts(&narrowed).len());
    }

    #[test]
    fn limit_caps_total_edges_callers_first() {
        let (_dir, engine) = fixture();
        let source = CodeGraphSource::new(engine);

        let evidence = source
            .collect(&target("src/lib.rs", 3, 4, None), 1)
            .unwrap();

        assert_eq!(
            texts(&evidence),
            vec!["route -> handle (Calls)".to_string()]
        );
    }
}
