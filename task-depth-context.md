#!/usr/bin/env python3
"""
Task for Claude Code: Add depth + max_chars + tree context navigation to memory_context tool.

Based on Jules' incomplete implementation in PR #219 — Jules defined 'depth' in schema but
never wired the actual logic.

Current state (commit 495e3ac):
- memory_context: only has query + limit params
- No depth-based tree navigation
- No max_chars limit enforced in handler

Target state:
- memory_context: query + limit + max_chars + depth + search_mode
- depth: 0 = flat (current behavior), 1 = direct relations, 2 = two-hop relations
- Depth explores the graph via parent_id/cluster_id/relation fields in MemoryDocument
- max_chars enforces a hard char limit on total returned content
- search_mode: bm25, semantic, or hybrid

Tools to MODIFY:
1. src/server/mcp/tools_memory.rs — update input_schema + handler logic for memory_context
2. src/server/mcp/types.rs — MCPContextResult already has the fields, verify
3. src/server/mcp/tests.rs — add tests for depth-based context retrieval

Tests needed:
- memory_context_depth_flat — depth=0 returns only query matches
- memory_context_depth_one — depth=1 includes direct parent/child relations
- memory_context_max_chars_truncation — verifies char limit works
- memory_context_depth_two — depth=2 explores two-hop relations

Crates.io deps needed: none (already have ulid, chrono, serde_json)

Build verification:
- cargo check --lib must pass
- cargo test --lib mcp must pass (all 22 existing + new depth tests)

REMEMBER: Write to temp dir first, test, then overwrite real files only after verification.

Read this file for full context: E:/cortex/xavier/task-depth-context.md
"""
