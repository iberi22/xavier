# FEATURE: SRC Reference Documentation

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-06-16

## Overview
Source Code Reference (SRC) and metadata mapping per the GitCore Protocol v3 specification. This provides internal AI agents and new developers with a precise source directory map, configuration schemas, module details, and building/verification commands.

## Architecture & Design
The SRC feature consists of high-quality Markdown specs maintained directly in `.gitcore/`. It acts as an indexing guide for other LLM-based autonomous engineers (like Jules or Hermes) to locate module boundaries, configurations, and core traits without scanning the entire source tree from scratch.

## Implementation Paths
- `.gitcore/SRC.md` (comprehensive directory structure, key Rust traits, and module mappings)
- `.gitcore/SRC_CONFIG.md` (environment variables and structural config options)

## Sub-features
- **Directory Structure Documentation:** Map of all primary codebase paths (clients, parsers, memory, code-graph, tauri, etc.).
- **Core Modules Definition:** Detailed scopes for modules like `a2a` (agent-to-agent), `agents`, `memory`, `server`, and `tasks`.
- **CLI Reference:** Syntax guides for all `xavier` binary commands.
- **Real Config Reference Mapping:** Precise descriptions of env vars like `XAVIER_JWT_SECRET`, local LLM overrides, and mesh licensing parameters.

## Test References
- GitCore compliance GAP analysis script checking required file existence.

## Known Issues & Notes
- Content must be kept perfectly synchronized with codebase refactorings to avoid guiding agents to legacy or deleted source files.
