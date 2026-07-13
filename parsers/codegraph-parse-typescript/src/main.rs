mod parser;

use crate::parser::TypeScriptParser;
use codegraph_types::{PluginRequest, PluginResponse};
use std::io::{self, Read};
use std::time::Instant;
use std::fs;

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

    let start_time = Instant::now();
    let mut all_results = Vec::new();

    for file_path in request.files {
        let content = match fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to read file {}: {}", file_path, e);
                continue;
            }
        };

        let is_tsx = file_path.ends_with(".tsx") || file_path.ends_with(".jsx");
        let lang = if is_tsx { "typescriptreact" } else { "typescript" };

        let mut parser = TypeScriptParser::new(lang.to_string(), is_tsx)?;
        match parser.parse(&content, &file_path) {
            Ok(nodes) => all_results.extend(nodes),
            Err(e) => eprintln!("Failed to parse {}: {}", file_path, e),
        }
    }

    let response = PluginResponse {
        version: "1.0".to_string(),
        results: all_results,
        duration_ms: start_time.elapsed().as_millis() as u64,
    };

    println!("{}", serde_json::to_string(&response)?);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_ts() {
        let mut parser = TypeScriptParser::new("typescript".to_string(), false).unwrap();
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
        let mut parser = TypeScriptParser::new("typescript".to_string(), false).unwrap();
        let source = "class MyClass { myMethod() {} }";
        let symbols = parser.parse(source, "test.ts").unwrap();
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "MyClass");
        assert_eq!(symbols[1].name, "myMethod");
        assert!(symbols[1].parent_id.is_some());
        assert!(symbols[1].parent_id.as_ref().unwrap().contains("MyClass"));
    }

    #[test]
    fn test_parse_arrow_function() {
        let mut parser = TypeScriptParser::new("typescript".to_string(), false).unwrap();
        let source = "const myFunc = () => {};";
        let symbols = parser.parse(source, "test.ts").unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "myFunc");
    }

    #[test]
    fn test_parse_interface_and_type() {
        let mut parser = TypeScriptParser::new("typescript".to_string(), false).unwrap();
        let source = "interface User { id: number; } type Point = { x: number; y: number; };";
        let symbols = parser.parse(source, "test.ts").unwrap();
        assert_eq!(symbols.len(), 2);
        assert!(symbols.iter().any(|s| s.name == "User"));
        assert!(symbols.iter().any(|s| s.name == "Point"));
    }

    #[test]
    fn test_parse_enum() {
        let mut parser = TypeScriptParser::new("typescript".to_string(), false).unwrap();
        let source = "enum Color { Red, Green, Blue }";
        let symbols = parser.parse(source, "test.ts").unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Color");
    }
}
