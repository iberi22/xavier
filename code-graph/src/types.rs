//! Core types for code-graph

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Programming language supported.
///
/// `Other(String)` covers languages discovered dynamically from an installed
/// parser plugin (e.g. `Language::Other("ruby")`). The payload is a lowercase
/// canonical language name sourced from the plugin's declared languages.
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
    /// A language backed by a plugin rather than a built-in tree-sitter parser.
    Other(String),
    Unknown,
}

impl std::fmt::Display for IndexStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} files, {} symbols, {} imports across {:?}",
            self.total_files, self.total_symbols, self.total_imports, self.languages
        )
    }
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => Language::Rust,
            "ts" => Language::TypeScript,
            "tsx" => Language::TypeScript,
            "js" => Language::JavaScript,
            "jsx" => Language::JavaScript,
            "py" => Language::Python,
            "go" => Language::Go,
            "java" => Language::Java,
            "c" | "h" => Language::C,
            "cpp" | "cc" | "cxx" | "hpp" => Language::Cpp,
            _ => Language::Unknown,
        }
    }

    /// Resolve an extension to a [`Language`], consulting discovery for plugins
    /// if it's not a built-in.
    pub fn from_extension_with_plugins(ext: &str, discovery: &dyn LanguageDiscovery) -> Self {
        match Self::from_extension(ext) {
            Language::Unknown => discovery.language_for_extension(ext),
            built_in => built_in,
        }
    }

    /// Stable lowercase identifier for this language.
    ///
    /// Used for **DB persistence and API output** instead of `Debug` so that
    /// `Language::Other("ruby")` survives a round-trip (its `Debug` form,
    /// `Other("ruby")`, does not). For `Other(s)` this returns the inner
    /// canonical name (`s`), lowercased.
    pub fn as_str(&self) -> &str {
        match self {
            Language::Rust => "rust",
            Language::TypeScript => "typescript",
            Language::JavaScript => "javascript",
            Language::Python => "python",
            Language::Go => "go",
            Language::Java => "java",
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::Other(name) => name,
            Language::Unknown => "unknown",
        }
    }

    /// Storage form used when writing the `lang` column to SQLite. For the
    /// `Other(String)` variant this is `other:<name>` so the payload survives
    /// the round-trip; built-ins use their bare `as_str()`.
    pub fn as_db_str(&self) -> String {
        match self {
            Language::Other(name) => format!("other:{}", name),
            other => other.as_str().to_string(),
        }
    }

    /// Parse a value previously written by [`Language::as_db_str`] (or by the
    /// legacy `Debug`/serde forms on existing rows) back into a `Language`.
    pub fn from_db_str(value: &str) -> Self {
        // New canonical form for plugin-backed languages.
        if let Some(name) = value.strip_prefix("other:") {
            return Language::Other(name.to_string());
        }
        // Canonical lowercase identifiers produced by `as_str`.
        match value {
            "rust" => Language::Rust,
            "typescript" => Language::TypeScript,
            "javascript" => Language::JavaScript,
            "python" => Language::Python,
            "go" => Language::Go,
            "java" => Language::Java,
            "c" => Language::C,
            "cpp" => Language::Cpp,
            "unknown" => Language::Unknown,
            _ => {
                // Legacy rows written with `Debug` ("Rust", "Cpp", ...) or
                // externally-tagged serde ("\"Rust\""). Try serde first, then
                // the bare capitalized form.
                serde_json::from_str(value).unwrap_or(match value {
                    "Rust" => Language::Rust,
                    "TypeScript" => Language::TypeScript,
                    "JavaScript" => Language::JavaScript,
                    "Python" => Language::Python,
                    "Go" => Language::Go,
                    "Java" => Language::Java,
                    "C" => Language::C,
                    "Cpp" => Language::Cpp,
                    _ => Language::Unknown,
                })
            }
        }
    }
}

/// Symbol type in the codebase
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
    // Advanced structural types
    Route,
    Component,
    Property,
    Field,
    Parameter,
    TypeAlias,
    Namespace,
    Symbol, // Fallback
}

/// Relationship type between indexed code entities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EdgeType {
    Calls,
    Defines,
    Uses,
    Imports,
    Exports,
    Contains,
    References,
    Extends,
    Implements,
    TypeOf,
    Returns,
    Instantiates,
    Overrides,
    Decorates,
}

impl EdgeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeType::Calls => "Calls",
            EdgeType::Defines => "Defines",
            EdgeType::Uses => "Uses",
            EdgeType::Imports => "Imports",
            EdgeType::Exports => "Exports",
            EdgeType::Contains => "Contains",
            EdgeType::References => "References",
            EdgeType::Extends => "Extends",
            EdgeType::Implements => "Implements",
            EdgeType::TypeOf => "TypeOf",
            EdgeType::Returns => "Returns",
            EdgeType::Instantiates => "Instantiates",
            EdgeType::Overrides => "Overrides",
            EdgeType::Decorates => "Decorates",
        }
    }
}

