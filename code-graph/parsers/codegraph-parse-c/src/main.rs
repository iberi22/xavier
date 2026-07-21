use anyhow::{anyhow, Result};
use codegraph_types::{Language, PluginRequest, PluginResponse, Symbol, SymbolKind};
use std::io::{self, Read};
use tree_sitter::{Node, Parser};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--version" {
        println!("codegraph-parse-c 0.1.0");
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

    let mut parser = CParser::new()?;
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

pub struct CParser {
    parser: Parser,
}

impl CParser {
    pub fn new() -> Result<Self> {
        let mut parser = Parser::new();
        let language = tree_sitter_c::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| anyhow!("failed to set C language: {}", e))?;
        Ok(Self { parser })
    }

    pub fn parse(&mut self, source: &str, file_path: &str) -> Result<Vec<Symbol>> {
        let tree = self
            .parser
            .parse(source.as_bytes(), None)
            .ok_or_else(|| anyhow!("Failed to parse C source"))?;
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
                    self.push_symbol(
                        symbols,
                        node,
                        source,
                        file_path,
                        name,
                        SymbolKind::Function,
                        parent.clone(),
                    );
                }
            }
            "struct_specifier" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                        self.push_symbol(
                            symbols,
                            node,
                            source,
                            file_path,
                            name.to_string(),
                            SymbolKind::Struct,
                            parent.clone(),
                        );
                    }
                }
            }
            "enum_specifier" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                        self.push_symbol(
                            symbols,
                            node,
                            source,
                            file_path,
                            name.to_string(),
                            SymbolKind::Enum,
                            parent.clone(),
                        );
                    }
                }
            }
            "preproc_include" => {
                self.push_import(node, source, file_path, symbols);
            }
            "preproc_def" => {
                if let Some(name_node) = node.child(1) {
                    if name_node.kind() == "identifier" {
                        if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                            self.push_symbol(
                                symbols,
                                node,
                                source,
                                file_path,
                                name.to_string(),
                                SymbolKind::Constant,
                                parent.clone(),
                            );
                        }
                    }
                }
            }
            _ => {}
        }

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
        if node.kind() == "identifier" {
            return node
                .utf8_text(source.as_bytes())
                .ok()
                .map(|s| s.to_string());
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
            lang: Language::C,
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
