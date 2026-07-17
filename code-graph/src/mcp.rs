//! MCP Server stub to stabilize code-graph build

pub struct McpServer {
    _indexer: std::sync::Arc<crate::indexer::Indexer>,
    _query_engine: std::sync::Arc<crate::query::QueryEngine>,
    _path: std::path::PathBuf,
}

impl McpServer {
    pub fn new(
        indexer: std::sync::Arc<crate::indexer::Indexer>,
        query_engine: std::sync::Arc<crate::query::QueryEngine>,
        path: std::path::PathBuf,
    ) -> Self {
        Self {
            _indexer: indexer,
            _query_engine: query_engine,
            _path: path,
        }
    }

    pub async fn run(&self) -> crate::error::Result<()> {
        Err(crate::error::GraphError::Parser(
            "MCP Server is not implemented in this version (see issue #466)".to_string(),
        ))
    }
}
