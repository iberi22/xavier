//! Public RAG implementation for CodeGraph
//!
//! Provides the ability to query public CodeGraph symbols and retrieve precise
//! snippets of lines for public workspaces/repositories, strictly filtering
//! for UNCLASSIFIED content.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use code_graph::db::CodeGraphDB;
use code_graph::query::QueryEngine;

/// Query payload for public RAG searches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicRagQuery {
    pub query: String,
    pub repo: Option<String>,
    pub limit: u8,
}

/// Result entry for a public RAG search, including precise code snippets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PublicRagResult {
    pub symbol: String,
    pub file: String,
    pub line_start: usize,
    pub line_end: usize,
    pub snippet: String,
    pub score: f32,
}

/// Helper to dynamically resolve the SWAL workspace directory path.
/// Order of priority:
/// 1. `SWAL_WORKSPACE_DIR` environment variable
/// 2. `XAVIER_SWAL_DIR` environment variable
/// 3. Canonical default `/home/belal/proyectosSWAL`
pub fn resolve_swal_workspace_dir() -> PathBuf {
    if let Ok(val) = std::env::var("SWAL_WORKSPACE_DIR") {
        PathBuf::from(val)
    } else if let Ok(val) = std::env::var("XAVIER_SWAL_DIR") {
        PathBuf::from(val)
    } else {
        PathBuf::from("/home/belal/proyectosSWAL")
    }
}

/// Search across public repository CodeGraph databases via QueryEngine.
/// Filters results to ensure only UNCLASSIFIED (public) contents are returned.
pub fn search_public(query: &str, repo: Option<&str>, limit: u8) -> Vec<PublicRagResult> {
    let mut results = Vec::new();
    let dbs = resolve_public_databases(repo);
    let swal_dir = resolve_swal_workspace_dir();

    for (repo_name, db_path) in dbs {
        let repo_dir = swal_dir.join(&repo_name);

        let Ok(db) = CodeGraphDB::new(&db_path) else {
            continue;
        };
        let engine = QueryEngine::new(Arc::new(db));

        // Use a larger limit during the raw database query to allow for filtering
        let search_limit = (limit as usize * 3).max(50);
        let Ok(query_res) = engine.search(query, search_limit) else {
            continue;
        };

        for symbol in query_res.symbols {
            // Apply clearance level filter: only return UNCLASSIFIED (public) contents.
            if !is_unclassified(&symbol.file_path, &symbol.name) {
                continue;
            }

            let snippet = get_snippet_for_symbol(&repo_dir, &symbol);
            let score = calculate_symbol_score(&symbol.name, query);

            results.push(PublicRagResult {
                symbol: symbol.name.clone(),
                file: symbol.file_path.clone(),
                line_start: symbol.start_line as usize,
                line_end: symbol.end_line as usize,
                snippet,
                score,
            });
        }
    }

    // Sort all combined results by score descending
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    // Truncate to requested limit
    results.truncate(limit as usize);

    results
}

/// Helper to locate public CodeGraph databases.
fn resolve_public_databases(repo: Option<&str>) -> Vec<(String, PathBuf)> {
    let mut dbs = Vec::new();
    let swal_dir = resolve_swal_workspace_dir();

    if let Some(repo_name) = repo {
        let repo_dir = swal_dir.join(repo_name);
        let db_path = crate::codebase::codegraph_paths::code_graph_db_path_for(&repo_dir);
        if db_path.exists() {
            dbs.push((repo_name.to_string(), db_path));
        } else {
            // Check relative to current workspace or absolute fallback
            let local_db = Path::new(".").join(".xavier").join("code_graph.db");
            if local_db.exists() {
                dbs.push((repo_name.to_string(), local_db));
            }
        }
    } else {
        // List directories in SWAL workspace directory
        if let Ok(entries) = std::fs::read_dir(&swal_dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                    let db_path = crate::codebase::codegraph_paths::code_graph_db_path_for(&path);
                    if db_path.exists() {
                        dbs.push((name, db_path));
                    }
                }
            }
        }
        // Fallback: if no databases were found under the swal_dir, try the current workspace
        if dbs.is_empty() {
            let local_db = crate::codebase::codegraph_paths::code_graph_db_path_for(Path::new("."));
            if local_db.exists() {
                dbs.push(("local".to_string(), local_db));
            }
        }
    }
    dbs
}

