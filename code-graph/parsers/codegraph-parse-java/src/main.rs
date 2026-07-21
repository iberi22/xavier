use anyhow::{anyhow, Result};
use codegraph_types::{Language, PluginRequest, PluginResponse, Symbol, SymbolKind};
use std::io::{self, Read};
use tree_sitter::{Node, Parser};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--version" {
        println!("codegraph-parse-java 0.1.0");
        return Ok(());
    }

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

    let mut parser = JavaParser::new()?;
    let mut all_symbols = Vec::new();

    for file in request.files {
        match parser.parse(&file.source, &file.path) {
            Ok(symbols) => all_symbols.extend(symbols),
            Err(e) => {
                let response = PluginResponse {
                    symbols: vec![],
                    error: Some(format!("Failed to parse file {}: {}", file.path, e)),
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

pub struct JavaParser {
    parser: Parser,
}

impl JavaParser {
    pub fn new() -> Result<Self> {
        let mut parser = Parser::new();
        let language = tree_sitter_java::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| anyhow!("failed to set Java language: {}", e))?;
        Ok(Self { parser })
    }

    pub fn parse(&mut self, source: &str, file_path: &str) -> Result<Vec<Symbol>> {
        let tree = self
            .parser
            .parse(source.as_bytes(), None)
            .ok_or_else(|| anyhow!("Failed to parse Java source"))?;
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
                let name = self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Trait,
                    parent.clone(),
                );
                self.extract_children(node, source, file_path, symbols, name.or(parent));
                return;
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
            "method_declaration" | "constructor_declaration" => {
                self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Method,
                    parent.clone(),
                );
            }
            "import_declaration" => {
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
            node,
            source,
            file_path,
            name.clone(),
            kind,
            parent,
        );
        Some(name)
    }

    fn push_import(&self, node: Node, source: &str, file_path: &str, symbols: &mut Vec<Symbol>) {
        let raw = node.utf8_text(source.as_bytes()).unwrap_or_default();
        let name = raw
            .trim_start_matches("import ")
            .trim_end_matches(';')
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
            lang: Language::Java,
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

fn compact_node_signature(node: Node, source: &str) -> Option<String> {
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

fn cyclomatic_complexity(node: Node, source: &str) -> f32 {
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
