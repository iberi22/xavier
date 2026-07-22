use std::io::{self, Read};
use codegraph_types::{PluginRequest, PluginResponse, Symbol, Language, SymbolKind};
use tree_sitter::{Node, Parser};

pub struct GoParser {
    parser: Parser,
}

impl GoParser {
    pub fn new() -> anyhow::Result<Self> {
        let mut parser = Parser::new();
        let language = tree_sitter_go::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| anyhow::anyhow!("failed to set Go language: {}", e))?;
        Ok(Self { parser })
    }

    pub fn parse(&mut self, source: &str, file_path: &str) -> anyhow::Result<Vec<Symbol>> {
        let tree = self
            .parser
            .parse(source.as_bytes(), None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse Go source"))?;
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
                                    grandchild,
                                    source,
                                    file_path,
                                    name.to_string(),
                                    kind.clone(),
                                    parent.clone(),
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
                        child,
                        source,
                        file_path,
                        name.to_string(),
                        kind,
                        None,
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
                    node,
                    source,
                    file_path,
                    name.to_string(),
                    kind,
                    parent,
                );
            }
        }
    }

    fn push_import(&self, node: Node, source: &str, file_path: &str, symbols: &mut Vec<Symbol>) {
        let raw = node.utf8_text(source.as_bytes()).unwrap_or_default();
        for part in raw.split('"').skip(1).step_by(2) {
            self.push_symbol(
                symbols,
                node,
                source,
                file_path,
                part.to_string(),
                SymbolKind::Import,
                None,
            );
        }
    }

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
        let complexity = (kind == SymbolKind::Function || kind == SymbolKind::Method)
            .then(|| cyclomatic_complexity(node, source));
        symbols.push(Symbol {
            id: None,
            stable_id: None,
            name,
            kind,
            lang: Language::Go,
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

    let mut parser = GoParser::new()?;
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
