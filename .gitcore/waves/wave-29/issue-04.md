# [WAVE-29.04] feat-skill-dispatch-confidence — dispatch returns confidence 1.0 for an irrelevant skill

> Wave 29 — Observability honesty. Labels: `wave-29`, `ola29` (NO `jules` yet)
> Merge order: 4/10 | Risk: MED | Effort: Medium 2-4h

---

## Current State (MEDIBLE)

Files: `src/context/skill_dispatcher.rs` (~270 lines), `src/context/skill_registry.rs` (~950 lines)

**Defect 1 — `dispatch` takes `matches.first()` unconditionally, with no confidence floor.**
`src/context/skill_dispatcher.rs` lines 76-97:
```rust
let matches = self.registry.search(&request.task, 3);
let (confidence, skill) = if let Some((score, skill)) = matches.first() {
    (*score, (*skill).clone())
} else {
    return Ok(SkillDispatchResult { skill_name: "_none", ..., confidence: 0.0, ... });
};
```
The `_none` branch fires only when the registry returns **zero** candidates. Any single candidate
is accepted at face value, whatever its score.

Measured production failure. Task submitted:
`"auditar la salud del servicio xavier y corregir el consumo de memoria y swap-thrash del servicio systemd"`
Response:
```json
{"skill_name":"hermes-session-indexing","confidence":1.0,
 "estimated_savings_pct":88.15,"ok":true,
 "context_pack":{"system_instructions":"--- name: hermes-session-indexing ... mount NTFS partitions ...","total_tokens":693}}
```
The returned skill instructs the agent to **mount Windows NTFS partitions** — completely unrelated
to systemd memory tuning. `confidence` was the maximum possible value, `1.0`.
Correct skills were present in the same registry and were not selected:
`nixos-system-recovery`, `system-performance-triage`, `xavier-embedding-maintenance`,
`swal-desktop-nixos-fixes`.

This is the most dangerous defect in the audit: an agent that trusts `confidence: 1.0` will
inject unrelated instructions into its own context and may act on them.

**Defect 2 — the registry ingests non-skills as skills.**
`src/context/skill_registry.rs` line 268 logs
`"Skill registry reindex complete: {} skills indexed, {} total"` (also line 369 with a `backfilled`
counter). Measured log line: `2257 skills indexed, 659 total` — the two counters disagree and
neither is explained.

Measured `xavier_skill_list` output (`count: 659`) contains entries that are not skills:
- name `{ fontFamily: "Source Serif 4", cqw: 2.3, weight: 600, lineHeight: 1.0 }`
- name `">-"`, name `"|"`
- name `"Capsule — Frame (video / frame layer)"`
- multiple entries with `description: ""`
- multiple entries with an empty or single-character `description` such as `">-"` and `"|"`

So arbitrary prose from a corpus is being promoted to a dispatchable skill.

**Defect 3 — `system_instructions` carries a whole SKILL.md into the context pack.**
Line 105: `let system_instructions = skill.compacted_content(skill_budget);` with
`skill_budget = (max_tokens as f32 * 0.40) as usize` (line 101). The caller passed
`max_tokens: 800` and the pack still reported `total_tokens: 693` dominated by that dump.
Two risks: context overflow, and prompt injection — the dumped text is untrusted content from
disk presented as system instructions.

Existing tests to preserve: `test_confidence_calibration_range` (line 909) already asserts every
confidence is a measured cosine in `[0,1]` and finite, across paraphrase and degenerate queries
(lines 911-944). Read it — your fix must keep it passing.

## Desired State (DELTA)

- **`skill_dispatcher.rs` lines 76-97**:
  - Introduce a documented minimum confidence threshold (as a named constant with a comment
    explaining the calibration). Below it, return the existing `no-match` shape
    (`skill_name: "_none"`, `confidence: 0.0`) instead of a forced best candidate.
  - A returned `confidence` must be a **measured** score. Never emit `1.0` unless the underlying
    score genuinely reached the top of the range; if the ranking saturates, that is a scoring
    defect to fix, not something to pass through.
  - Do not change the `SkillDispatchResult` field names or the `_none` sentinel string — other
    callers depend on them.
