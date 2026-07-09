//! TypeScript parser using tree-sitter.

use crate::error::{GraphError, Result};
use crate::parser::{compact_node_signature, cyclomatic_complexity, PushSymbolArgs};
use crate::types::{Language, Symbol, SymbolKind};
use tree_sitter::{Node, Parser};

pub struct TypeScriptParser {
    parser: Parser,
    lang: Language,
}

impl TypeScriptParser {
    pub fn new(lang: Language) -> Result<Self> {
        let mut parser = Parser::new();
        let is_tsx = lang == Language::TypeScript && true; // Simple heuristic for now, or just use TSX as it usually works for TS too
        // Actually, let's use the extension if we had it, but we only have Language here.
        // tree-sitter-typescript provides both.
        let grammar = tree_sitter_typescript::LANGUAGE_TYPESCRIPT;
        parser
            .set_language(&grammar.into())
            .map_err(|e| GraphError::TreeSitter(format!("failed to set TypeScript language: {}", e)))?;
        Ok(Self { parser, lang })
    }

    pub fn parse(&mut self, source: &str, file_path: &str) -> Result<Vec<Symbol>> {
        let tree = self
            .parser
            .parse(source.as_bytes(), None)
            .ok_or_else(|| GraphError::Parser("Failed to parse TypeScript source".to_string()))?;
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
            "function_declaration" | "generator_function_declaration" => {
                self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Function,
                    parent.clone(),
                );
            }
            "enum_declaration" => {
                self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Enum,
                    parent.clone(),
                );
            }
            "method_definition" | "public_field_definition" => {
                self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Method,
                    parent.clone(),
                );
            }
            "class_declaration" => {
                let class_name = self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Class,
                    parent.clone(),
                );
                self.extract_children(node, source, file_path, symbols, class_name.or(parent));
                return;
            }
            "interface_declaration" => {
                self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Trait,
                    parent.clone(),
                );
            }
            "type_alias_declaration" => {
                self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Struct,
                    parent.clone(),
                );
            }
            "lexical_declaration" | "variable_declaration" => {
                self.extract_variable_functions(node, source, file_path, symbols, parent.clone());
            }
            "import_statement" => {
                self.push_import(node, source, file_path, symbols);
            }
            "export_statement" => {
                self.push_export(node, source, file_path, symbols, parent.clone());
                return;
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

    fn push_named(
        &self,
        node: Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Symbol>,
        kind: SymbolKind,
        parent: Option<String>,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source.as_bytes()).ok()?.to_string();
        self.push_symbol(symbols, node, source, file_path, name.clone(), kind, parent);
        Some(name)
    }

    fn extract_variable_functions(
        &self,
        node: Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Symbol>,
        parent: Option<String>,
    ) {
        let is_const = node.child(0).is_some_and(|c| c.kind() == "const");
        let base_kind = if is_const {
            SymbolKind::Constant
        } else {
            SymbolKind::Variable
        };

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "variable_declarator" {
                continue;
            }

            let value_kind = child
                .child_by_field_name("value")
                .map(|value| value.kind().to_string())
                .unwrap_or_default();

            let final_kind = if value_kind == "arrow_function" || value_kind == "function" {
                SymbolKind::Function
            } else {
                base_kind.clone()
            };

            if let Some(name_node) = child.child_by_field_name("name") {
                self.extract_identifiers_from_pattern(
                    name_node,
                    child,
                    source,
                    file_path,
                    symbols,
                    final_kind,
                    parent.clone(),
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn extract_identifiers_from_pattern(
        &self,
        node: Node,
        symbol_node: Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Symbol>,
        kind: SymbolKind,
        parent: Option<String>,
    ) {
        match node.kind() {
            "identifier" => {
                if let Ok(name) = node.utf8_text(source.as_bytes()) {
                    self.push_symbol(
                        symbols,
                        symbol_node,
                        source,
                        file_path,
                        name.to_string(),
                        kind,
                        parent,
                    );
                }
            }
            "object_pattern" | "array_pattern" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if matches!(child.kind(), "{" | "}" | "[" | "]" | "," | ":") {
                        continue;
                    }
                    self.extract_identifiers_from_pattern(
                        child,
                        symbol_node,
                        source,
                        file_path,
                        symbols,
                        kind.clone(),
                        parent.clone(),
                    );
                }
            }
            "shorthand_property_identifier" => {
                if let Ok(name) = node.utf8_text(source.as_bytes()) {
                    self.push_symbol(
                        symbols,
                        symbol_node,
                        source,
                        file_path,
                        name.to_string(),
                        kind,
                        parent,
                    );
                }
            }
            "pair" => {
                if let Some(value_node) = node.child_by_field_name("value") {
                    self.extract_identifiers_from_pattern(
                        value_node,
                        symbol_node,
                        source,
                        file_path,
                        symbols,
                        kind,
                        parent,
                    );
                }
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "identifier" || child.kind().contains("pattern") {
                        self.extract_identifiers_from_pattern(
                            child,
                            symbol_node,
                            source,
                            file_path,
                            symbols,
                            kind.clone(),
                            parent.clone(),
                        );
                    }
                }
            }
        }
    }

    fn push_import(&self, node: Node, source: &str, file_path: &str, symbols: &mut Vec<Symbol>) {
        let raw = node.utf8_text(source.as_bytes()).unwrap_or_default();
        let name = raw
            .split(['"', '\''])
            .nth(1)
            .unwrap_or(raw)
            .trim()
            .to_string();
        self.push_symbol(
            symbols,
            node,
            source,
            file_path,
            name,
            SymbolKind::Import,
            None,
        );
    }

    fn push_export(
        &self,
        node: Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Symbol>,
        parent: Option<String>,
    ) {
        let raw = node.utf8_text(source.as_bytes()).unwrap_or_default();
        let first_line = raw.lines().next().unwrap_or("export").to_string();

        self.push_symbol(
            symbols,
            node,
            source,
            file_path,
            first_line,
            SymbolKind::Export,
            parent.clone(),
        );

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let k = child.kind();
            if matches!(k, "export" | "default" | "*" | "{" | "}" | "," | "from" | ";") {
                continue;
            }
            self.extract(child, source, file_path, symbols, parent.clone());
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_symbol(
        &self,
        symbols: &mut Vec<Symbol>,
        node: Node,
        source: &str,
        file_path: &str,
        name: String,
        kind: SymbolKind,
        parent: Option<String>,
    ) {
        let start = node.start_position();
        let end = node.end_position();
        let complexity = matches!(kind, SymbolKind::Function | SymbolKind::Method)
            .then(|| cyclomatic_complexity(node, source));

        symbols.push(Symbol {
            id: None,
            stable_id: None,
            name,
            kind,
            lang: self.lang.clone(),
            file_path: file_path.to_string(),
            start_line: (start.row + 1) as u32,
            end_line: (end.row + 1) as u32,
            start_col: start.column as u32,
            end_col: end.column as u32,
            signature: compact_node_signature(node, source),
            parent,
            complexity,
        });
    }
}
