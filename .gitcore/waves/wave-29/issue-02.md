# [WAVE-29.02] feat-mcp-health-honesty — sys_health fabricates zeros and gods() ranks constructors

> Wave 29 — Observability honesty. Labels: `wave-29`, `ola29` (NO `jules` yet)
> Merge order: 2/10 | Risk: MED | Effort: Medium 2-4h

---

## Current State (MEDIBLE)

File: `src/server/mcp/tools_core.rs` (1084 lines)

**Defect 1 — `health_check` maps a field that is false while the real service is healthy.**
Line 645-652:
```rust
let result = MCPHealthResult {
    status: health.status.clone(),
    tools_count,
    handshake_ok: true,
    memory_store_ok: health.database.size_mb >= 0.0, // store exists
    embedding_ok: health.embedding.connected,
    mcp_protocol: "2026-07-28".to_string(),
};
```
Measured: this tool returned `{"embeddingOk": false, "status": "degraded", "handshakeOk": true,
"memoryStoreOk": true, "toolsCount": 42}` while `GET /health` on the same process returned
`embedding: {status: "healthy", available: true, model: "nomic-embed-text",
provider: "openai-compatible"}` with cache `hit_rate 0.9969` (59257 hits / 184 misses).
Note also `memory_store_ok: health.database.size_mb >= 0.0` — that expression is **always true**
for any non-negative f64 and therefore carries zero information.

**Defect 2 — `sys_health` invents zeros (lines 673-686):**
```rust
let benchmark = crate::auto_improvement::benchmark::BenchmarkSnapshot {
    timestamp_secs: ..., recall_at_k: 0.0, precision: 0.0,
    avg_latency_ms: 0.0, p99_latency_ms: 0.0,
    memory_hit_rate: in_process_health.database.size_mb / 1024.0,
    cache_hit_rate: 0.0,
    mesh_peers_reachable: in_process_health.mesh.connected_peers,
    health_status: in_process_health.status.clone(),
    db_integrity_ok: db_integrity, total_documents: 0, test_iterations: 0,
};
```
`recall_at_k`, `precision`, `avg_latency_ms`, `p99_latency_ms`, `cache_hit_rate`,
`total_documents` and `test_iterations` are hardcoded `0.0` / `0`. They flow into
`analyze_gaps` (line 687) and surface in the `active_gaps` array as if measured. Measured output
showed `embedding_coverage.percent: 100.0` with `indexed: 0, total: 0` and `uptime_secs: 20`
on a process with 10h45m uptime — a stub that looks like real data.
`checks` and `dependency_graph` come from `in_process_health` and were always `[]`.

**Defect 3 — `codegraph_gods` (line 1020) returns 0 nodes.**
Measured: `codegraph_gods` → `{"god_nodes":[],"returned":0}`.
Meanwhile `xavier_get_code_graph` on the same DB returned 10 "hubs", and **all 10 were named
`new`** (Rust constructors) with `incoming: 736, outgoing: 226`. The centrality ranking rewards
generic constructors; `gods` then filters them as builtins and returns nothing. Both tools are
useless on this corpus: 14486 symbols indexed, 0 usable hubs.

Tests in `src/server/mcp/tests.rs` that already cover these tools (do not break):
`health_check_method_and_tool` (1376), tool list assertions at 1076 (`codegraph_gods`),
2138-2247 (`codegraph_gods` dispatch + `xavier_dispatch_skill` presence).

## Desired State (DELTA)

- **Lines 645-652**:
  - `embedding_ok` must reflect the embedder's real availability. If `health.embedding` has both
    a `connected` flag and a status/`available` notion, reconcile them: treat the embedder as OK
    when the probe succeeded OR a documented fallback is active (there is already a
    `fallback_success` concept in the health module — use it rather than ignoring it).
  - Replace `memory_store_ok: health.database.size_mb >= 0.0` with a real signal
    (e.g. a check on the `sqlite_integrity` check status, which is already computed nearby at
    lines 669-671 as `db_integrity`).
  - Keep every field name and the `#[serde(rename = ...)]` contract in
    `src/server/mcp/types.rs` (MCPHealthResult, lines 176-189) UNCHANGED. This issue must not
    require editing `types.rs`.
