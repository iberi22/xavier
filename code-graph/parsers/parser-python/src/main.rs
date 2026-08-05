use codegraph_types::{Language, PluginRequest, PluginResponse, Symbol, SymbolKind};
use std::io::{self, Read};
use tree_sitter::{Node, Parser};

pub struct PythonParser {
    parser: Parser,
}

impl PythonParser {
    pub fn new() -> anyhow::Result<Self> {
        let mut parser = Parser::new();
        let language = tree_sitter_python::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| anyhow::anyhow!("failed to set Python language: {}", e))?;
        Ok(Self { parser })
    }

    pub fn parse(&mut self, source: &str, file_path: &str) -> anyhow::Result<Vec<Symbol>> {
        let tree = self
            .parser
            .parse(source.as_bytes(), None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse Python source"))?;
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
            "function_definition" | "async_function_definition" => {
                self.push_named(
                    node,
                    source,
                    file_path,
                    symbols,
                    SymbolKind::Function,
                    parent.clone(),
                );
            }
            "class_definition" => {
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
            "import_statement" | "import_from_statement" => {
                self.push_import(node, source, file_path, symbols);
            }
            "assignment" => {
                self.push_assignment(node, source, file_path, symbols, parent.clone());
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

    fn push_assignment(
        &self,
        node: Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Symbol>,
        parent: Option<String>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                if let Ok(name) = child.utf8_text(source.as_bytes()) {
                    self.push_symbol(
                        symbols,
                        child,
                        source,
                        file_path,
                        name.to_string(),
                        SymbolKind::Variable,
                        parent.clone(),
                    );
                }
            } else if child.kind() == "pattern_list" || child.kind() == "tuple" {
                self.extract_identifiers_from_pattern(child, source, file_path, symbols, &parent);
            } else if child.kind() == "=" {
                break;
            }
        }
    }

    fn extract_identifiers_from_pattern(
        &self,
        node: Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<Symbol>,
        parent: &Option<String>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                if let Ok(name) = child.utf8_text(source.as_bytes()) {
                    self.push_symbol(
                        symbols,
                        child,
                        source,
                        file_path,
                        name.to_string(),
                        SymbolKind::Variable,
                        parent.clone(),
                    );
                }
            } else if child.kind() == "pattern_list"
                || child.kind() == "tuple"
                || child.kind() == "list_pattern"
            {
                self.extract_identifiers_from_pattern(child, source, file_path, symbols, parent);
            }
        }
    }

    fn push_import(&self, node: Node, source: &str, file_path: &str, symbols: &mut Vec<Symbol>) {
        let raw = node.utf8_text(source.as_bytes()).unwrap_or_default();
        let name = raw
            .trim_start_matches("from ")
            .trim_start_matches("import ")
            .split_whitespace()
            .next()
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

        let final_kind = if kind == SymbolKind::Function && parent.is_some() {
            SymbolKind::Method
        } else {
            kind
        };

        let complexity = (final_kind == SymbolKind::Function || final_kind == SymbolKind::Method)
            .then(|| cyclomatic_complexity(node, source));
        symbols.push(Symbol {
            id: None,
            stable_id: None,
            name,
            kind: final_kind,
            lang: Language::Python,
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

    let mut parser = PythonParser::new()?;
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
