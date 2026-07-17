# Devlog: Ola 3 UsageCounters (#578)

**Date:** 2026-07-17  
**Why:** `/v1/account/usage` returned hardcoded zeros and `/v1/usage` returned fake JSON. Operators could not see real LLM traffic, fallback hops, or memory-fallback hits.

## Decision

Implement a process-local `UsageCounters` (`parking_lot::RwLock`) shared between `ProxyUseCase` and `CliState`, rather than re-aggregating SQLite rate-limit logs on every request. RateLimitManager remains the durable quota store; counters are the cheap live snapshot.

## Changes

- `src/observability/usage_counters.rs` — `record_success/error/fallback_hop/memory_fallback` + snapshot
- `ProxyUseCase` instruments success + 3 error paths + `handle_provider_fallback`
- `account_usage_handler` and `headless_usage` expose real snapshot
- Panel memory-fallback path calls `record_memory_fallback`

## Jules note

Jules PR #603 was an empty commit (0 files). Re-implemented by orchestrator.

## Verification

```
cargo check --workspace  # OK
cargo test -p xavier --lib usage_counters  # 2 passed
```
