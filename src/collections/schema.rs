//! Document Collection schema for Xavier DocBot.
//!
//! Defines the data model for document management: collections (folders),
//! document entries with versioning, tags, and access control.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

// ── Access Control ────────────────────────────────────────────────────────────

/// Access policy for a document collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CollectionAccess {
    /// Readable by any authenticated user.
    Public,
    /// Only the owner and explicitly invited roles can read.
    #[default]
    Private,
    /// Access gated by one or more RBAC role names.
    RoleGated { roles: Vec<String> },
}

// ── Collection ─────────────────────────────────────────────────────────────────

/// A named, tagged group of documents that can be queried together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocCollection {
    /// ULID primary key.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Free-form tags for filtering.
    pub tags: Vec<String>,
    /// Owner user/agent identifier.
    pub owner_id: Option<String>,
    /// Access policy.
    pub access: CollectionAccess,
    /// Preferred language for RAG prompts (e.g. "es", "en").
    pub language: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl DocCollection {
    /// Create a new collection with sensible defaults.
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Ulid::new().to_string(),
            name: name.into(),
            description: None,
            tags: Vec::new(),
            owner_id: None,
            access: CollectionAccess::default(),
            language: "es".to_string(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_owner(mut self, owner_id: impl Into<String>) -> Self {
        self.owner_id = Some(owner_id.into());
        self
    }

    pub fn with_access(mut self, access: CollectionAccess) -> Self {
        self.access = access;
        self
    }

    pub fn with_language(mut self, lang: impl Into<String>) -> Self {
        self.language = lang.into();
        self
    }
}

// ── Document Entry ─────────────────────────────────────────────────────────────

/// Processing status of an ingested document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DocStatus {
    /// Queued for processing.
    #[default]
    Pending,
    /// Actively being chunked and embedded.
    Processing,
    /// Fully indexed and searchable.
    Ready,
    /// Ingestion failed; see `error_message`.
    Failed,
}

/// A single document within a collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocEntry {
    /// ULID primary key.
    pub id: String,
    /// Parent collection ID.
    pub collection_id: String,
    /// Display title (inferred from filename or metadata).
    pub title: String,
    /// Original file name.
    pub file_name: Option<String>,
    /// Source URL if ingested from the web.
    pub source_url: Option<String>,
    /// MIME type (e.g. "application/pdf").
    pub mime_type: String,
    /// Raw byte size.
    pub raw_size: u64,
    /// Number of semantic chunks generated.
    pub chunk_count: u32,
    /// Monotonically increasing version number within this doc.
    pub version: u32,
    /// Processing status.
    pub status: DocStatus,
    /// Error message if status == Failed.
    pub error_message: Option<String>,
    /// SHA-256 hex digest for dedup.
    pub sha256: Option<String>,
    /// Arbitrary metadata (author, department, year, etc.).
    pub metadata: serde_json::Value,
    pub ingested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl DocEntry {
    /// Create a new pending document entry.
    pub fn new(
        collection_id: impl Into<String>,
        title: impl Into<String>,
        mime_type: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Ulid::new().to_string(),
            collection_id: collection_id.into(),
            title: title.into(),
            file_name: None,
            source_url: None,
            mime_type: mime_type.into(),
            raw_size: 0,
            chunk_count: 0,
            version: 1,
            status: DocStatus::Pending,
            error_message: None,
            sha256: None,
            metadata: serde_json::Value::Object(Default::default()),
            ingested_at: now,
            updated_at: now,
        }
    }

    pub fn with_file_name(mut self, name: impl Into<String>) -> Self {
        self.file_name = Some(name.into());
        self
    }

    pub fn with_source_url(mut self, url: impl Into<String>) -> Self {
        self.source_url = Some(url.into());
        self
    }

    pub fn with_sha256(mut self, hash: impl Into<String>) -> Self {
        self.sha256 = Some(hash.into());
        self
    }

    pub fn with_metadata(mut self, meta: serde_json::Value) -> Self {
        self.metadata = meta;
        self
    }

    pub fn with_raw_size(mut self, size: u64) -> Self {
        self.raw_size = size;
        self
    }
}

// ── Document Chunk ──────────────────────────────────────────────────────────────

