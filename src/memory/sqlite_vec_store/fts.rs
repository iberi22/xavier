//! Full-text search for SQLite vector store
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
pub use crate::domain::cycle_breaks::w30_10::{
    build_fts_query, code_tokens, search_tokens, split_camel_case,
};
