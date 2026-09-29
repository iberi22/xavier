//! Builders that turn raw sources into a document tree.

pub mod legal;
#[cfg(feature = "markdown")]
pub mod markdown;
pub mod plain;