- **Lines 673-686**: stop hardcoding measurement-shaped zeros. Either
  (a) compute the values from real sources where a cheap source exists, or
  (b) make the unmeasured ones explicitly unknown — `Option<f64>` / `None` / a distinct
    `"unmeasured"` sentinel — so `analyze_gaps` cannot treat 0.0 as a measured bad value.
  Add a short comment stating which fields are measured vs unmeasured.
  Do NOT invent a benchmark implementation in this issue; that is out of scope.
- **Line 1020 (`codegraph_gods`)**: exclude generic constructor names (`new`, `default`,
  `from`, `with_capacity`, `clone`) and trivial accessors from the centrality ranking, and/or
  deduplicate by name so 10 copies of `new` cannot occupy every hub slot. Ensure a non-empty,
  meaningful result for a real corpus. If the honest answer is "this corpus has no god nodes",
  return an explicit empty result with a reason rather than a bare `[]`.
- **New tests** in `src/server/mcp/tests.rs`:
  - `health_check_embedding_ok_true_when_embedder_available`
  - `sys_health_does_not_report_hardcoded_zero_benchmarks`
  - `codegraph_gods_excludes_generic_constructors`
- Risk: MED — `health_check` output is consumed by every agent. Field names and the boolean
  semantics must stay backward compatible.

## 🌐 Web Research Required

**MANDATORY — 4 queries. Research BEFORE implementing.**
1. search: "Rust Option<f64> serialize null vs sentinel value API design"
2. search: "graph centrality ranking exclude trivial nodes god nodes"
3. search: "Rust serde flatten struct adding field backward compatible"
4. search: "MCP tool result schema stability backward compatibility"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read these files COMPLETELY before editing:
   - `src/server/mcp/tools_core.rs` — focus on lines 630-760 (health_check, sys_health,
     log_scan, env_status arms) and line 1000-1084 (codegraph_gods).
   - `src/server/mcp/types.rs` lines 160-225 (MCPHealthResult + MCPToolResult helpers) to
     understand the serialization contract you must not break.
   - `src/server/mcp/tests.rs` around lines 1076, 1376, 2138-2247 for existing coverage.
3. Execute in 3 micro-phases:
   - **Phase 1 — Types & Contracts**: decide and document how 'unmeasured' is represented.
     Add the 3 tests first if that helps you pin the contract.
   - **Phase 2 — Core Logic**: fix `health_check` mapping, fix the `sys_health` benchmark
     construction, fix the `codegraph_gods` ranking.
   - **Phase 3 — Tests & Delivery**: run the full verification block, then create the PR.
4. **Blocker Recovery**: on any compiler/test failure, search the web for the exact error
   signature, fix it, document it in the PR body. NEVER wait for feedback."

## Existing Code Patterns (MUST follow)

- `db_integrity` is already computed at lines 669-671 by scanning `in_process_health.checks`
  for `c.name == "sqlite_integrity"`. Reuse that exact pattern; do not invent a second probe.
- `MCPToolResult::structured(...)` is the established wrapper (see lines 657-660, 705-708).
  Keep using it. The `is_error` flag means *tool execution* failure, not host state — the
  comment at lines 654-656 explains this; preserve that semantic.
- JSON is built with `serde_json::json!({...})` (see lines 697-703). Match it.
- Comments in English, minimal density.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep -c "^error"` == 0
- [ ] `cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep -c "^error"` == 0
- [ ] `cargo test --package xavier --lib --features ci-safe mcp:: -- --test-threads=1 2>&1 | grep "test result"` shows 0 failed
- [ ] `cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | grep "test result: FAILED"` produces NO output
- [ ] `grep -n "recall_at_k: 0.0" src/server/mcp/tools_core.rs` returns NO match
- [ ] `grep -n "total_documents: 0" src/server/mcp/tools_core.rs` returns NO match
- [ ] `grep -n "size_mb >= 0.0" src/server/mcp/tools_core.rs` returns NO match
- [ ] `git diff --stat HEAD -- src/server/mcp/types.rs` is EMPTY (contract untouched)
- [ ] `cargo fmt --all -- --check` produces no diff
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/server/mcp/tools_core.rs` | 1084 lines; health_check at 645, sys_health at 662-708, codegraph_gods at 1020 | Fix embedding_ok, fix memory_store_ok, de-hardcode benchmarks, fix gods ranking | MED |
| `src/server/mcp/tests.rs` | existing coverage at 1076, 1376, 2138-2247 | Add 3 new tests | LOW |

