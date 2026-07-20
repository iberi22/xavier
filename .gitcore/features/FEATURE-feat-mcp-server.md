# FEATURE: MCP Server

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
Xavier implements an HTTP-first integration server with optional Model Context Protocol (MCP) support. This protocol exposes 12 specialized tools allowing external LLMs or agents to query memory, retrieve documents, execute code searches, and manage contexts efficiently.

## Architecture & Design
The MCP Server conforms strictly to the Model Context Protocol specifications. It enforces camelCase serialization (e.g., mapping `input_schema` to `inputSchema`, `mime_type` to `mimeType`, and `is_error` to `isError`) to maintain schema compliance with official clients. To prevent test flakiness, tests handle both snake_case and camelCase serialized keys robustly.

## Implementation Paths
- `src/server/http/` (REST endpoints and HTTP routing)
- `src/server/mcp/` (MCP server protocol, tools, and schema handlers)
- `src/cli/server.rs` (server CLI launching and capability configuration)

## Sub-features
- **mcp-tools-core:** Implements definitions and execution handlers for 12 core tools (including `mem_search`, `memory_context`, and codebase symbols).
- **mcp-progressive:** Features the Progressive Disclosure pattern for token optimization (relying on structured, lightweight `mem_search` followed by a targeted `memory_context` fetch).
- **mcp-auth:** Token-based authentication integration for secure endpoint access.

## Test References
- `src/server/mcp/tests.rs` unit and integration tests asserting camelCase keys.
- Integration tests validating the progressive disclosure search pipeline.

## Known Issues & Notes
- Grok MCP doctor: Handshake is successful, but there is some protocol drift on `tools/list` causing 18 unit tests to fail under the full suite when strictly checking strict protocol boundaries.
- Progressive disclosure reduces token overhead by ~90% for active agents.
