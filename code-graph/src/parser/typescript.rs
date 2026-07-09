//! TypeScript and JavaScript parser using tree-sitter.

use crate::error::{GraphError, Result};
use crate::parser::{compact_node_signature, cyclomatic_complexity, PushSymbolArgs};
use crate::types::{Language, Symbol, SymbolKind};
use tree_sitter::{Node, Parser};

pub struct TypeScriptParser {
    parser: Parser,
    lang: Language,
}

impl Default for TypeScriptParser {
    fn default() -> Self {
        Self::new(Language::TypeScript).expect("failed to initialize TypeScript parser")
    }
}

impl TypeScriptParser {
    pub fn new(lang: Language) -> Result<Self> {
        let mut parser = Parser::new();
        let grammar = match lang {
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
            Language::JavaScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
            _ => tree_sitter_typescript::LANGUAGE_TSX,
        }
        .into();
        parser.set_language(&grammar).map_err(|e| {
            GraphError::TreeSitter(format!("failed to set TypeScript language: {}", e))
        })?;
        Ok(Self { parser, lang })
    }

    pub fn parse(&mut self, source: &str, file_path: &str) -> Result<Vec<Symbol>> {
        let grammar = if file_path.ends_with(".tsx") || file_path.ends_with(".jsx") {
            tree_sitter_typescript::LANGUAGE_TSX
        } else {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT
        };
        self.parser.set_language(&grammar.into()).map_err(|e| {
            GraphError::TreeSitter(format!("failed to set TypeScript/TSX language: {}", e))
        })?;

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
            "interface_declaration" | "type_alias_declaration" => {
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
                self.push_export(node, source, file_path, symbols);
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
        self.push_symbol(
            symbols,
            PushSymbolArgs {
                node,
                source,
                language: self.lang.clone(),
                kind,
                file_path,
                name: name.clone(),
                depth: 0,
                parent,
            },
        );
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
        let kind = if is_const {
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
                kind.clone()
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
                        PushSymbolArgs {
                            node: symbol_node,
                            source,
                            language: self.lang.clone(),
                            kind,
                            file_path,
                            name: name.to_string(),
                            depth: 0,
                            parent,
                        },
                    );
                }
            }
            "object_pattern" | "array_pattern" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "{"
                        || child.kind() == "}"
                        || child.kind() == "["
                        || child.kind() == "]"
                        || child.kind() == ","
                    {
                        continue;
                    }
                    // In patterns, we might have shorthand_property_identifier, or [identifier, identifier]
                    // We need to be careful with depth or recursion
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
                        PushSymbolArgs {
                            node: symbol_node,
                            source,
                            language: self.lang.clone(),
                            kind,
                            file_path,
                            name: name.to_string(),
                            depth: 0,
                            parent,
                        },
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
                // Other patterns like rest_pattern can be handled if needed
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
            PushSymbolArgs {
                node,
                source,
                language: self.lang.clone(),
                kind: SymbolKind::Import,
                file_path,
                name,
                depth: 0,
                parent: None,
            },
        );
    }

    fn push_export(&self, node: Node, source: &str, file_path: &str, symbols: &mut Vec<Symbol>) {
        let raw = node.utf8_text(source.as_bytes()).unwrap_or_default();
        let mut name = "default".to_string();

        if let Some(declaration) = node.child_by_field_name("declaration") {
            if let Some(name_node) = declaration.child_by_field_name("name") {
                name = name_node
                    .utf8_text(source.as_bytes())
                    .unwrap_or("?")
                    .to_string();
            } else if declaration.kind() == "lexical_declaration"
                || declaration.kind() == "variable_declaration"
            {
                // export const x = 1; -> lexical_declaration -> variable_declarator -> identifier
                let mut cursor = declaration.walk();
                for child in declaration.children(&mut cursor) {
                    if child.kind() == "variable_declarator" {
                        if let Some(name_node) = child.child_by_field_name("name") {
                            name = name_node
                                .utf8_text(source.as_bytes())
                                .unwrap_or("?")
                                .to_string();
                            break;
                        }
                    }
                }
            }
        } else {
            name = raw.lines().next().unwrap_or(raw).trim().to_string();
        };

        self.push_symbol(
            symbols,
            PushSymbolArgs {
                node,
                source,
                language: self.lang.clone(),
                kind: SymbolKind::Export,
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
            lang: self.lang.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SymbolKind;

    #[test]
    fn test_parse_typescript() {
        let mut parser = TypeScriptParser::new(Language::TypeScript).unwrap();
        let source = r#"
            import { foo } from './foo';
            export const bar = 1;
            export function baz() { return 42; }
            interface User { id: number; name: string; }
            type ID = string | number;
            enum Role { Admin, User }
            class UserService {
                private users: User[] = [];
                getUsers(): User[] { return this.users; }
            }
        "#;
        let symbols = parser.parse(source, "test.ts").unwrap();

        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Import));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Export && s.name == "bar"));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Export && s.name == "baz"));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Function && s.name == "baz"));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Struct && s.name == "User"));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Struct && s.name == "ID"));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Enum && s.name == "Role"));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Class && s.name == "UserService"));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Method && s.name == "getUsers"));
    }

    #[test]
    fn test_parse_tsx() {
        let mut parser = TypeScriptParser::new(Language::TypeScript).unwrap();
        let source = r#"
            import React from 'react';
            export const MyComponent = ({ name }: { name: string }) => {
                return <div>Hello {name}</div>;
            };
        "#;
        let symbols = parser.parse(source, "component.tsx").unwrap();

        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Import));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Function && s.name == "MyComponent"));
    }

    #[test]
    fn test_parse_javascript() {
        let mut parser = TypeScriptParser::new(Language::JavaScript).unwrap();
        let source = r#"
            const fs = require('fs');
            function log(msg) { console.log(msg); }
            class Logger {
                info(msg) { log(msg); }
            }
            module.exports = Logger;
        "#;
        let symbols = parser.parse(source, "logger.js").unwrap();

        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Function && s.name == "log"));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Class && s.name == "Logger"));
        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Method && s.name == "info"));
    }
}