## DO NOT touch (Anti-Regression)

- `src/health/mod.rs` — **island of WAVE-29.01**. It is fixing `EmbeddingCoverage::default()`
  and adding `degraded_reasons` in parallel. You consume its output; do NOT edit it.
- `src/server/mcp/types.rs` — the serde contract. Read it, do not change it.
- `src/self_manage/mod.rs` — island of WAVE-29.03.
- `src/context/skill_dispatcher.rs`, `src/context/skill_registry.rs` — island of WAVE-29.04.
- `src/memory/**` — island of WAVE-29.09.
- `.gitcore/features.json`, `Cargo.toml`, `Cargo.lock`.
- Do NOT rename or remove any MCP tool. Tool count is asserted in tests (42 tools).

## Anti-Hallucination Guard ⚠️

1. **READ before write**: read tools_core.rs around each arm before editing. The `match` arms
   share an `arguments` binding; a careless edit can break sibling arms.
2. **Do not "fix" the health probe**: `health.embedding.connected` being false is a separate
   defect. Your job is the MCP *mapping* layer. If you conclude the probe itself is wrong,
   say so in the PR body — do not patch `src/health/mod.rs`.
3. **Do not add dependencies** to read graph data; the code-graph crate is already a workspace
   member. Check `Cargo.toml` before assuming anything is available.
4. **`cargo check` gate**: 0 errors before commit.
5. **No features.json edit.**

## PR Delivery Requirements (ANTI-EMPTY-PR) — OBLIGATORIO

- [ ] `git status --porcelain` shows `src/server/mcp/tools_core.rs` modified BEFORE opening the PR
- [ ] `git diff --stat HEAD` is NOT empty
- [ ] `git show HEAD --name-only | grep -c "src/server/mcp/"` >= 1
- [ ] `grep -c "fn .*embedding_ok\|fn .*gods_excludes\|fn .*hardcoded_zero" src/server/mcp/tests.rs` >= 3
- [ ] PR body shows the BEFORE/AFTER of the `MCPHealthResult` construction
- [ ] PR body states explicitly, for each benchmark field, whether it is now MEASURED or UNMEASURED
- [ ] If the work could not be completed: DO NOT open a PR — comment the blocker on this issue.

## Verification

```bash
cargo fmt --all -- --check
cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep "^error" | wc -l   # expect 0
cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep "^error" | wc -l  # expect 0
cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | tail -20
```

## Dependencies & Merge Order

- **Depends on:** nothing. Merge order **2/10**.
- **Parallel with:** 29.01, 29.03, 29.04, 29.05, 29.06, 29.07, 29.08, 29.09, 29.10.
- **Consumed by:** 29.10 (E2E asserts `health_check.embeddingOk` agrees with `GET /health`).
- **Expected effort:** Medium 2-4h

## Failure Recovery

| If this happens | Action |
|----------------|--------|
| `cargo check` fails | Fix errors, do NOT commit broken code |
| A pre-existing `mcp::` test fails | Confirm it fails on `origin/main` first (`git stash && cargo test`) |
| Making benchmarks `Option` breaks `analyze_gaps` signature | That function is owned by WAVE-29.01/auto_improvement — do NOT edit it. Use a sentinel value + comment instead, and explain the tradeoff in the PR body |
| `codegraph_gods` has no access to symbol names to filter | Use what the query already returns; if you cannot filter by name, deduplicate by name and document the limitation |
| Tool count assertion fails (42) | You added or removed a tool — revert that; this issue must not change the tool surface |
