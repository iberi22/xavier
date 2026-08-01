use codegraph_types::{Language, PluginRequest, PluginResponse, Symbol, SymbolKind};
use std::io::{self, Read};
use tree_sitter::{Node, Parser, Tree};

pub struct RustParser {
    parser: Parser,
}

impl RustParser {
    pub fn new() -> anyhow::Result<Self> {
        let mut parser = Parser::new();
        let lang = tree_sitter_rust::LANGUAGE.into();
        parser
            .set_language(&lang)
            .map_err(|e| anyhow::anyhow!("failed to set Rust language: {}", e))?;
        Ok(Self { parser })
    }

    pub fn parse(&mut self, source: &str, file_path: &str) -> anyhow::Result<Vec<Symbol>> {
        let source_bytes = source.as_bytes();
        let tree = self
            .parser
            .parse(source_bytes, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse Rust source"))?;

        let mut symbols = Vec::new();
        self.extract_symbols(&tree, source, file_path, &mut symbols);
        Ok(symbols)
    }

    fn extract_symbols(
        &mut self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Symbol>,
    ) {
        self.extract_symbols_from_node(tree.root_node(), source, file_path, symbols, None);
    }

    fn extract_symbols_from_node(
        &mut self,
        node: Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Symbol>,
        parent: Option<String>,
    ) {
        let kind = node.kind();

        match kind {
            "function_item" | "function_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let kind = if parent.is_some() {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    };
                    self.push_symbol(
                        symbols,
                        node,
                        name_node,
                        source,
                        file_path,
                        kind,
                        parent.clone(),
                    );
                }
            }
            "struct_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    self.push_symbol(
                        symbols,
                        node,
                        name_node,
                        source,
                        file_path,
                        SymbolKind::Struct,
                        parent.clone(),
                    );
                }
            }
            "enum_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    self.push_symbol(
                        symbols,
                        node,
                        name_node,
                        source,
                        file_path,
                        SymbolKind::Enum,
                        parent.clone(),
                    );
                }
            }
            "trait_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    self.push_symbol(
                        symbols,
                        node,
                        name_node,
                        source,
                        file_path,
                        SymbolKind::Trait,
                        parent.clone(),
                    );
                }
            }
            "impl_item" => {
                let impl_name = self.extract_impl_name(node, source);
                self.push_symbol(
                    symbols,
                    node,
                    node, // Use the whole node as name reference if needed, but we have a custom name
                    source,
                    file_path,
                    SymbolKind::Impl,
                    parent.clone(),
                );
                // Update last symbol name because push_symbol uses name_node text
                if let Some(last) = symbols.last_mut() {
                    if last.kind == SymbolKind::Impl {
                        if let Some(name) = &impl_name {
                            last.name = name.clone();
                        }
                    }
                }

                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "declaration_list" {
                        let mut sub_cursor = child.walk();
                        for grandchild in child.children(&mut sub_cursor) {
                            self.extract_symbols_from_node(
                                grandchild,
                                source,
                                file_path,
                                symbols,
                                impl_name.clone().or(parent.clone()),
                            );
                        }
                    }
                }
                return;
            }
            "const_item" | "static_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    self.push_symbol(
                        symbols,
                        node,
                        name_node,
                        source,
                        file_path,
                        SymbolKind::Constant,
                        parent.clone(),
                    );
                }
            }
            "let_declaration" => {
                if let Some(pattern) = node.child_by_field_name("pattern") {
                    self.extract_identifiers_from_pattern(
                        pattern,
                        node,
                        source,
                        file_path,
                        symbols,
                        SymbolKind::Variable,
                        parent.clone(),
                    );
                }
            }
            "mod_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    self.push_symbol(
                        symbols,
                        node,
                        name_node,
                        source,
                        file_path,
                        SymbolKind::Module,
                        parent.clone(),
                    );
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_symbols_from_node(child, source, file_path, symbols, parent.clone());
        }
    }

    fn extract_impl_name(&self, node: Node, source: &str) -> Option<String> {
        // impl Trait for Struct { ... } -> Struct
        // impl Struct { ... } -> Struct
        if let Some(type_node) = node.child_by_field_name("type") {
            return type_node
                .utf8_text(source.as_bytes())
                .ok()
                .map(|s| s.to_string());
        }
        None
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
                if node.utf8_text(source.as_bytes()).is_ok() {
                    self.push_symbol(symbols, symbol_node, node, source, file_path, kind, parent);
                }
            }
            "tuple_pattern" | "struct_pattern" | "slice_pattern" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
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

    #[allow(clippy::too_many_arguments)]
    fn push_symbol(
        &self,
        symbols: &mut Vec<Symbol>,
        node: Node,
        name_node: Node,
        source: &str,
        file_path: &str,
        kind: SymbolKind,
        parent: Option<String>,
    ) {
        let start = node.start_position();
        let end = node.end_position();
        let complexity = (kind == SymbolKind::Function || kind == SymbolKind::Method)
            .then(|| cyclomatic_complexity(node, source));

        symbols.push(Symbol {
            id: None,
            stable_id: None,
            name: name_node
                .utf8_text(source.as_bytes())
                .unwrap_or("?")
                .to_string(),
            kind,
            lang: Language::Rust,
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

fn main() -> anyhow::Result<()> {
    let mut buffer = String::new();
    io::stdin().read_to_string(&mut buffer)?;

    if buffer.trim().is_empty() {
        return Ok(());
    }

    let request: PluginRequest = match serde_json::from_str(&buffer) {
        Ok(req) => req,
        Err(e) => {
            let response = PluginResponse {
                symbols: vec![],
                error: Some(format!("Failed to parse request: {}", e)),
            };
            println!("{}", serde_json::to_string(&response)?);
            return Ok(());
        }
    };

    let mut parser = RustParser::new()?;
    let mut all_symbols = Vec::new();

    for file in request.files {
        match parser.parse(&file.source, &file.path) {
            Ok(syms) => {
                all_symbols.extend(syms);
            }
            Err(e) => {
                let response = PluginResponse {
                    symbols: vec![],
                    error: Some(format!("Failed to parse: {}", e)),
                };
                println!("{}", serde_json::to_string(&response)?);
                return Ok(());
            }
        }
    }

    let response = PluginResponse {
        symbols: all_symbols,
        error: None,
    };
    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}