- **`skill_registry.rs` ingestion**:
  - Validate on ingest: a skill needs a name that is a plausible slug/identifier and a
    non-empty, substantive description. Reject and count rejects; log the reject count.
  - Make the "indexed" vs "total" counters coherent, or emit one counter with a clear name.
  - Prune/ignore already-ingested garbage on the next reindex, so the 659-entry list self-heals.
- **`system_instructions`**: cap it by an explicit byte/line budget independent of the caller's
  `max_tokens`, and mark truncated content as truncated. Never present untrusted file content as
  authoritative system instructions without a boundary marker.
- **New tests**:
  - in `skill_dispatcher.rs`: `dispatch_returns_no_match_below_confidence_threshold`
  - in `skill_dispatcher.rs`: `dispatch_never_reports_full_confidence_for_weak_match`
  - in `skill_registry.rs`: `registry_rejects_non_skill_entries`
  - in `skill_registry.rs`: `registry_rejects_empty_description`
  - Keep and pass `test_confidence_calibration_range`.
- Risk: MED — the dispatcher is used by `src/api/skills.rs:61` (`dispatch_skill`) and the MCP tool
  `xavier_dispatch_skill` (registered in `src/server/mcp/tools_context.rs:100`, dispatched at
  line 427). Those are NOT yours to edit; keep their signatures compiling unchanged.

## 🌐 Web Research Required

**MANDATORY — 4 queries. Research BEFORE implementing.**
1. search: "retrieval score threshold reject below cutoff semantic search calibration"
2. search: "prompt injection defense untrusted content boundary marker system prompt"
3. search: "validate markdown frontmatter schema slug regex ingestion pipeline"
4. search: "Rust truncate string by bytes without breaking char boundary"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read these files COMPLETELY before editing:
   - `src/context/skill_dispatcher.rs` (all ~270 lines, including the existing test at 265)
   - `src/context/skill_registry.rs` — the ingest path and the two reindex log sites
     (lines 268, 369), the `search` and `search_with_vector` functions, `score_skill_match`,
     and the whole test module including `test_confidence_calibration_range` (909-950).
3. Reproduce the bug first, in a test, using this exact task string:
   `auditar la salud del servicio xavier y corregir el consumo de memoria y swap-thrash del servicio systemd`
   Assert that the current code does NOT return a high-confidence unrelated skill. Then fix it.
4. Execute in 3 micro-phases:
   - **Phase 1 — Types & Contracts**: add the threshold constant; add ingest validation rules.
   - **Phase 2 — Core Logic**: threshold check in `dispatch`; validation in the registry;
     bounded `system_instructions`.
   - **Phase 3 — Tests & Delivery**: add the 4 named tests, keep the calibration test passing,
     run verification, open the PR.
5. **Blocker Recovery**: on any failure, search the web for the exact error signature, fix it,
   document it in the PR body. NEVER wait for feedback."

## Existing Code Patterns (MUST follow)

- `SkillDispatchResult` / `ContextPack` / `SkillDispatchRequest` are declared in this file
  (see lines 39-60). Extend additively only.
- The `_none` + `confidence: 0.0` early-return shape at lines 84-95 is the established
  no-match convention — reuse it verbatim.
- `info!(skill = ..., confidence, ..., "Skill dispatched")` at the end is the existing
  observability pattern; keep it and consider logging the threshold decision.
