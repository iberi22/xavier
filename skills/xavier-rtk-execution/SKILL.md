---
name: xavier-rtk-execution
title: Xavier RTK Kernel Execution & Token Compression Protocol
description: Canonical guide for AI agents to run terminal commands (cargo, git, grep) via Xavier's rtk-kernel proxy to compress CLI outputs and save 60-90% LLM tokens.
tags:
  - xavier
  - rtk-kernel
  - token-savings
  - cli
  - execution
category: execution
---

# Xavier RTK Kernel Execution Protocol

> **PURPOSE:** Accelerate command execution and reduce context consumption by intercepting and filtering verbose CLI outputs (cargo tests, compiler errors, git status, ripgrep logs) using Xavier's native `rtk-kernel` proxy.

## 1. Verified Capabilities (`xavier plugin list`)
- **Plugin Name:** `rtk-kernel` (v0.1.0)
- **Token Compression:** 60% – 90% reduction on compiler outputs and verbose test runners.
- **Commands Supported:** `cargo`, `git`, `grep`, `pytest`, `npm / pnpm`.

## 2. Invocation Methods

### Direct Xavier CLI (`exec`)
```bash
# Execute cargo tests through RTK filter
xavier exec "cargo test --test kernel_proxy_test"

# Filter git status or diff
xavier exec "git status" -C .

# Grep through codebase with compressed output
xavier exec "grep -rn 'fn main' src"
```

### MCP Tool Usage (`rtk_exec`)
When connected to Xavier MCP server:
```json
{
  "tool": "rtk_exec",
  "arguments": {
    "command": "cargo test --workspace"
  }
}
```

## 3. Best Practices for Autonomous Agents
1. **Prefer `xavier exec` for large test suites:** Running `cargo test` directly consumes thousands of redundant tokens on passing lines; `rtk-kernel` compresses output to concise summaries.
2. **Monitor Savings:** Check `[xavier-proxy] tokens: raw ≈ X, filtered ≈ Y | saved: Z%` metrics to verify compression.
