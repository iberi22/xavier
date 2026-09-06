---
name: xavier-code-graph-analysis
title: Xavier CodeGraph Analysis & AST Navigation Protocol
description: Canonical guide for AI agents to query symbols, AST structures, call hierarchies, and blast-radius impact analysis using Xavier's embedded code-graph engine.
tags:
  - xavier
  - codegraph
  - ast
  - blast-radius
  - symbols
category: research
---

# Xavier CodeGraph Analysis Protocol

> **PURPOSE:** Enable deep semantic exploration of the codebase, structural relationship mapping, and blast-radius calculation before and during refactors without blindly grepping raw text.

## 1. Core Architecture
- **Engines:** `code-graph` (parser + symbol indexer) and `codegraph-types`.
- **Database:** Local SQLite store `data/code_graph.db` (lazy loaded, cached).
- **Supported Languages:** Rust (tree-sitter-rust), TypeScript/JavaScript (swc/tree-sitter), Python, Ruby.

## 2. Key Operations & MCP Tools

| Operation | CLI Command | MCP Tool | Purpose |
| :--- | :--- | :--- | :--- |
| **Inspect Graph** | `xavier nav graph` | `get_code_graph` | Retrieve active symbol index & high-level architecture. |
| **Symbol Search** | `xavier nav symbols <query>` | `codegraph_explore` | Find definitions, structs, enums, functions by pattern. |
| **Call Tracing** | `xavier nav trace <symbol>` | `trace_path` | Trace inbound/outbound call paths and dependencies. |
| **Blast Radius** | `xavier nav impact <file>` | `codegraph_explore` | Calculate affected modules when modifying a file. |

## 3. Workflow for Agents
1. **Pre-Refactor Check:** Run `codegraph_explore` or `trace_path` on the target function to identify callers across crates.
2. **Blast Radius Isolation:** Confirm how changes to `src/` impact `code-graph/` or `xavier-core/`.
3. **Symbol Integrity:** Re-index on demand if AST has undergone major structural edits.
