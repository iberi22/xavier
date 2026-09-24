# FEATURE: Session Context Fusion Injects Matched Skills

**Status:** `implemented` (Implemented 2026-09-21 (integrated from feat/304-fusion worktree)) | **Score:** server::http::context 5 pass | **Last Tested:** 2026-09-21

## Overview
The `session/compact` flow (`src/server/http/context.rs:128`) calls `builder.build(level, &selected_docs, &[], &[])` — skill/memory slots permanently empty ("integrated later in fusion step"). The builder (`builder.rs:43`) and dispatcher (`ContextPack`, `compacted_content` budget truncation) are already built; nothing feeds them. This feature wires dispatch into fusion on `ContextLevel::Maximum` with a 0.5 confidence gate and a trivial-prompt ack gate, keeping Minimal/Medium byte-identical.

## Architecture & Design
- Maximum level only (classifier already defines Maximum as "full retrieval + skills").
- `confidence >= 0.5` → inject `compacted_content(remaining_budget)` + pack memories; below → explicit no-skill verdict (Jev-inspired abstention, local).
- Ack gate: greetings/acks/short confirmations skip dispatch on a documented 0-ms predicate.
- Thresholds as named constants; existing whitespace token convention kept (no new tokenizer here).
- The line-128 TODO is removed when wired (strict pipeline scans stubs).

## Implementation Paths
- `src/server/http/context.rs` (dispatch + gates + builder feed)
- `src/context/builder.rs` (only if skill-payload shaping needs changes)

## Sub-features
- **Maximum-only dispatch:** level gating via existing classifier.
- **Confidence gate 0.5:** named constant, tested both sides.
- **Ack gate:** trivial-prompt predicate, documented canonically (#306).
- **Budget respect:** injection never exceeds remaining budget.

## Test References
- `test_fusion_injects_skill_on_maximum`, `test_fusion_skips_below_confidence`, `test_fusion_skips_trivial_prompt`, `test_fusion_respects_token_budget` + live `/session/compact` probes (architecture prompt vs `"ok thanks"`).

## Known Issues & Notes
- Depends on #301 (non-empty registry) + #302 (calibrated confidence). Per-request reindex perf is out of scope (follow-up if measured).
