//! Go parser using tree-sitter.

use crate::error::{GraphError, Result};
use crate::parser::{compact_node_signature, cyclomatic_complexity,  PushSymbolArgs};
use crate::types::{Language, Symbol, SymbolKind};
use tree_sitter::{Node, Parser};

pub struct GoParser {
    parser: Parser,
}

impl Default for GoParser {
    fn default() -> Self {
        Self::new().expect("failed to initialize Go parser")
    }
}

impl GoParser {
    pub fn new() -> Result<Self> {
        let mut parser = Parser::new();
        let language = tree_sitter_go::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| GraphError::TreeSitter(format!("failed to set Go language: {}", e)))?;
        Ok(Self { parser })
    }

    pub fn parse(&mut self, source: &str, file_path: &str) -> Result<Vec<Symbol>> {
        let tree = self
            .parser
            .parse(source.as_bytes(), None)
            .ok_or_else(|| GraphError::Parser("Failed to parse Go source".to_string()))?;
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
            "function_declaration" => {
                self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Function,
                    parent.clone(),
                );
            }
            "method_declaration" => {
                let receiver_parent = self.extract_receiver_type(node, source);
                self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Method,
                    receiver_parent.or(parent.clone()),
                );
            }
            "type_declaration" => {
                self.extract_type_specs(node, source, file_path, symbols);
            }
            "import_declaration" | "import_spec" => {
                self.push_import(node, source, file_path, symbols);
            }
            "const_declaration" => {
                self.extract_declarations(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Constant,
                    parent.clone(),
                );
            }
            "var_declaration" => {
                self.extract_declarations(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Variable,
                    parent.clone(),
                );
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract(child, source, file_path, symbols, parent.clone());
        }
    }

    fn extract_receiver_type(&self, node: Node, source: &str) -> Option<String> {
        let receiver = node.child_by_field_name("receiver")?;
        // El receiver puede ser (u User) o (u *User)
        // Buscamos el tipo dentro del receiver
        let mut cursor = receiver.walk();
        for child in receiver.children(&mut cursor) {
            if child.kind() == "parameter_declaration" {
                if let Some(type_node) = child.child_by_field_name("type") {
                    return self.find_type_identifier(type_node, source);
                }
            }
        }
        None
    }

    fn find_type_identifier(&self, node: Node, source: &str) -> Option<String> {
        if node.kind() == "type_identifier" {
            return node
                .utf8_text(source.as_bytes())
                .ok()
                .map(|s| s.to_string());
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(name) = self.find_type_identifier(child, source) {
                return Some(name);
            }
        }
        None
    }

    fn extract_declarations(
        &self,
        node: Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Symbol>,
        kind: SymbolKind,
        parent: Option<String>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "const_spec" | "var_spec" => {
                    let mut spec_cursor = child.walk();
                    for grandchild in child.children(&mut spec_cursor) {
                        if grandchild.kind() == "identifier" {
                            if let Ok(name) = grandchild.utf8_text(source.as_bytes()) {
                                self.push_symbol(
                                    symbols,
                                    PushSymbolArgs {
                                        node: grandchild,
                                        source,
                                        language: Language::Go,
                                        kind: kind.clone(),
                                        file_path,
                                        name: name.to_string(),
                                        depth: 0,
                                        parent: parent.clone(), metadata: None,
                                    },
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn extract_type_specs(
        &self,
        node: Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Symbol>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "type_spec" {
                continue;
            }
            if let Some(name_node) = child.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                    let kind = child
                        .child_by_field_name("type")
                        .map(|node| match node.kind() {
                            "struct_type" => SymbolKind::Struct,
                            "interface_type" => SymbolKind::Trait,
                            _ => SymbolKind::Symbol,
                        })
                        .unwrap_or(SymbolKind::Symbol);
                    self.push_symbol(
                        symbols,
                        PushSymbolArgs {
                            node: child,
                            source,
                            language: Language::Go,
                            kind,
                            file_path,
                            name: name.to_string(),
                            depth: 0,
                            parent: None, metadata: None,
                        },
                    );
                }
            }
        }
    }

    fn push_named(
        &self,
        node: Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Symbol>,
        kind: SymbolKind,
        parent: Option<String>,
    ) {
        if let Some(name_node) = node.child_by_field_name("name") {
            if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                self.push_symbol(
                    symbols,
                    PushSymbolArgs {
                        node,
                        source,
                        language: Language::Go,
                        kind,
                        file_path,
                        name: name.to_string(),
                        depth: 0,
                        parent,
                    },
                );
            }
        }
    }

    fn push_import(&self, node: Node, source: &str, file_path: &str, symbols: &mut Vec<Symbol>) {
        let raw = node.utf8_text(source.as_bytes()).unwrap_or_default();
        for part in raw.split('"').skip(1).step_by(2) {
            self.push_symbol(
                symbols,
                PushSymbolArgs {
                    node,
                    source,
                    language: Language::Go,
                    kind: SymbolKind::Import,
                    file_path,
                    name: part.to_string(),
                    depth: 0,
                    parent: None, metadata: None,
                },
            );
        }
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
            lang: Language::Go,
            file_path: args.file_path.to_string(),
            start_line: (start.row + 1) as u32,
            end_line: (end.row + 1) as u32,
            start_col: start.column as u32,
            end_col: end.column as u32,
            signature: compact_node_signature(args.node, args.source),
            parent: args.parent,
            complexity, metadata: None,
        });
    }
}
