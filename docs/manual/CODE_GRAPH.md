# AST Code Graph & Semantic Navigation

The Code Graph module parses multi-language codebases using Tree-sitter, constructing an in-memory or SQLite-backed Abstract Syntax Tree (AST) graph of symbols, references, definitions, and call hierarchies.

---

## 1. Features

- **Multi-Language Tree-sitter Parsing**: Native support for Rust, TypeScript, JavaScript, Python, Go, and C/C++.
- **Zero-Token Symbol Lookups**: Query function signatures, struct definitions, and trait implementations without sending code blocks to external LLMs.
- **Cross-File Reference Resolution**: Traverse callers, callees, and import graphs with deterministic hops.

---

## 2. CLI Usage

```bash
# Index current codebase into Code Graph
xavier codegraph index --path .

# Query symbol definition
xavier codegraph find --symbol "XavierSettings"

# Find all callers of a function
xavier codegraph callers --symbol "resolve_config_path"
```
