//! Standalone PageIndex-style tree retrieval. Must not depend on `xavier`.

pub mod builders;
pub mod error;
pub mod eval;
pub mod model;
pub mod optimize;
#[cfg(any(feature = "pdf-outline", feature = "pdf-layout"))]
pub mod pdf;
pub mod query;
pub mod store;
pub mod summarize;

pub use error::PageIndexError;
pub use model::{DocStatus, Document, DocumentTree, Page, PageUnit, SourceKind, TreeNode};

/// Facade over a store; methods arrive in later issues.
pub struct PageIndex<S> {
    #[allow(dead_code)]
    store: S,
}

impl<S> PageIndex<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }
}
