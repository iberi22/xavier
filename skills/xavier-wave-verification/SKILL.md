---
name: xavier-wave-verification
title: Xavier Wave Verification & Pipeline Execution Protocol
description: Canonical protocol to run the GitCore 3.8 feature verification pipeline, validate features.json against real test executions, and ensure strict zero-regression quality.
tags:
  - xavier
  - gitcore
  - verification
  - wave
  - testing
category: testing
---

# Xavier Wave Verification Protocol

> **PURPOSE:** Validate that all declared features in `.gitcore/features.json` are genuinely tested, green, and verifiable via automated execution rather than hand promotion.

## 1. Principles
- **Source of Truth:** `.gitcore/features.json`.
- **Golden Rule:** Features are NEVER manually promoted to `stable` or `completed`. Only a green run of `scripts/verify-pipeline.sh` executes the declared tests and validates status.
- **Clippy Policy:** Zero warnings (`cargo clippy --all-targets -- -D warnings`).

## 2. Verification Commands

```bash
# 1. Full pipeline verification (as run in CI)
./scripts/verify-pipeline.sh

# 2. Check only mode (validates schema & test status without full execution)
./scripts/verify-pipeline.sh --check-only

# 3. Targeted feature test execution
cargo test --test <feature_test_name>

# 4. Format & clippy enforcement
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

## 3. Updating the Ledger
- To register a new feature, edit `.gitcore/features.json` setting `status: "planned"`.
- Implement tests matching the pattern declared in `features.json`.
- Execute `./scripts/verify-pipeline.sh` to let the verifier record the cryptographic execution run.