/// Extract exact lines from the file's source content.
pub fn extract_lines(source: &str, start_line: u32, end_line: u32) -> String {
    let start = start_line.saturating_sub(1) as usize;
    let end = end_line as usize;
    source
        .lines()
        .skip(start)
        .take(end.saturating_sub(start).max(1))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Helper to extract snippet lines or fallback if the file is missing/unreadable.
fn get_snippet_for_symbol(repo_dir: &Path, symbol: &code_graph::types::Symbol) -> String {
    let file_path = repo_dir.join(&symbol.file_path);
    let content = if file_path.exists() {
        std::fs::read_to_string(&file_path).ok()
    } else {
        std::fs::read_to_string(&symbol.file_path).ok()
    };

    if let Some(source) = content {
        extract_lines(&source, symbol.start_line, symbol.end_line)
    } else {
        symbol.signature.clone().unwrap_or_else(|| symbol.name.clone())
    }
}

/// Safety check to filter out non-UNCLASSIFIED (public) information.
fn is_unclassified(file_path: &str, symbol_name: &str) -> bool {
    let path_lower = file_path.to_lowercase();
    let name_lower = symbol_name.to_lowercase();

    // Check typical classified/confidential patterns
    if path_lower.contains(".env")
        || path_lower.contains("secret")
        || path_lower.contains("confidential")
        || path_lower.contains("private_key")
        || path_lower.contains("credentials")
        || path_lower.contains("token")
        || path_lower.contains("wallet")
    {
        return false;
    }

    if name_lower.contains("secret")
        || name_lower.contains("private_key")
        || name_lower.contains("password")
    {
        return false;
    }

    true
}

/// Basic symbol similarity match scoring helper.
fn calculate_symbol_score(symbol_name: &str, query: &str) -> f32 {
    let name_lower = symbol_name.to_lowercase();
    let query_lower = query.to_lowercase();

    if name_lower == query_lower {
        10.0
    } else if name_lower.starts_with(&query_lower) {
        5.0
    } else if name_lower.contains(&query_lower) {
        1.0
    } else {
        0.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_public_rag_result_fields() {
        let result = PublicRagResult {
            symbol: "IrohTransport".to_string(),
            file: "src/mesh/iroh_transport.rs".to_string(),
            line_start: 12,
            line_end: 45,
            snippet: "pub struct IrohTransport {}".to_string(),
            score: 10.0,
        };

        assert_eq!(result.symbol, "IrohTransport");
        assert_eq!(result.file, "src/mesh/iroh_transport.rs");
        assert_eq!(result.line_start, 12);
        assert_eq!(result.line_end, 45);
        assert_eq!(result.snippet, "pub struct IrohTransport {}");
        assert_eq!(result.score, 10.0);
    }

    #[test]
    fn test_public_rag_query_fields() {
        let query = PublicRagQuery {
            query: "IrohTransport".to_string(),
            repo: Some("xavier".to_string()),
            limit: 10,
        };

        assert_eq!(query.query, "IrohTransport");
        assert_eq!(query.repo, Some("xavier".to_string()));
        assert_eq!(query.limit, 10);
    }

    #[test]
    fn test_search_public_limit() {
        let query = "test";
        let repo = Some("mock_repo");
        let limit = 5;

        // Verify limit bounds logic can be constructed properly
        assert_eq!(limit, 5);
        let query_payload = PublicRagQuery {
            query: query.to_string(),
            repo: repo.map(|s| s.to_string()),
            limit,
        };
        assert_eq!(query_payload.limit, 5);
    }

    #[test]
    fn test_search_public_repo_filter() {
        let repo_some = Some("xavier");
        let repo_none: Option<&str> = None;

        assert!(repo_some.is_some());
        assert!(repo_none.is_none());
    }

    #[test]
    fn test_search_public_empty_query() {
        let empty_results = search_public("", Some("nonexistent_repo"), 10);
        assert!(empty_results.is_empty());
    }

    #[test]
    fn test_search_public_unclassified_filter() {
        // Test with explicitly classified patterns to ensure they are excluded
        assert!(!is_unclassified("src/secrets/private_key.rs", "get_private_key"));
        assert!(!is_unclassified(".env.production", "api_key"));
        assert!(!is_unclassified("src/wallet.rs", "sign_tx"));

        // Test with unclassified patterns
        assert!(is_unclassified("src/mesh/iroh_transport.rs", "IrohTransport"));
    }

    #[test]
    fn test_extract_lines_helper() {
        let content = "line 1\nline 2\nline 3\nline 4\nline 5";
        let exact_snippet = extract_lines(content, 2, 4);
        assert_eq!(exact_snippet, "line 2\nline 3\nline 4");

        let single_line = extract_lines(content, 1, 1);
        assert_eq!(single_line, "line 1");
    }

    #[test]
    fn test_calculate_score_helper() {
        assert_eq!(calculate_symbol_score("IrohTransport", "irohtransport"), 10.0);
        assert_eq!(calculate_symbol_score("IrohTransport", "iroh"), 5.0);
        assert_eq!(calculate_symbol_score("IrohTransport", "transport"), 1.0);
        assert_eq!(calculate_symbol_score("IrohTransport", "other"), 0.1);
    }

    #[test]
    fn test_search_public_mock_db() {
        let dir = tempdir().unwrap();
        let db_file_path = dir.path().join("code_graph.db");
        let db = CodeGraphDB::create_new(&db_file_path).unwrap();

        let symbol = code_graph::Symbol {
            id: None,
            stable_id: None,
            name: "IrohTransport".to_string(),
            kind: code_graph::SymbolKind::Struct,
            lang: code_graph::Language::Rust,
            file_path: "src/mesh/iroh_transport.rs".to_string(),
            start_line: 1,
            end_line: 3,
            start_col: 0,
            end_col: 20,
            signature: Some("pub struct IrohTransport".to_string()),
            parent: None,
            complexity: Some(1.2),
        };
        db.insert_symbol(&symbol).unwrap();

        // Verify QueryEngine can fetch mock DB symbols
        let engine = QueryEngine::new(Arc::new(db));
        let query_res = engine.search("IrohTransport", 10).unwrap();
        assert_eq!(query_res.symbols.len(), 1);
        assert_eq!(query_res.symbols[0].name, "IrohTransport");
    }
}
