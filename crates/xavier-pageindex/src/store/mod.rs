//! Tree storage backends.

pub mod schema;
#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteStore;

use crate::error::PageIndexError;
use crate::model::{Document, DocumentTree, Page};

/// Result of storing a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutOutcome {
    pub doc_id: String,
    /// True when an identical (same workspace, name, content hash) document
    /// already existed and nothing was written.
    pub reused: bool,
}

/// Synchronous tree store; async callers wrap it in `spawn_blocking`.
pub trait Store {
    /// Store a document with its tree and pages. Re-ingesting the same
    /// `content_hash` under the same `(workspace, name)` is a no-op; a changed
    /// hash replaces the previous document.
    fn put_document(
        &self,
        doc: &Document,
        tree: &DocumentTree,
        pages: &[Page],
    ) -> Result<PutOutcome, PageIndexError>;

    fn get_document(
        &self,
        workspace: &str,
        doc_id: &str,
    ) -> Result<Option<Document>, PageIndexError>;

    fn find_by_name(&self, workspace: &str, name: &str)
        -> Result<Option<Document>, PageIndexError>;

    fn get_tree(
        &self,
        workspace: &str,
        doc_id: &str,
    ) -> Result<Option<DocumentTree>, PageIndexError>;

    /// Pages `start..=end` (1-based, inclusive), ordered by page number.
    fn get_pages(
        &self,
        workspace: &str,
        doc_id: &str,
        start: u32,
        end: u32,
    ) -> Result<Vec<Page>, PageIndexError>;

    fn list_documents(&self, workspace: &str) -> Result<Vec<Document>, PageIndexError>;

    /// Documents whose name or any node title contains `query` (case-insensitive).
    fn search_by_title(
        &self,
        workspace: &str,
        query: &str,
    ) -> Result<Vec<Document>, PageIndexError>;

    /// Delete a document and its nodes/pages. Returns whether it existed.
    fn delete_document(&self, workspace: &str, doc_id: &str) -> Result<bool, PageIndexError>;
}
