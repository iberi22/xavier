//! Parser module - tree-sitter based

use crate::error::Result;
#[cfg(feature = "native-parsers")]
use crate::parser::c::CParser;
#[cfg(feature = "native-parsers")]
use crate::parser::cpp::CppParser;
#[cfg(feature = "native-parsers")]
use crate::parser::go::GoParser;
#[cfg(feature = "native-parsers")]
use crate::parser::java::JavaParser;
#[cfg(feature = "native-parsers")]
use crate::parser::python::PythonParser;
#[cfg(feature = "native-parsers")]
use crate::parser::rust::RustParser;
use crate::plugin::types::{FallbackStep, FileToParse};
use crate::plugin::PluginManager;
#[cfg(feature = "native-parsers")]
use crate::types::SymbolKind;
use crate::types::{Language, Symbol};
#[cfg(feature = "native-parsers")]
use tree_sitter::Node;

#[cfg(feature = "native-parsers")]
pub mod c;
#[cfg(feature = "native-parsers")]
pub mod cpp;
#[cfg(feature = "native-parsers")]
pub mod go;
#[cfg(feature = "native-parsers")]
pub mod java;
#[cfg(feature = "native-parsers")]
pub mod python;
#[cfg(feature = "native-parsers")]
pub mod rust;
#[cfg(feature = "native-parsers")]
pub mod typescript;

#[cfg(feature = "native-parsers")]
use crate::parser::typescript::TypeScriptParser;

/// True when `lang` is backed by a built-in tree-sitter parser.
///
/// Used by the fallback chain to pick a sensible default and by callers that
/// want to know whether `parse_native` can ever succeed for a language.
pub fn has_native_parser(lang: &Language) -> bool {
    #[cfg(feature = "native-parsers")]
    {
        matches!(
            lang,
            Language::Rust
                | Language::TypeScript
                | Language::JavaScript
                | Language::Python
                | Language::Go
                | Language::Java
                | Language::C
                | Language::Cpp
        )
    }
    #[cfg(not(feature = "native-parsers"))]
    {
        let _ = lang;
        false
    }
}

/// Run the built-in tree-sitter parser for `lang`.
///
/// Returns `Ok(empty)` for languages without a native parser rather than an
/// error, so the fallback chain can treat "no native parser" the same as
/// "native parser found nothing".
pub fn parse_native(source: &str, lang: &Language, file_path: &str) -> Result<Vec<Symbol>> {
    #[cfg(feature = "native-parsers")]
    {
        match lang {
            Language::Rust => {
                let mut parser = RustParser::new()?;
                parser.parse(source, file_path)
            }
            Language::TypeScript | Language::JavaScript => {
                let mut parser = TypeScriptParser::new(lang.clone())?;
                parser.parse(source, file_path)
            }
            Language::Python => {
                let mut parser = PythonParser::new()?;
                parser.parse(source, file_path)
            }
            Language::Go => {
                let mut parser = GoParser::new()?;
                parser.parse(source, file_path)
            }
            Language::Java => {
                let mut parser = JavaParser::new()?;
                parser.parse(source, file_path)
            }
            Language::C => {
                let mut parser = CParser::new()?;
                parser.parse(source, file_path)
            }
            Language::Cpp => {
                let mut parser = CppParser::new()?;
                parser.parse(source, file_path)
            }
            // Plugin-only or unknown languages have no native parser.
            Language::Other(_) | Language::Unknown => Ok(vec![]),
        }
    }
    #[cfg(not(feature = "native-parsers"))]
    {
        let _ = (source, lang, file_path);
        Ok(vec![])
    }
}

