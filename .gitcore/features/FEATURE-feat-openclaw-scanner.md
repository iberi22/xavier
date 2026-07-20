# FEATURE: OpenClaw Agent Scanner

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-06-25

## Overview
Scans configured agent directories (e.g. `clawd/agents/`) for OpenClaw agent memory structures. It parses standard OpenClaw formats (including `MEMORY.md`, `SOUL.md`, `USER.md`, and daily activity logs), converting them into standardized memory records ready for semantic vector embedding.

## Architecture & Design
The scanner uses highly efficient asynchronous I/O via `tokio::fs` to recursively traverse agent workspace directories. When matching files are found, it maps them into an `AgentMemory` struct and delegates them to the indexer to generate vector embeddings.

## Implementation Paths
- `src/memory/openclaw_scanner.rs` (OpenClaw directory scanner, file parser, and token extractor)
- `src/memory/openclaw_indexer.rs` (OpenClaw memory vector encoder and database syncer)

## Sub-features
- **OpenClawAgentScanner Struct:** Native parser defining scanning scopes.
- **Recursive Directory Scan:** Traverses and matches files asynchronously.
- **Single Agent Focus Scan:** Allows scanning a specific agent's workspace by name.
- **Cognitive Document Extraction:** Directly parses and structured-imports MEMORY, SOUL, USER, and logs.
- **Supabase Cloud Push:** Built-in hooks to sync indexed agent vectors to remote cloud memory instances.

## Test References
- Asynchronous file matching and parse tests.
- Supabase integration and token-masking tests.

## Known Issues & Notes
- Closed under Issue #336. Fully merged and optimized under PR #342.
