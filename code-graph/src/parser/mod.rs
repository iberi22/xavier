//! Parser module - tree-sitter based

use crate::error::Result;
use crate::parser::c::CParser;
use crate::parser::cpp::CppParser;
use crate::parser::go::GoParser;
use crate::parser::java::JavaParser;
use crate::parser::python::PythonParser;
use crate::parser::rust::RustParser;
use crate::plugin_host::{FileToParse, ParserDispatch, PluginHost};
use crate::types::{Language, Symbol, SymbolKind};
use tree_sitter::Node;

pub mod c;
pub mod cpp;
pub mod go;
pub mod java;
pub mod python;
pub mod rust;
pub mod typescript;

use crate::parser::typescript::TypeScriptParser;

/// Parse source code using tree-sitter or a plugin
pub async fn parse_source(
    source: &str,
    lang: &Language,
    file_path: &str,
    plugin_host: Option<&PluginHost>,
) -> Result<Vec<Symbol>> {
    let dispatch = if let Some(host) = plugin_host {
        host.parser_for(lang)
    } else {
        ParserDispatch::Native
    };

    match dispatch {
        ParserDispatch::Plugin(config) => {
            let files = vec![FileToParse {
                path: file_path.to_string(),
                source: source.to_string(),
            }];
            plugin_host
                .unwrap()
                .parse_with_plugin(&config, lang.clone(), files)
                .await
                .map_err(|e| {
                    tracing::warn!("Plugin parser failed for {}: {}", file_path, e);
                    e
                })
        }

        ParserDispatch::Native => match lang {
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
            Language::Python => {
                let mut parser = PythonParser::new()?;
                parser.parse(source, file_path)
            }
            Language::TypeScript | Language::JavaScript => {
                let mut parser = TypeScriptParser::new(lang.clone())?;
                parser.parse(source, file_path)
            }
            _ => Ok(vec![]),
        },
        ParserDispatch::NoOp => {
            tracing::debug!("No parser available for language {:?}", lang);
            Ok(vec![])
        }
    }
}

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

#[cfg(test)]
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