/// Parse source code via the per-language fallback chain.
///
/// Resolution order, when a [`PluginManager`] is supplied:
/// 1. The manager's live chain for `lang` (plugin-first if one is registered),
///    falling back to its persisted overrides, then to the default chain.
/// 2. Without a manager, the default chain applies (`[Native, NoOp]`).
///
/// Each step that errors is logged at `warn`; a step that succeeds short-
/// circuits the chain. If every step fails or the chain ends in `NoOp`, an
/// empty `Vec<Symbol>` is returned — this function never propagates a parse
/// failure as a hard error so a single bad plugin can never crash the indexer.
pub async fn parse_source(
    source: &str,
    lang: &Language,
    file_path: &str,
    plugin_manager: Option<&PluginManager>,
) -> Result<Vec<Symbol>> {
    let chain = match plugin_manager {
        Some(mgr) => mgr.chain_for(lang),
        None => default_chain(lang),
    };

    for step in &chain {
        match step {
            FallbackStep::Plugin(name) => {
                let Some(mgr) = plugin_manager else { continue };

                // Circuit breaker: skip plugin if its health circuit is Open.
                if let Some(health) = mgr.health() {
                    if health.is_open(name) {
                        tracing::warn!(
                            plugin = %name,
                            "plugin circuit is OPEN, skipping to next fallback step"
                        );
                        continue;
                    }
                }

                let files = vec![FileToParse {
                    path: file_path.to_string(),
                    source: source.to_string(),
                }];
                match mgr.parse_with_plugin(name, lang.clone(), files).await {
                    Ok(symbols) => return Ok(symbols),
                    Err(e) => tracing::warn!(
                        plugin = %name,
                        file = %file_path,
                        error = %e,
                        "plugin parse failed, continuing fallback chain"
                    ),
                }
            }
            FallbackStep::Native => {
                return parse_native(source, lang, file_path);
            }
            FallbackStep::NoOp => {
                tracing::debug!(
                    lang = ?lang,
                    file = %file_path,
                    "no parser available (NoOp)"
                );
                return Ok(vec![]);
            }
        }
    }

    // Chain exhausted without hitting Native/NoOp explicitly.
    Ok(vec![])
}

/// Default chain used when no [`PluginManager`] is present.
fn default_chain(lang: &Language) -> Vec<FallbackStep> {
    if has_native_parser(lang) {
        vec![FallbackStep::Native, FallbackStep::NoOp]
    } else {
        vec![FallbackStep::NoOp]
    }
}

#[cfg(feature = "native-parsers")]
pub(crate) fn compact_node_signature(node: Node, source: &str) -> Option<String> {
    let raw = node.utf8_text(source.as_bytes()).ok()?;
    let header = if let Some(body) = node.child_by_field_name("body") {
        let index = body.start_byte().saturating_sub(node.start_byte());
        format!("{} {{ ... }}", raw.get(..index).unwrap_or(raw))
    } else {
        match raw.find('{') {
            Some(index) => format!("{} {{ ... }}", &raw[..index]),
            None => raw
                .find(';')
                .map(|index| raw[..=index].to_string())
                .unwrap_or_else(|| raw.lines().next().unwrap_or(raw).to_string()),
        }
    };

    let compact = header.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        None
    } else if compact.len() > 400 {
        Some(format!(
            "{}...",
            compact.chars().take(400).collect::<String>()
        ))
    } else {
        Some(compact)
    }
}

/// Arguments for pushing a symbol
#[cfg(feature = "native-parsers")]
pub struct PushSymbolArgs<'src> {
    pub node: Node<'src>,
    pub source: &'src str,
    pub language: Language,
    pub kind: SymbolKind,
    pub file_path: &'src str,
    pub name: String,
    pub depth: usize,
    pub parent: Option<String>,
}

#[cfg(feature = "native-parsers")]
pub(crate) fn cyclomatic_complexity(node: Node, source: &str) -> f32 {
    fn count(node: Node, _source: &str) -> usize {
        let kind = node.kind();
        let mut total = if matches!(
            kind,
            "if_expression"
                | "if_statement"
                | "elif_clause"
                | "else_if_clause"
                | "while_expression"
                | "while_statement"
                | "for_expression"
                | "for_statement"
                | "for_in_clause"
                | "enhanced_for_statement"
                | "loop_expression"
                | "match_expression"
                | "match_statement"
                | "switch_statement"
                | "case"
                | "conditional_expression"
                | "catch_clause"
                | "except_clause"
        ) {
            1
        } else {
            0
        };

        if kind == "binary_expression" || kind == "boolean_operator" {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                let ck = child.kind();
                if ck == "&&" || ck == "||" || ck == "and" || ck == "or" {
                    total += 1;
                }
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            total += count(child, _source);
        }
        total
    }

    1.0 + count(node, source) as f32
}

#[cfg(all(test, feature = "native-parsers"))]
mod tests {
    use super::*;
    use crate::types::SymbolKind;

