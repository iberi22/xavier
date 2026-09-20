# FEATURE: MCP Skill Dispatch Tools

**Status:** `planned` | **Score:** — | **Last Tested:** —

## Overview
Skill dispatch exists only as REST (`POST /api/skill/dispatch`, `GET /api/skill/list`, `GET /skills`). MCP consumers (38 tools today, none skill-related) cannot reach it. This feature registers `xavier_dispatch_skill {task*, max_tokens?, project?}` and `xavier_skill_list` in `get_xavier_context_tools` (`src/server/mcp/tools_context.rs:22`) with handler arms in `handle_context_tool` (line 103), mirroring the REST dispatcher wiring exactly — ranking logic lives in one place.

## Architecture & Design
- Tool descriptions short and functional (≤60 chars lesson from Hermes approver limits).
- Dispatch response reuses `DispatchResponse` / `MemoryRefResponse` shapes from `api/skills.rs`.
- Fail-open across the MCP boundary: errors serialize to `{ok:false}` or empty-skill verdicts, never panics.
- Existing tool names/schemas untouched.

## Implementation Paths
- `src/server/mcp/tools_context.rs` (2 registrations + 2 handler arms)
- `src/server/mcp/tests.rs` (round-trip tests)

## Sub-features
- **`xavier_dispatch_skill`:** task + budget + project filter → skill + pack.
- **`xavier_skill_list`:** count parity with REST `GET /skills` (same registry assertion).

## Test References
- `test_mcp_dispatch_tool_roundtrip`, `test_mcp_skill_list_announced` + live `tools/list` grep.

## Known Issues & Notes
- Depends on #301 + #302. Transport/auth layers untouched.
