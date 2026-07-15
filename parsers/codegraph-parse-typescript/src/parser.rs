use codegraph_types::{Node, Position, SymbolKind};
use tree_sitter::{Node as TSNode, Parser};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct TypeScriptParser {
    parser: Parser,
    lang: String,
}

impl TypeScriptParser {
    pub fn new(lang: String, is_tsx: bool) -> anyhow::Result<Self> {
        let mut parser = Parser::new();
        let grammar = if is_tsx {
            tree_sitter_typescript::LANGUAGE_TSX
        } else {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT
        };
        parser.set_language(&grammar.into())?;
        Ok(Self { parser, lang })
    }

    pub fn parse(&mut self, source: &str, file_path: &str) -> anyhow::Result<Vec<Node>> {
        let tree = self
            .parser
            .parse(source.as_bytes(), None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse source"))?;
        let mut symbols = Vec::new();
        self.extract(tree.root_node(), source, file_path, &mut symbols, None);
        Ok(symbols)
    }

    fn extract(
        &self,
        node: TSNode,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Node>,
        parent_id: Option<String>,
    ) {
        match node.kind() {
            "function_declaration" | "generator_function_declaration" => {
                self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Function,
                    parent_id.clone(),
                );
            }
            "enum_declaration" => {
                self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Enum,
                    parent_id.clone(),
                );
            }
            "method_definition" | "public_field_definition" => {
                self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Method,
                    parent_id.clone(),
                );
            }
            "class_declaration" => {
                let class_id = self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Class,
                    parent_id.clone(),
                );
                self.extract_children(node, source, file_path, symbols, class_id.or(parent_id));
                return;
            }
            "interface_declaration" => {
                self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Interface,
                    parent_id.clone(),
                );
            }
            "type_alias_declaration" => {
                self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Struct,
                    parent_id.clone(),
                );
            }
            "lexical_declaration" | "variable_declaration" => {
                self.extract_variable_functions(node, source, file_path, symbols, parent_id.clone());
            }
            "export_statement" => {
                // For export statements, we just extract their children
                self.extract_children(node, source, file_path, symbols, parent_id);
                return;
            }
            _ => {}
        }

        self.extract_children(node, source, file_path, symbols, parent_id);
    }

    fn extract_children(
        &self,
        node: TSNode,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Node>,
        parent_id: Option<String>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract(child, source, file_path, symbols, parent_id.clone());
        }
    }

    fn push_named(
        &self,
        node: TSNode,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Node>,
        kind: SymbolKind,
        parent_id: Option<String>,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source.as_bytes()).ok()?.to_string();
        Some(self.push_symbol(symbols, node, source, file_path, name, kind, parent_id))
    }

    fn extract_variable_functions(
        &self,
        node: TSNode,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Node>,
        parent_id: Option<String>,
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
                    parent_id.clone(),
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn extract_identifiers_from_pattern(
        &self,
        node: TSNode,
        symbol_node: TSNode,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Node>,
        kind: SymbolKind,
        parent_id: Option<String>,
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
                        parent_id,
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
                        parent_id.clone(),
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
                        parent_id,
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
                        parent_id,
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
                            parent_id.clone(),
                        );
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_symbol(
        &self,
        symbols: &mut Vec<Node>,
        node: TSNode,
        source: &str,
        file_path: &str,
        name: String,
        kind: SymbolKind,
        parent_id: Option<String>,
    ) -> String {
        let start = node.start_position();
        let end = node.end_position();
        let complexity = matches!(kind, SymbolKind::Function | SymbolKind::Method)
            .then(|| cyclomatic_complexity(node, source));

        let position = Position {
            start_line: (start.row + 1) as u32,
            start_col: start.column as u32,
            end_line: (end.row + 1) as u32,
            end_col: end.column as u32,
        };

        let id = format!("{}:{}:{}:{}", file_path, name, format!("{:?}", kind), start.row);

        let mut modifiers = json!({});
        if let Some(c) = complexity {
            modifiers["complexity"] = json!(c);
        }

        let updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        symbols.push(Node {
            id: id.clone(),
            kind,
            name,
            qual_name: None, // Could be improved
            file_path: file_path.to_string(),
            language: self.lang.clone(),
            position,
            signature: compact_node_signature(node, source),
            docstring: None,
            visibility: None,
            modifiers,
            parent_id,
            updated_at,
        });

        id
    }
}

fn compact_node_signature(node: TSNode, source: &str) -> Option<String> {
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

fn cyclomatic_complexity(node: TSNode, source: &str) -> f32 {
    fn count(node: TSNode, _source: &str) -> usize {
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
