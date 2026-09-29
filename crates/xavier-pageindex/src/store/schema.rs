//! SQL schema for the tree store.

/// Current schema version, recorded in `pi_schema_version`.
pub const SCHEMA_VERSION: i64 = 1;

pub const DDL_V1: &str = "
CREATE TABLE IF NOT EXISTS pi_schema_version (
    version INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS pi_documents (
    doc_id       TEXT PRIMARY KEY,
    workspace    TEXT NOT NULL,
    name         TEXT NOT NULL,
    source_kind  TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    page_count   INTEGER NOT NULL,
    page_unit    TEXT NOT NULL,
    status       TEXT NOT NULL,
    builder      TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    UNIQUE (workspace, name)
);
CREATE TABLE IF NOT EXISTS pi_nodes (
    doc_id         TEXT NOT NULL REFERENCES pi_documents(doc_id) ON DELETE CASCADE,
    node_id        TEXT NOT NULL,
    parent_id      TEXT,
    ord            INTEGER NOT NULL,
    title          TEXT NOT NULL,
    start_page     INTEGER NOT NULL,
    end_page       INTEGER NOT NULL,
    summary        TEXT,
    token_estimate INTEGER NOT NULL,
    PRIMARY KEY (doc_id, node_id)
);
CREATE TABLE IF NOT EXISTS pi_pages (
    doc_id  TEXT NOT NULL REFERENCES pi_documents(doc_id) ON DELETE CASCADE,
    page_no INTEGER NOT NULL,
    text    TEXT NOT NULL,
    PRIMARY KEY (doc_id, page_no)
);
";
