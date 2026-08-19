# code-graph

> **Codebase Understanding without RAG** - Tree-sitter + Agentic Search

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2024+-orange.svg)](https://rust-lang.org)
[![Tokio](https://img.shields.io/badge/Tokio-1.42+-blue.svg)](https://tokio.rs)

## ⚡ Why Not RAG?

Based on research from Aider, Claude Code, and Cline:

| Approach | Use Case | code-graph |
|----------|----------|------------|
| **RAG (Vector DB)** | Unstructured docs (knowledge bases) | ❌ Not needed |
| **Tree-sitter** | Code structure (AST, functions, classes) | ✅ Primary |
| **Agentic Search** | Navigation via filesystem | ✅ Fallback |
| **Symbol Index** | Fast lookup (CTags style) | ✅ Fast path |

## 📦 Installation

```bash
cargo install code-graph
```

## 🚀 Quick Start

```bash
# Scan a project
code-graph scan ./my-project

# Query functions/structs
code-graph find "function_name" --lang rust

# Agentic mode: ask questions
code-graph ask "How does auth work?"
```

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      CLI (main.rs)                          │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │  Scanner    │  │   Indexer   │  │   Query Engine     │ │
│  │  (walkdir)  │─▶│(tree-sitter)│─▶│  (hybrid search)   │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
│         │                │                    │             │
│         ▼                ▼                    ▼             │
│  ┌──────────────────────────────────────────────────────┐   │
│  │              SQLite (code_graph.db)                   │   │
│  │  - symbols    - imports    - exports    - refs       │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

## 📊 Features

- **Multi-language**: Rust, TypeScript, Python, Go, Java, C++
- **Tree-sitter AST**: Parse functions, classes, structs, imports
- **Fast Index**: SQLite-based symbol index (< 1s for 10k files)
- **Multi-Project Namespace Isolation**: Project-scoped deterministic IDs prevent symbol collisions across codebases in a single index
- **Agentic Fallback**: Navigate filesystem when needed
- **Zero Config**: Auto-detect language and structure

## 🌐 Multi-Project Namespace Isolation

`code-graph` supports indexing multiple projects into a unified database without symbol collisions.

```rust
use code-graph::types::Symbol;

// Structural symbol identity includes `project_id`
let symbol_id = symbol.deterministic_id("project_alpha");
```

- **Project Scoping**: Each symbol `stable_id` is derived from `v2|project_id|file_path|name|kind|parent|signature`.
- **Zero Collisions**: Two projects containing identical file paths (`src/lib.rs`) and symbol names (`initialize`) generate distinct structural hashes and coexist safely in SQLite.
- **Namespace Management**: Integrations use project ID parameters (`register_project` / `list_projects`) to organize and filter multi-project namespaces across cross-project queries.

## 📋 Commands

| Command | Description |
|---------|-------------|
| `scan <path>` | Index entire codebase |
| `find <query>` | Find symbols |
| `ask <question>` | Agentic Q&A |
| `refs <symbol>` | Find references |
| `graph <func>` | Show call graph |

## 🔧 Tech Stack

- **Runtime**: Tokio 1.42+
- **Parser**: tree-sitter 0.24+
- **Database**: SQLite (rusqlite)
- **CLI**: Clap 4
- **Async**: futures, tokio

## 📄 License

MIT License - See [LICENSE](LICENSE) file.

---

*Based on Aider's tree-sitter strategy + Claude Code's agentic search*
