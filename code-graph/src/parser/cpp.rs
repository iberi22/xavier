//! C++ parser using tree-sitter.

use crate::error::{GraphError, Result};
use crate::parser::{compact_node_signature, cyclomatic_complexity, PushSymbolArgs};
use crate::types::{Language, Symbol, SymbolKind};
use tree_sitter::{Node, Parser};

pub struct CppParser {
    parser: Parser,
}

impl Default for CppParser {
    fn default() -> Self {
        Self::new().expect("failed to initialize C++ parser")
    }
}

impl CppParser {
    pub fn new() -> Result<Self> {
        let mut parser = Parser::new();
        let language = tree_sitter_cpp::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| GraphError::TreeSitter(format!("failed to set C++ language: {}", e)))?;
        Ok(Self { parser })
    }

    pub fn parse(&mut self, source: &str, file_path: &str) -> Result<Vec<Symbol>> {
        let tree = self
            .parser
            .parse(source.as_bytes(), None)
            .ok_or_else(|| GraphError::Parser("Failed to parse C++ source".to_string()))?;
        let mut symbols = Vec::new();
        self.extract(tree.root_node(), source, file_path, &mut symbols, None);
        Ok(symbols)
    }

    fn extract(
        &self,
        node: Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Symbol>,
        parent: Option<String>,
    ) {
        match node.kind() {
            "function_definition" => {
                let name = self.extract_function_name(node, source);
                if let Some(name) = name {
                    let kind = if parent.is_some() {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    };
                    self.push_symbol(
                        symbols,
                        PushSymbolArgs {
                            node,
                            source,
                            language: Language::Cpp,
                            kind,
                            file_path,
                            name,
                            depth: 0,
                            parent: parent.clone(),
                        },
                    );
                }
            }
            "class_specifier" | "struct_specifier" => {
                let kind = if node.kind() == "class_specifier" {
                    SymbolKind::Class
                } else {
                    SymbolKind::Struct
                };
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                        let name_str = name.to_string();
                        self.push_symbol(
                            symbols,
                            PushSymbolArgs {
                                node,
                                source,
                                language: Language::Cpp,
                                kind,
                                file_path,
                                name: name_str.clone(),
                                depth: 0,
                                parent: parent.clone(),
                            },
                        );
                        if let Some(body) = node.child_by_field_name("body") {
                            self.extract_children(body, source, file_path, symbols, Some(name_str));
                        }
                        return;
                    }
                }
            }
            "enum_specifier" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                        self.push_symbol(
                            symbols,
                            PushSymbolArgs {
                                node,
                                source,
                                language: Language::Cpp,
                                kind: SymbolKind::Enum,
                                file_path,
                                name: name.to_string(),
                                depth: 0,
                                parent: parent.clone(),
                            },
                        );
                    }
                }
            }
            "namespace_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                        self.push_symbol(
                            symbols,
                            PushSymbolArgs {
                                node,
                                source,
                                language: Language::Cpp,
                                kind: SymbolKind::Module,
                                file_path,
                                name: name.to_string(),
                                depth: 0,
                                parent: parent.clone(),
                            },
                        );
                    }
                }
            }
            "preproc_include" => {
                self.push_import(node, source, file_path, symbols);
            }
            _ => {}
        }

        self.extract_children(node, source, file_path, symbols, parent);
    }

    fn extract_children(
        &self,
        node: Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Symbol>,
        parent: Option<String>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract(child, source, file_path, symbols, parent.clone());
        }
    }

    fn extract_function_name(&self, node: Node, source: &str) -> Option<String> {
        let declarator = node.child_by_field_name("declarator")?;
        self.find_identifier(declarator, source)
    }

    fn find_identifier(&self, node: Node, source: &str) -> Option<String> {
        match node.kind() {
            "identifier" | "field_identifier" => {
                return node
                    .utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.to_string());
            }
            "qualified_identifier" | "destructor_name" => {
                return node
                    .utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.to_string());
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(name) = self.find_identifier(child, source) {
                return Some(name);
            }
        }
        None
    }

    fn push_import(&self, node: Node, source: &str, file_path: &str, symbols: &mut Vec<Symbol>) {
        let raw = node.utf8_text(source.as_bytes()).unwrap_or_default();
        let name = raw
            .trim_start_matches("#include")
            .trim()
            .trim_matches(|c| c == '<' || c == '>' || c == '"')
            .to_string();
        self.push_symbol(
            symbols,
            PushSymbolArgs {
                node,
                source,
                language: Language::Cpp,
                kind: SymbolKind::Import,
                file_path,
                name,
                depth: 0,
                parent: None,
            },
        );
    }

    fn push_symbol(&self, symbols: &mut Vec<Symbol>, args: PushSymbolArgs<'_>) {
        let start = args.node.start_position();
        let end = args.node.end_position();
        let complexity = matches!(args.kind, SymbolKind::Function | SymbolKind::Method)
            .then(|| cyclomatic_complexity(args.node, args.source));
        symbols.push(Symbol {
            id: None,
            stable_id: None,
            name: args.name,
            kind: args.kind,
            lang: Language::Cpp,
            file_path: args.file_path.to_string(),
            start_line: (start.row + 1) as u32,
            end_line: (end.row + 1) as u32,
            start_col: start.column as u32,
            end_col: end.column as u32,
            signature: compact_node_signature(args.node, args.source),
            parent: args.parent,
            complexity,
        });
    }
}
