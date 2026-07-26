//! Shared code-graph path resolution.
//!
//! Centralises where the code-graph SQLite DB and its portable JSON dump
//! live, so that CLI, maturity scanner, auto-docs, and any other consumer
//! resolve the same paths.
//!
//! See [`docs/research/PLAN-DEFINITIVO-XTSP-GPU-LAB.md`](https://github.com/iberi22/xavier/tree/main/docs/research/PLAN-DEFINITIVO-XTSP-GPU-LAB.md)
//! for the architectural plan that drove these helpers.

use std::path::{Path, PathBuf};

/// Returns the path to the code-graph SQLite database for `workspace`.
///
/// When `workspace` is `.` or matches the current directory, resolves the
/// canonical path via settings and env var (same logic as
/// `cli::config::code_graph_db_path`). Otherwise places the DB at
/// `workspace/.xavier/code_graph.db`.
pub fn code_graph_db_path_for(workspace: &Path) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    if workspace == Path::new(".") || workspace == cwd {
        // Resolve via settings + env override (mirrors cli/config.rs logic)
        std::env::var("XAVIER_CODE_GRAPH_DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let settings = crate::settings::XavierSettings::current();
                PathBuf::from(settings.server.code_graph_db_path)
            })
    } else {
        workspace.join(".xavier").join("code_graph.db")
    }
}

/// Returns the path to the portable JSON dump for `workspace`.
///
/// Always placed under `workspace/.xavier/codegraph.json`.
/// This dump is produced after `code scan` / `code index` and consumed
/// by the maturity scanner.
pub fn codegraph_dump_path_for(workspace: &Path) -> PathBuf {
    workspace.join(".xavier").join("codegraph.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_graph_db_path_for_default() {
        let path = code_graph_db_path_for(Path::new("."));
        assert!(!path.as_os_str().is_empty());
    }

    #[test]
    fn test_code_graph_db_path_for_explicit() {
        let ws = Path::new("/tmp/test-workspace");
        let path = code_graph_db_path_for(ws);
        assert_eq!(path, ws.join(".xavier").join("code_graph.db"));
    }

    #[test]
    fn test_codegraph_dump_path_for() {
        let ws = Path::new("/tmp/test-workspace");
        let path = codegraph_dump_path_for(ws);
        assert_eq!(path, ws.join(".xavier").join("codegraph.json"));
    }

    #[test]
    fn test_codegraph_dump_path_is_not_db_path() {
        let ws = Path::new("/tmp/test-workspace");
        assert_ne!(
            code_graph_db_path_for(ws),
            codegraph_dump_path_for(ws),
        );
    }
}