    #[tokio::test]
    async fn parses_typescript_symbols() {
        let symbols = parse_source(
            "import x from 'pkg';\nclass UserService { run() {} }\nfunction main() {}\nconst load = () => main();\nenum Color { Red }\nconst a = 1, b = 2;\nlet count = 0;",
            &Language::TypeScript,
            "app.ts",
            None,
        )
        .await
        .expect("parse");
        assert!(symbols
            .iter()
            .any(|s| s.name == "UserService" && s.kind == SymbolKind::Class));
        assert!(symbols
            .iter()
            .any(|s| s.name == "main" && s.kind == SymbolKind::Function));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Import));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Color" && s.kind == SymbolKind::Enum));
        assert!(symbols
            .iter()
            .any(|s| s.name == "a" && s.kind == SymbolKind::Constant));
        assert!(symbols
            .iter()
            .any(|s| s.name == "b" && s.kind == SymbolKind::Constant));
        assert!(symbols
            .iter()
            .any(|s| s.name == "count" && s.kind == SymbolKind::Variable));
    }

    #[tokio::test]
    async fn parses_python_symbols() {
        let symbols = parse_source(
            "import os\nclass Service:\n    VERSION = 1\n    async def run(self):\n        return os.getcwd()\nx, y = (1, 2)",
            &Language::Python,
            "app.py",
            None,
        )
        .await
        .expect("parse");
        assert!(symbols
            .iter()
            .any(|s| s.name == "Service" && s.kind == SymbolKind::Class));
        assert!(symbols
            .iter()
            .any(|s| s.name == "run" && s.kind == SymbolKind::Method));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Import));
        assert!(symbols.iter().any(|s| s.name == "VERSION"
            && s.kind == SymbolKind::Variable
            && s.parent.as_deref() == Some("Service")));
        assert!(symbols
            .iter()
            .any(|s| s.name == "x" && s.kind == SymbolKind::Variable));
        assert!(symbols
            .iter()
            .any(|s| s.name == "y" && s.kind == SymbolKind::Variable));
    }

    #[tokio::test]
    async fn parses_go_symbols() {
        let symbols = parse_source(
            "package main\nimport \"fmt\"\ntype User struct{}\nfunc (u *User) GetName() string { return \"\" }\nconst Max = 10\nvar count = 0\nfunc main() { fmt.Println(\"x\") }\n",
            &Language::Go,
            "main.go",
            None,
        )
        .await
        .expect("parse");
        assert!(symbols
            .iter()
            .any(|s| s.name == "User" && s.kind == SymbolKind::Struct));
        assert!(symbols
            .iter()
            .any(|s| s.name == "main" && s.kind == SymbolKind::Function));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Import));
        assert!(symbols.iter().any(|s| s.name == "GetName"
            && s.kind == SymbolKind::Method
            && s.parent.as_deref() == Some("User")));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Max" && s.kind == SymbolKind::Constant));
        assert!(symbols
            .iter()
            .any(|s| s.name == "count" && s.kind == SymbolKind::Variable));
    }

    #[tokio::test]
    async fn parses_java_symbols() {
        let symbols = parse_source(
            "import java.util.List; class Service { void run() {} }",
            &Language::Java,
            "Service.java",
            None,
        )
        .await
        .expect("parse");
        assert!(symbols
            .iter()
            .any(|s| s.name == "Service" && s.kind == SymbolKind::Class));
        assert!(symbols
            .iter()
            .any(|s| s.name == "run" && s.kind == SymbolKind::Method));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Import));
    }

    #[tokio::test]
    async fn parses_c_symbols() {
        let symbols = parse_source(
            "#include <stdio.h>\n#define MAX 100\nstruct Point { int x; };\nvoid main() { printf(\"hello\"); }",
            &Language::C,
            "main.c",
            None,
        )
        .await
        .expect("parse");
        assert!(symbols
            .iter()
            .any(|s| s.name == "main" && s.kind == SymbolKind::Function));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Point" && s.kind == SymbolKind::Struct));
        assert!(symbols
            .iter()
            .any(|s| s.name == "MAX" && s.kind == SymbolKind::Constant));
        assert!(symbols
            .iter()
            .any(|s| s.name == "stdio.h" && s.kind == SymbolKind::Import));
    }

    #[tokio::test]
    async fn parses_cpp_symbols() {
        let symbols = parse_source(
            "#include <iostream>\nnamespace xav { class Scanner { public: void run() {} }; }\nint main() { return 0; }",
            &Language::Cpp,
            "main.cpp",
            None,
        )
        .await
        .expect("parse");
        assert!(symbols
            .iter()
            .any(|s| s.name == "main" && s.kind == SymbolKind::Function));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Scanner" && s.kind == SymbolKind::Class));
        assert!(symbols.iter().any(|s| s.name == "run"
            && s.kind == SymbolKind::Method
            && s.parent.as_deref() == Some("Scanner")));
        assert!(symbols
            .iter()
            .any(|s| s.name == "xav" && s.kind == SymbolKind::Module));
        assert!(symbols
            .iter()
            .any(|s| s.name == "iostream" && s.kind == SymbolKind::Import));
    }
}
