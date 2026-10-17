#!/usr/bin/env bash
# affected-tests-batch-test.sh — verify batching of affected lib and integration tests
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

OUT1="$(scripts/affected-tests.sh --dry-run --files src/memory/access.rs src/memory/repo_store.rs tests/access_flush_drain.rs)"

# Check batched test commands
# Under nextest or fallback cargo test:
# 1) Build step or test invocation includes --lib and multiple targets
# 2) Both memory::access and memory::repo_store are passed (either in filter or -E expr)
# 3) Both integration targets access_flush_drain and tgd_prune_gate_test are passed

if ! echo "$OUT1" | grep -q "memory::access.*memory::repo_store"; then
    echo "FAILED: lib filters not batched together in command"
    echo "$OUT1"
    exit 1
fi

if ! echo "$OUT1" | grep -q "access_flush_drain.*tgd_prune_gate_test"; then
    echo "FAILED: integration targets not batched in single invocation"
    echo "$OUT1"
    exit 1
fi

# Check --no-integration flag
OUT2="$(scripts/affected-tests.sh --dry-run --no-integration --files src/memory/access.rs src/memory/repo_store.rs tests/access_flush_drain.rs)"
if echo "$OUT2" | grep -F " --test " | grep -q "cargo test"; then
    echo "FAILED: --no-integration should not produce integration test commands"
    echo "$OUT2"
    exit 1
fi
if ! echo "$OUT2" | grep -q "deferred to pre-push/CI: access_flush_drain tgd_prune_gate_test"; then
    echo "FAILED: --no-integration should report deferred targets"
    echo "$OUT2"
    exit 1
fi

# Check WI_BATCH_VALIDATE=1 env var (implies --quick equivalent behavior)
OUT3="$(WI_BATCH_VALIDATE=1 scripts/affected-tests.sh --dry-run --files src/memory/access.rs src/memory/repo_store.rs tests/access_flush_drain.rs)"
if echo "$OUT3" | grep -q "cargo test"; then
    echo "FAILED: WI_BATCH_VALIDATE=1 should not produce test commands"
    echo "$OUT3"
    exit 1
fi

echo "affected-tests-batch-test: PASS"