/// A searchable chunk stored alongside its embedding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocChunk {
    /// ULID primary key.
    pub id: String,
    /// Parent document ID.
    pub doc_id: String,
    /// Parent collection ID (denormalized for fast queries).
    pub collection_id: String,
    /// Sequential index within the document.
    pub chunk_index: u32,
    /// Text content of this chunk.
    pub content: String,
    /// Page number if applicable (PDF).
    pub page: Option<u32>,
    /// Hierarchical breadcrumb (e.g. "Chapter 2 > Article 15 > Para 1").
    pub breadcrumb: Option<String>,
    /// Estimated token count.
    pub token_count: u32,
    /// Arbitrary per-chunk metadata.
    pub metadata: serde_json::Value,
}

impl DocChunk {
    pub fn new(
        doc_id: impl Into<String>,
        collection_id: impl Into<String>,
        chunk_index: u32,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: Ulid::new().to_string(),
            doc_id: doc_id.into(),
            collection_id: collection_id.into(),
            chunk_index,
            content: content.into(),
            page: None,
            breadcrumb: None,
            token_count: 0,
            metadata: serde_json::Value::Object(Default::default()),
        }
    }

    pub fn with_page(mut self, page: u32) -> Self {
        self.page = Some(page);
        self
    }

    pub fn with_breadcrumb(mut self, crumb: impl Into<String>) -> Self {
        self.breadcrumb = Some(crumb.into());
        self
    }

    pub fn with_token_count(mut self, count: u32) -> Self {
        self.token_count = count;
        self
    }
}

// ── Request / Response DTOs ────────────────────────────────────────────────────

/// Request body for creating a collection.
#[derive(Debug, Deserialize, Serialize)]
pub struct CreateCollectionRequest {
    pub name: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub language: Option<String>,
    pub access: Option<CollectionAccess>,
}

/// Request body for asking a question to a collection.
#[derive(Debug, Deserialize, Serialize)]
pub struct AskRequest {
    pub query: String,
    /// Optional collection scope (None = search all accessible collections).
    pub collection_id: Option<String>,
    /// Maximum number of chunks to retrieve for context.
    pub top_k: Option<usize>,
    /// Language override (defaults to collection language).
    pub language: Option<String>,
}

/// A single cited source returned with a RAG answer.
#[derive(Debug, Serialize, Deserialize)]
pub struct CitedSource {
    pub chunk_id: String,
    pub doc_id: String,
    pub doc_title: String,
    pub collection_id: String,
    pub collection_name: String,
    pub page: Option<u32>,
    pub breadcrumb: Option<String>,
    pub excerpt: String,
    pub score: f32,
}

/// Response body for a RAG ask operation.
#[derive(Debug, Serialize, Deserialize)]
pub struct AskResponse {
    pub answer: String,
    pub sources: Vec<CitedSource>,
    pub query: String,
    pub model_used: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collection_builder() {
        let col = DocCollection::new("Legal Docs")
            .with_description("Regulatory contracts")
            .with_tags(vec!["legal".to_string(), "contracts".to_string()])
            .with_language("es");

        assert_eq!(col.name, "Legal Docs");
        assert_eq!(col.language, "es");
        assert_eq!(col.tags.len(), 2);
        assert!(col.description.is_some());
    }

    #[test]
    fn test_doc_entry_builder() {
        let entry = DocEntry::new("col-123", "Contract 2024", "application/pdf")
            .with_file_name("contract.pdf")
            .with_raw_size(512_000);

        assert_eq!(entry.status, DocStatus::Pending);
        assert_eq!(entry.version, 1);
        assert_eq!(entry.raw_size, 512_000);
    }

    #[test]
    fn test_doc_chunk_builder() {
        let chunk = DocChunk::new("doc-1", "col-1", 0, "This is a legal clause.")
            .with_page(3)
            .with_breadcrumb("Article 5 > Paragraph 2")
            .with_token_count(42);

        assert_eq!(chunk.page, Some(3));
        assert_eq!(chunk.token_count, 42);
    }

    #[test]
    fn test_collection_access_serde() {
        let access = CollectionAccess::RoleGated {
            roles: vec!["admin".to_string(), "legal".to_string()],
        };
        let json = serde_json::to_string(&access).unwrap();
        let back: CollectionAccess = serde_json::from_str(&json).unwrap();
        assert_eq!(access, back);
    }
}
