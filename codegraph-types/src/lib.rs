use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum Language {
    #[default]
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Java,
    C,
    Cpp,
    Other(String),
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum SymbolKind {
    #[default]
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Class,
    Method,
    Variable,
    Constant,
    Import,
    Export,
    Module,
    File,
    Route,
    Component,
    Property,
    Field,
    Parameter,
    TypeAlias,
    Namespace,
    Symbol,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Symbol {
    pub id: Option<i64>,
    pub stable_id: Option<String>,
    pub name: String,
    pub kind: SymbolKind,
    pub lang: Language,
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub start_col: u32,
    pub end_col: u32,
    pub signature: Option<String>,
    pub parent: Option<String>,
    pub complexity: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginRequest {
    pub language: Language,
    pub files: Vec<FileToParse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileToParse {
    pub path: String,
    pub source: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginResponse {
    pub symbols: Vec<Symbol>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub name: String,
    pub command: String,
    pub version: String,
    pub languages: Vec<Language>,
    pub extensions: Option<Vec<String>>,
    pub capabilities: Vec<String>,
}
