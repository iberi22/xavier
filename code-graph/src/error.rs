//! Error types for code-graph

use thiserror::Error;

#[derive(Error, Debug)]
pub enum GraphError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Parser error: {0}")]
    Parser(String),

    #[error("Tree-sitter error: {0}")]
    TreeSitter(String),

    #[error("Language not supported: {0}")]
    LanguageNotSupported(String),

    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),

    #[error("Query error: {0}")]
    Query(String),

    #[error("Indexer error: {0}")]
    Indexer(String),

    /// Plugin registry failures: index fetch, network, schema mismatch,
    /// unknown plugin name, min-engine-version mismatch, etc.
    #[error("Registry error: {0}")]
    Registry(String),

    /// Plugin lifecycle/execution failures: install, checksum mismatch,
    /// extraction, version resolution, circuit breaker open, etc.
    #[error("Plugin error: {0}")]
    Plugin(String),
}

pub type Result<T> = std::result::Result<T, GraphError>;