- Registry tests use plain `#[test]` with descriptive snake_case names.
- Comments in English, minimal density.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep -c "^error"` == 0
- [ ] `cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep -c "^error"` == 0
- [ ] `cargo test --package xavier --lib --features ci-safe skill -- --test-threads=1 2>&1 | grep "test result"` shows 0 failed
- [ ] `cargo test --package xavier --lib --features ci-safe test_confidence_calibration_range -- --test-threads=1 2>&1 | grep "1 passed"` (or more) — the pre-existing calibration test MUST still pass
- [ ] `cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | grep "test result: FAILED"` produces NO output
- [ ] `grep -n "if let Some((score, skill)) = matches.first()" src/context/skill_dispatcher.rs` returns NO match
- [ ] `grep -n "MIN_DISPATCH_CONFIDENCE\|MIN_CONFIDENCE" src/context/skill_dispatcher.rs` >= 1 match
- [ ] `cargo fmt --all -- --check` produces no diff
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/context/skill_dispatcher.rs` | ~270 lines; `dispatch` 74-97, budget 101, `compacted_content` call 105 | Add threshold + no-match path, bound `system_instructions`, 2 tests | MED |
| `src/context/skill_registry.rs` | ~950 lines; reindex logs 268/369, `search`, `test_confidence_calibration_range` 909 | Ingest validation, coherent counters, 2 tests | MED |

## DO NOT touch (Anti-Regression)

- `src/api/skills.rs` (calls `dispatch_skill` at line 61) — NOT yours. Keep signatures compiling.
- `src/server/mcp/tools_context.rs` (registers `xavier_dispatch_skill` at line 100, dispatches at
  line 427) — **island of WAVE-29.10's assertions / general MCP surface. DO NOT edit.**
- `src/health/mod.rs`, `src/server/mcp/tools_core.rs`, `src/self_manage/mod.rs` — other islands.
- `src/memory/**` — island of WAVE-29.09.
- `.gitcore/features.json`, `Cargo.toml`, `Cargo.lock`.
- Do NOT delete or weaken `test_confidence_calibration_range`. It is the existing guard rail.

## Anti-Hallucination Guard ⚠️

1. **READ before write**: read both files fully. The registry has a vector-search path
   (`search_with_vector`) and a keyword path (`score_skill_match`); the threshold must behave
   sensibly for BOTH — decide and document which one `dispatch` uses.
2. **Do not invent a scoring model.** Fix the calibration of the existing one. If the existing
   score is provably meaningless, say so in the PR body rather than inventing a new formula.
3. **Do not change the public response shape** (`skill_name`, `confidence`, `context_pack`,
   `estimated_savings_pct`) — MCP clients parse it.
4. **Do not add dependencies** for slug validation; a small hand-written check is expected.
5. **`cargo check` gate**: 0 errors before commit.

## PR Delivery Requirements (ANTI-EMPTY-PR) — OBLIGATORIO

- [ ] `git status --porcelain` shows both `src/context/skill_dispatcher.rs` and
      `src/context/skill_registry.rs` modified BEFORE opening the PR
- [ ] `git diff --stat HEAD` is NOT empty
- [ ] `git show HEAD --name-only | grep -c "src/context/"` >= 1
- [ ] PR body contains the **reproduction**: the exact task string, the BEFORE response
      (skill_name + confidence) and the AFTER response
- [ ] PR body states the chosen threshold value AND the reasoning for it
- [ ] PR body shows `test_confidence_calibration_range` still passing
- [ ] If the work could not be completed: DO NOT open a PR — comment the blocker on this issue.

## Verification

```bash
cargo fmt --all -- --check
cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep "^error" | wc -l   # expect 0
cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep "^error" | wc -l  # expect 0
cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | tail -20
```

## Dependencies & Merge Order

- **Depends on:** nothing. Merge order **4/10**.
- **Parallel with:** 29.01, 29.02, 29.03, 29.05, 29.06, 29.07, 29.08, 29.09, 29.10.
- **Expected effort:** Medium 2-4h

## Failure Recovery

| If this happens | Action |
|----------------|--------|
| `cargo check` fails | Fix errors, do NOT commit broken code |
| Threshold breaks `test_confidence_calibration_range` | The calibration test is the guard rail. Re-tune the threshold, do NOT edit that test |
| Registry ingest is async / cached elsewhere | Trace `SkillRegistry` construction before assuming a single ingest site; document what you find |
| `compacted_content` is the only truncation path | Add the bound at the dispatcher call site instead of changing the shared helper (it may be used elsewhere) |
| You cannot reproduce the 1.0 result in a unit test | The registry likely needs a populated index — use the existing test fixtures/patterns in the file and say so in the PR body |
