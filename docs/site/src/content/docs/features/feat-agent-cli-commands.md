---
title: "Xavier Agent CLI Commands"
description: "CLI commands: xavier agent scan/index/push/pull/status/sync for managing OpenClaw agent memory"
---

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-06-25

## Overview
A comprehensive suite of command-line tools under the `xavier agent` namespace to scan, index, synchronize, and monitor OpenClaw agent memories. These commands allow server scripts and external automation pipelines to manage local memories directly.

## Architecture & Design
CLI commands are parsed using `clap` into the `AgentCommand` enum structure. The handlers coordinate the underlying scanning and indexing engines, providing clean standard outputs (supporting standard terminal logs as well as a pure `--json` output format for seamless programmatic integration).

## Implementation Paths
- `src/cli/commands/enums.rs` (clap enum subcommands definition)
- `src/cli/handlers/agent_cli.rs` (CLI request processor and command router)
- `src/cli/handlers/agent.rs` (underlying service orchestrator)
- `src/cli/server.rs` (mapping HTTP routes like `/xavier/agents/status`)

## Sub-features
- **Agent Subcommands:** `scan`, `index`, `push`, and `pull` operations.
- **Status & Sync Control:** Full-featured status reports and complete multi-step `sync` sequencing.
- **HTTP Agent Bridges:** Exposes endpoint interfaces to allow web panels or Tauri apps to query active agent configurations.
- **JSON Output Mode:** Structured output format for clean scripting integrations.

## Test References
- CLI parsing and subcommand dispatch unit tests.
- Status and Sync process sequencing tests.

## Known Issues & Notes
- Developed and verified under PRs #342, #345, and #346. Evaluated successfully under standard workspace checks.

### Functional Agent CLI Commands Example
Execute full CLI synchronization with remote Supabase or central servers:

```bash
# Perform full scan, index, and push sequence
xavier agent sync --full

# Output synchronization logs as JSON for integration
xavier agent sync --json
```
