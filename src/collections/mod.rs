//! Document Collections module for Xavier DocBot.
//!
//! Provides document management: collections, entries, chunks, ingestion pipeline.

pub mod indexer;
pub mod schema;
pub mod store;

pub use indexer::{
    ingest_document, semantic_chunk, ChunkConfig, IngestResult, VecMatch, VecStoreAdapter,
};
pub use schema::{
    AskRequest, AskResponse, CitedSource, CollectionAccess, CreateCollectionRequest, DocChunk,
    DocCollection, DocEntry, DocStatus,
};
pub use store::{ChunkSearchResult, CollectionStore};