/// A code symbol with location
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
    pub parent: Option<String>, // parent struct/class
    pub complexity: Option<f32>,
}

impl Symbol {
    /// Structural identity: project + path + name + kind + parent + signature.
    ///
    /// Does **not** include `start_line`, so moving a symbol within a file keeps
    /// the same id (edges and memory chunks remain stable across edits).
    pub fn deterministic_id(&self, project_id: &str) -> String {
        stable_symbol_id(
            project_id,
            &self.file_path,
            &self.name,
            &format!("{:?}", self.kind),
            self.parent.as_deref(),
            self.signature.as_deref(),
        )
    }

    pub fn stable_key(&self, project_id: &str) -> String {
        self.stable_id
            .clone()
            .unwrap_or_else(|| self.deterministic_id(project_id))
    }
}

/// A graph edge. Endpoints are stable symbol IDs or prefixed pseudo-nodes:
/// `file:<path>` and `module:<name>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeEdge {
    pub id: Option<i64>,
    pub from_symbol: String,
    pub to_symbol: String,
    pub edge_type: EdgeType,
    pub file_path: String,
    pub line: u32,
    pub confidence: f32,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubNode {
    pub symbol: Symbol,
    pub incoming: u64,
    pub outgoing: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityHotspot {
    pub symbol: Symbol,
    pub incoming: u64,
    pub outgoing: u64,
    pub risk_score: f32,
}

/// Link between an agent memory record and a code symbol
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemorySymbolLink {
    pub memory_id: String,
    pub symbol_id: String,
    pub confidence: f64,
}

/// Content-addressed structural symbol id (v2).
///
/// Hash input: `project|file|name|kind|parent|signature` (normalized whitespace
/// in signature). Line numbers are intentionally excluded so git-driven moves
/// do not invalidate graph edges.
pub fn stable_symbol_id(
    project_id: &str,
    file_path: &str,
    name: &str,
    kind: &str,
    parent: Option<&str>,
    signature: Option<&str>,
) -> String {
    let parent = parent.unwrap_or("");
    let signature = signature.map(normalize_signature).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(b"v2|");
    hasher.update(project_id.as_bytes());
    hasher.update(b"|");
    hasher.update(file_path.as_bytes());
    hasher.update(b"|");
    hasher.update(name.as_bytes());
    hasher.update(b"|");
    hasher.update(kind.as_bytes());
    hasher.update(b"|");
    hasher.update(parent.as_bytes());
    hasher.update(b"|");
    hasher.update(signature.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn normalize_signature(sig: &str) -> String {
    sig.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod stable_id_tests {
    use super::*;

    #[test]
    fn structural_id_stable_across_line_moves() {
        let mut a = Symbol {
            name: "helper".into(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "lib.rs".into(),
            start_line: 1,
            end_line: 3,
            signature: Some("fn helper()".into()),
            parent: None,
            ..Default::default()
        };
        let id1 = a.deterministic_id("default");
        a.start_line = 40;
        a.end_line = 42;
        let id2 = a.deterministic_id("default");
        assert_eq!(id1, id2, "moving a symbol must keep structural stable_id");
    }

    #[test]
    fn parent_and_signature_disambiguate() {
        let base = Symbol {
            name: "run".into(),
            kind: SymbolKind::Method,
            lang: Language::Rust,
            file_path: "svc.rs".into(),
            signature: Some("fn run(&self)".into()),
            parent: Some("Service".into()),
            ..Default::default()
        };
        let other = Symbol {
            parent: Some("Other".into()),
            ..base.clone()
        };
        assert_ne!(
            base.deterministic_id("default"),
            other.deterministic_id("default")
        );
    }
}

/// Reference to a symbol (caller/callee)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Reference {
    pub symbol_id: i64,
    pub file_path: String,
    pub line: u32,
    pub column: u32,
    pub context: String, // surrounding code
}

/// Import/dependency relationship
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Import {
    pub from: String,
    pub to: String,
    pub file_path: String,
    pub line: u32,
}

/// Indexing statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub total_files: u64,
    pub total_symbols: u64,
    pub total_imports: u64,
    pub languages: Vec<LanguageCount>,
    pub duration_ms: u64,
}

/// Trait for discovering languages supported by plugins.
pub trait LanguageDiscovery: Send + Sync {
    /// Resolve an extension to a [`Language`].
    fn language_for_extension(&self, ext: &str) -> Language;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageCount {
    pub lang: Language,
    pub count: u64,
}

/// Query result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub symbols: Vec<Symbol>,
    pub total: usize,
    pub query_time_ms: u64,
}
