use super::CodeGraphDB;
use crate::error::{GraphError, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct TraversalPath {
    pub path_str: String,
    pub depth: u32,
    pub target_symbol: String,
}

impl CodeGraphDB {
    /// Executes a Cypher-like recursive CTE to find paths between symbols
    /// Pattern adapted from codebase-memory-mcp
    pub fn trace_path(
        &self,
        start_symbol: &str,
        max_depth: u32,
        reverse: bool,
    ) -> Result<Vec<TraversalPath>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;

        // If reverse = false, we trace who start_symbol CALLS (callees)
        // If reverse = true, we trace who calls start_symbol (callers)
        let (anchor_col, target_col) = if reverse {
            ("to_symbol", "from_symbol")
        } else {
            ("from_symbol", "to_symbol")
        };

        let query = format!(
            r#"
            WITH RECURSIVE path_cte(current_symbol, depth, path_str) AS (
                SELECT {}, 1, {}
                FROM edges
                WHERE {} = ?
                
                UNION ALL
                
                SELECT e.{}, p.depth + 1, p.path_str || ' -> ' || e.{}
                FROM edges e
                JOIN path_cte p ON e.{} = p.current_symbol
                WHERE p.depth < ?
            )
            SELECT DISTINCT current_symbol, depth, path_str FROM path_cte
            ORDER BY depth ASC;
            "#,
            target_col, target_col, anchor_col, target_col, target_col, anchor_col
        );

        let mut stmt = conn
            .prepare(&query)
            .map_err(|e| GraphError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![start_symbol, max_depth], |row| {
                Ok(TraversalPath {
                    target_symbol: row.get(0)?,
                    depth: row.get(1)?,
                    path_str: row.get(2)?,
                })
            })
            .map_err(|e| GraphError::Database(e.to_string()))?;

        let mut paths = Vec::new();
        for path in rows.flatten() {
            paths.push(path);
        }

        Ok(paths)
    }
}
