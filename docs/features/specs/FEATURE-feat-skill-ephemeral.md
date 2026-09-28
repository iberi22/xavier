# FEATURE: Deterministic Ephemeral Skill Composer

**Status:** `planned`
**Ledger feature ID:** `feat-skill-ephemeral`

## Overview
Autonomous agent tasks frequently require task-scoped context that combines relevant skills, past decision memories, and code graph symbols without polluting canonical skill repositories or leaving orphaned filesystem artifacts. This feature introduces a deterministic ephemeral skill composer in `src/context/skill_controller/composer.rs`. Before agent spawn, it writes an atomic, task-scoped skill (`<worktree>/.agents/skills/eph-<task>/SKILL.md` and `.claude/skills` symlink) or delivers the equivalent context pack via MCP for live sessions, cleaning up upon task completion.

## Architecture & Design
- **Deterministic Assembly without LLM (D11):** Ephemeral skills are generated deterministically from existing skills, scoped memories, and symbol graphs without invoking an LLM. Total serialized composition is bounded by `ephemeral.max_tokens` (default 4000). The unused legacy decision budget (`src/context/skill_dispatcher.rs:107`) does not guarantee a total limit; this feature explicitly enforces the final serialized budget.
- **Authorized Workspace Scoping for Ordinary and Decision Memories (D11):** Memory and decision retrieval must strictly filter by the authorized workspace and worktree boundary. The current decision query filters only by kind (`src/context/skill_dispatcher.rs:207`); the new path enforces authorized workspace scope for **both** ordinary memories and decision memories. Caller-provided `project` strings are untrusted data and do not grant access.
- **Whole-Package Secret Gate (D10):** Every composed output is evaluated by the in-process `RegexSet` secret scanner prior to writing or delivery. Any detected credential pattern blocks publication and delivery; findings report location and classification only.
- **Provenance Stamping (D11):** Injects immutable provenance headers including source package hashes, memory IDs, graph revision, task ID, and lease expiry. Raw prompts are never used in skill descriptions.
- **Trust Boundaries & Safe Omission:** Untrusted evidence and memory fragments are quarantined in bounded quoted evidence with origin IDs (`src/context/skill_registry.rs:43-75`) and cannot inject executable frontmatter, tools, or shell commands. If a complete safe instruction section cannot fit within the token budget, it is omitted or returns `_none` rather than truncating away trust boundaries or safety clauses.
- **CLI-Only Mutation & Live MCP Delivery (D4):** Filesystem creation and cleanup of ephemeral skills are restricted to CLI commands (`xavier skills compose|release`). Live sessions receive the composition as an in-memory MCP context pack without creating filesystem artifacts.
- **Lifecycle Cleanup:** Post-execution hooks remove only journaled, unchanged `eph-*` directories and links. Active tasks renew leases to prevent premature garbage collection; stale leases require confirmed inactivity before removal.

## Implementation Paths
- `src/context/skill_controller/composer.rs`
- `src/context/skill_controller/mod.rs`
- `src/cli/commands/spawn.rs` (`src/cli/commands/spawn.rs:265-279`)
- `src/server/http/context.rs` (`src/server/http/context.rs:177-245`)
- `src/server/mcp/tools_context.rs` (`src/server/mcp/tools_context.rs:136-517`)

## Sub-features
- **`context_assembly`:** Deterministic composition combining eligible skills, scoped ordinary and decision memories, and CodeGraph symbols within `ephemeral.max_tokens`.
- **`secret_gate`:** Whole-package secret scanning blocking publication or delivery on any finding.
- **`worktree_projection`:** Pre-spawn generation of `<worktree>/.agents/skills/eph-<task>/SKILL.md` and per-skill tool links.
- **`live_mcp_pack`:** Context pack generation for live sessions without creating filesystem artifacts.
- **`lease_cleanup`:** Lifecycle hook removing journaled, unchanged ephemeral directories upon task termination.

## Test References
- `test_ephemeral_composes_within_token_budget`.
- `test_ephemeral_provenance_and_scope_filtering` (validates authorized workspace boundary on both ordinary memories and decision memories).
- `test_ephemeral_blocks_on_secret_scan_finding`.
- `test_ephemeral_safe_omission_preserves_trust_boundaries`.
- `test_ephemeral_cleanup_removes_only_eph_dirs`.
- `test_ephemeral_live_session_returns_pack_without_files`.

## Known Issues & Notes
- Depends on `feat-skill-registry-unify` and `feat-skill-projector`.
