use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Struct,
    Class,
    Enum,
    Trait,
    Impl,
    Interface,
    Method,
    Variable,
    Constant,
    Module,
    File,
    Route,
    Component,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EdgeKind {
    Calls,
    Imports,
    Extends,
    Implements,
    Contains,
    References,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: SymbolKind,
    pub name: String,
    pub qual_name: Option<String>,
    pub file_path: String,
    pub language: String,
    pub position: Position,
    pub signature: Option<String>,
    pub docstring: Option<String>,
    pub visibility: Option<String>,
    pub modifiers: serde_json::Value,
    pub parent_id: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub source_id: String,
    pub target_id: String,
    pub kind: EdgeKind,
    pub line: u32,
    pub col: u32,
    pub metadata: serde_json::Value,
    pub provenance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct File {
    pub path: String,
    pub language: String,
    pub size: u64,
    pub content_hash: String,
    pub modified_at: i64,
    pub indexed_at: i64,
}
