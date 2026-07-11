mod parser;

use crate::parser::TypeScriptParser;
use codegraph_types::{PluginRequest, PluginResponse};
use std::io::{self, Read};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--version" {
        println!("codegraph-parse-typescript 0.1.0 (tree_sitter=true, lsp=false, framework_routes=false)");
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
            eprintln!("Failed to parse request: {}", e);
            return Err(e.into());
        }
    };

    let mut all_symbols = Vec::new();

    for file in request.files {
        let is_tsx = file.path.ends_with(".tsx") || file.path.ends_with(".jsx");
        let mut parser = TypeScriptParser::new(file.language, is_tsx)?;
        let symbols = parser.parse(&file.content, &file.path)?;
        all_symbols.extend(symbols);
    }

    let response = PluginResponse {
        symbols: all_symbols,
        edges: vec![],
    };

    println!("{}", serde_json::to_string(&response)?);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph_types::Language;

    #[test]
    fn test_parse_simple_ts() {
        let mut parser = TypeScriptParser::new(Language::TypeScript, false).unwrap();
        let source = "function hello() { console.log('hello'); }";
        let symbols = parser.parse(source, "test.ts").unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "hello");
        assert_eq!(
            symbols[0].signature,
            Some("function hello() { ... }".to_string())
        );
    }

    #[test]
    fn test_parse_class() {
        let mut parser = TypeScriptParser::new(Language::TypeScript, false).unwrap();
        let source = "class MyClass { myMethod() {} }";
        let symbols = parser.parse(source, "test.ts").unwrap();
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "MyClass");
        assert_eq!(symbols[1].name, "myMethod");
        assert_eq!(symbols[1].parent, Some("MyClass".to_string()));
    }

    #[test]
    fn test_parse_arrow_function() {
        let mut parser = TypeScriptParser::new(Language::TypeScript, false).unwrap();
        let source = "const myFunc = () => {};";
        let symbols = parser.parse(source, "test.ts").unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "myFunc");
    }

    #[test]
    fn test_parse_interface_and_type() {
        let mut parser = TypeScriptParser::new(Language::TypeScript, false).unwrap();
        let source = "interface User { id: number; } type Point = { x: number; y: number; };";
        let symbols = parser.parse(source, "test.ts").unwrap();
        assert_eq!(symbols.len(), 2);
        assert!(symbols.iter().any(|s| s.name == "User"));
        assert!(symbols.iter().any(|s| s.name == "Point"));
    }

    #[test]
    fn test_parse_enum() {
        let mut parser = TypeScriptParser::new(Language::TypeScript, false).unwrap();
        let source = "enum Color { Red, Green, Blue }";
        let symbols = parser.parse(source, "test.ts").unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Color");
    }
}
