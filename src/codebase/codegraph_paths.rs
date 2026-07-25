//! Shared code-graph paths helper
//!
//! Provides resolution of the `.xavier/code_graph.db` and `.xavier/codegraph.json`
//! files relative to any given workspace root, while respecting relevant environment
//! variable overrides.

use std::path::{Path, PathBuf};

/// Returns the path to the code-graph SQLite database for the given workspace root.
///
/// Priority:
/// 1. `XAVIER_CODE_GRAPH_DB_PATH` environment variable
/// 2. `<workspace>/.xavier/code_graph.db`
pub fn code_graph_db_path_for(workspace: &Path) -> PathBuf {
    if let Ok(env_path) = std::env::var("XAVIER_CODE_GRAPH_DB_PATH") {
        PathBuf::from(env_path)
    } else {
        workspace.join(".xavier").join("code_graph.db")
    }
}

/// Returns the path to the portable code-graph dump JSON file for the given workspace root.
///
/// Priority:
/// 1. `XAVIER_CODEGRAPH_DUMP_PATH` environment variable
/// 2. `<workspace>/.xavier/codegraph.json`
pub fn codegraph_dump_path_for(workspace: &Path) -> PathBuf {
    if let Ok(env_path) = std::env::var("XAVIER_CODEGRAPH_DUMP_PATH") {
        PathBuf::from(env_path)
    } else {
        workspace.join(".xavier").join("codegraph.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Use a mutex to serialize environment variable mutation in tests
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_codegraph_paths_default() {
        let _guard = ENV_MUTEX.lock().unwrap();

        // Ensure env variables are cleared for this test
        let old_db = std::env::var("XAVIER_CODE_GRAPH_DB_PATH");
        let old_dump = std::env::var("XAVIER_CODEGRAPH_DUMP_PATH");
        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");
        std::env::remove_var("XAVIER_CODEGRAPH_DUMP_PATH");

        let workspace = Path::new("/test/workspace");
        assert_eq!(
            code_graph_db_path_for(workspace),
            PathBuf::from("/test/workspace/.xavier/code_graph.db")
        );
        assert_eq!(
            codegraph_dump_path_for(workspace),
            PathBuf::from("/test/workspace/.xavier/codegraph.json")
        );

        // Restore env variables
        if let Ok(val) = old_db {
            std::env::set_var("XAVIER_CODE_GRAPH_DB_PATH", val);
        }
        if let Ok(val) = old_dump {
            std::env::set_var("XAVIER_CODEGRAPH_DUMP_PATH", val);
        }
    }

    #[test]
    fn test_codegraph_paths_env_overrides() {
        let _guard = ENV_MUTEX.lock().unwrap();

        // Save original env
        let old_db = std::env::var("XAVIER_CODE_GRAPH_DB_PATH");
        let old_dump = std::env::var("XAVIER_CODEGRAPH_DUMP_PATH");

        std::env::set_var("XAVIER_CODE_GRAPH_DB_PATH", "/custom/path/to/db.db");
        std::env::set_var("XAVIER_CODEGRAPH_DUMP_PATH", "/custom/path/to/dump.json");

        let workspace = Path::new("/test/workspace");
        assert_eq!(
            code_graph_db_path_for(workspace),
            PathBuf::from("/custom/path/to/db.db")
        );
        assert_eq!(
            codegraph_dump_path_for(workspace),
            PathBuf::from("/custom/path/to/dump.json")
        );

        // Restore original env
        if let Ok(val) = old_db {
            std::env::set_var("XAVIER_CODE_GRAPH_DB_PATH", val);
        } else {
            std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");
        }
        if let Ok(val) = old_dump {
            std::env::set_var("XAVIER_CODEGRAPH_DUMP_PATH", val);
        } else {
            std::env::remove_var("XAVIER_CODEGRAPH_DUMP_PATH");
        }
    }
}
