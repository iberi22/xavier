# FEATURE: Deterministic Ephemeral Skill Composer

**Status:** `planned`

## Overview
Autonomous agent tasks frequently require task-scoped context that combines relevant skills, past decision memories, and code graph symbols without polluting canonical skill repositories or leaving orphaned filesystem artifacts. This feature introduces a deterministic ephemeral skill composer in `src/context/skill_controller/composer.rs`. Before agent spawn, it writes an atomic, task-scoped skill (`<worktree>/.agents/skills/eph-<task>/SKILL.md` and `.claude/skills` symlink) or delivers the equivalent context pack via MCP for live sessions, cleaning up upon task completion.

## Architecture & Design
- **Deterministic Assembly without LLM (D11):** Ephemeral skills are generated deterministically from existing skills, scoped memories, and symbol graphs without invoking an LLM. Content is bounded by `ephemeral.max_tokens` (default 4000).
- **Authorized Workspace Scoping (D11):** Memory and decision retrieval must strictly filter by authorized workspace scope (`src/context/skill_dispatcher.rs:207`); caller-provided `project` strings are untrusted data and do not grant access.
- **Provenance Stamping (D11):** Injects immutable provenance headers including source package hashes, memory IDs, graph revision, task ID, and lease expiry.
- **Trust Boundaries:** Untrusted evidence and memory fragments are quarantined using delimited blocks (`src/context/skill_registry.rs:43-75`) and cannot inject executable frontmatter or tool definitions.
- **Lifecycle Cleanup:** Post-execution hooks remove only journaled, unchanged `eph-*` directories. Active tasks renew leases to prevent premature garbage collection.

## Implementation Paths
- `src/context/skill_controller/composer.rs`
- `src/context/skill_controller/mod.rs`
- `src/cli/commands/spawn.rs` (`src/cli/commands/spawn.rs:265-279`)
- `src/server/http/context.rs` (`src/server/http/context.rs:177-245`)
- `src/server/mcp/tools_context.rs` (`src/server/mcp/tools_context.rs:136-517`)

## Sub-features
- **`context_assembly`:** Deterministic composition combining eligible skills, scoped memories, and CodeGraph symbols.
- **`worktree_projection`:** Pre-spawn generation of `<worktree>/.agents/skills/eph-<task>/SKILL.md` and tool links.
- **`live_mcp_pack`:** Context pack generation for live sessions without creating filesystem artifacts.
- **`lease_cleanup`:** Lifecycle hook removing journaled ephemeral directories upon task termination.

## Test References
- `test_ephemeral_composes_within_token_budget`.
- `test_ephemeral_provenance_and_scope_filtering` (validates workspace authorization on memories and decisions).
- `test_ephemeral_cleanup_removes_only_eph_dirs`.
- `test_ephemeral_live_session_returns_pack_without_files`.

## Known Issues & Notes
- Depends on `feat-skill-registry-unify` and `feat-skill-projector`.
