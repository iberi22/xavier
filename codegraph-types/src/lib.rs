use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: SymbolKind,
    pub name: String,
    pub signature: Option<String>,
    pub file_path: String,
    pub range: Range,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Interface,
    Struct,
    Enum,
    Module,
    Constant,
    Variable,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Range {
    pub start: Point,
    pub end: Point,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Point {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRequest {
    pub version: String,
    pub files: Vec<String>,
    pub config: PluginConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginResponse {
    pub version: String,
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub extensions: Vec<String>,
}

pub enum EdgeKind {
    Call,
    Reference,
    Inheritance,
    Import,
}
