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

    /// O1 honesty (US-101, ripwire R1 refuse-vs-empty): the selector matched
    /// nothing. Callers must surface `suggestions` instead of treating this
    /// as an empty result — zero means "none found", never "none exists".
    #[error("Unknown symbol: {name}")]
    UnknownSymbol {
        name: String,
        suggestions: Vec<String>,
    },

    #[error("Query error: {0}")]
    Query(String),

    #[error("Skipped file: {0}")]
    Skipped(String),
}

pub type Result<T> = std::result::Result<T, GraphError>;
