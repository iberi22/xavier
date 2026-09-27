# [WAVE-29.09] feat-memory-write-integrity — memory store accepts corrupt content and indexes one path twice

> Wave 29 — Memory integrity. Labels: `wave-29`, `ola29` (NO `jules` yet)
> Merge order: 9/10 | Risk: MED | Effort: Medium 2-4h

---

## Current State (MEDIBLE)

Files: `src/memory/` — the write/insert path and the search index. Relevant symbols to locate
with grep before editing: the memory record insert used by the create-memory API, the FTS/search
index sync, and the entity extraction. MCP surface: `xavier_create_memory`,
`xavier_memory_save`, `xavier_mem_search`.

**Defect 1 — the write path accepts malformed content with no validation.**
During a live audit, a memory document was written containing corrupted mixed-language text —
fragments such as `cobertura OTRO PROYECTO`, `skills nommé`, `no tiene.noción`,
`ninguno de los cualesPassed review`, `La全区 prioritario`. Several of these destroy the meaning
of the sentence they sit in (e.g. `ninguno de los cualesPassed review`).

Xavier accepted it with **no error and no warning**:
- `xavier_create_memory` returned success.
- `xavier_get_memory` returned the content **verbatim**, with
  `"primary": true, "revision": 1, "zone": "atomic"` and **no integrity marker of any kind**.

Consequence: that garbage is now part of the corpus and will be retrieved as context by future
agents. A memory system whose whole purpose is to be trusted as recall has no validation gate on
write and no way for a reader to tell good content from corrupted content.

**Defect 2 — one logical document produced two index entries.**
Observed sequence:
```
xavier_create_memory(path="projects/xavier/audit-self-review-2026-09-26")
  -> returned id 01M3F3FTV8C3XQ5DQ3PE2WASRB
xavier_mem_search(same topic)
  -> candidate 1: id 01M3F3G02FP35B1N9Z470P8HAA, score 2.4193832874298096
  -> candidate 2: id 01M3F3FTV8C3XQ5DQ3PE2WASRB, score 2.4185962677001953
xavier_get_memory("01M3F3G02FP35B1N9Z470P8HAA")
  -> same path, "primary": true
```
Two different ids, the **same** `path`, scores differing in the 4th decimal, and the returned
write id is not the id that resolves as primary. The search index holds a duplicate entry per
path, so every future search over that path returns two near-identical candidates and burns the
result budget. It also breaks the caller contract: the id you are handed back is not the id you
get when you look it up.

## Desired State (DELTA)

- **Validate on write.** Reject or sanitise content that is structurally broken, at a
  documented boundary. Be conservative: Xavier legitimately stores many languages and
  code/JSON payloads, so do NOT build an English-only or ASCII-only filter — that would reject
  legitimate content. Target the failure modes actually observed:
  - mixed-script sentences with no word boundaries (e.g. `los cualesPassed`, `全区 prioritario`),
  - a stray `word.Word` join inside prose,
  - content that is not valid UTF-8 or contains control characters.
  Define the rule, put it in a named function, and document what it rejects and why.
- **Make the decision explicit and observable.** A rejected write must return a clear error (or a
  documented sanitisation result) — never a silent success. A caller must always be able to tell
  whether its content was stored verbatim.
- **Mark integrity on read.** Records should carry an integrity signal so a consumer can
  distinguish verified content from content that bypassed or failed validation. Keep it
  backward compatible: existing records have no marker and must still load.
- **Fix the duplicate index entry.** One logical document (per `path` within its workspace) must
  produce exactly ONE search candidate. Resolve the id contract: the id returned by a write must
  be the id that resolves on subsequent lookup, or the API must clearly document the primary/alias
  relationship and make lookups by either id work. Choose ONE coherent model and implement it.
- **New tests**:
  - `create_rejects_mixed_script_corruption`
  - `create_accepts_multilingual_valid_content` (guard against over-filtering — this test is
    mandatory and must contain genuinely valid non-English content)
  - `create_is_idempotent_per_path` (writing the same path twice yields one search candidate)
  - `returned_id_resolves_on_lookup`
  - `legacy_records_without_integrity_marker_still_load`
- Risk: MED. This is the write path of the primary data store. Every agent in the ecosystem
  writes here. A filter that is too aggressive is a data-loss incident.

## 🌐 Web Research Required

**MANDATORY — 4 queries. Research BEFORE implementing.**
1. search: "detect mixed script words corrupted text unicode script boundary validation"
2. search: "content addressed id idempotent upsert unique index on path"
3. search: "unicode NFC normalization sanitize text storage best practice"
4. search: "data validation on write vs repair on read database design tradeoff"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. FIRST, explore and report the actual write path with grep before editing. Find:
   - the function behind the create-memory API (`xavier_create_memory` / `xavier_memory_save`)
   - the record insert / upsert (SQLite; note `code-graph`, `crates/xavier-core-logic` may also
     hold store code — establish which crate owns the canonical store)
   - the FTS / search-index sync that populates `xavier_mem_search` candidates
   - how `primary` and `revision` are assigned, and where the id returned to the caller is minted
   Read the relevant files COMPLETELY. Do not guess.
3. Write the failing tests FIRST for both defects, using these exact reproductions:
   - content: `"La全区 prioritario no es agregar features: es dejar de mentir."`
   - content: `"ninguno de los cualesPassed review"` embedded in a longer paragraph
   - then create the same `path` twice and assert exactly one search candidate comes back
4. Execute in 3 micro-phases:
   - **Phase 1 — Types & Contracts**: the validation function + its error type; the integrity
     marker field (additive, with a serde default so legacy rows load).
   - **Phase 2 — Core Logic**: enforce validation on write; fix the duplicate index entry and
     the id contract.
   - **Phase 3 — Tests & Delivery**: all 5 named tests, verification, PR.
5. **Blocker Recovery**: on any failure, search the web for the exact error signature, fix it,
   document it in the PR body. NEVER wait for feedback. NEVER weaken a failing test to make it
   pass — if a test cannot pass, report why in a PR comment instead."

## Existing Code Patterns (MUST follow)

- `anyhow` in binaries, `thiserror` in libs (`AGENTS.md` §8).
- Never call Rayon `.par_iter()` directly inside a Tokio worker — wrap in
  `tokio::task::spawn_blocking` (`AGENTS.md` §8). The embedding/FTS path may use Rayon.
- Existing store traits and `async_trait` patterns in `src/memory/store.rs` — follow them.
- Schema changes must be additive and idempotent; the DB is a long-lived runtime cache on disk
  (`data/*.db`) that must not require a manual migration step to keep working.
- Comments in English, minimal density.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep -c "^error"` == 0
- [ ] `cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep -c "^error"` == 0
- [ ] `cargo test --package xavier --lib --features ci-safe memory -- --test-threads=1 2>&1 | grep "test result"` shows 0 failed
- [ ] `cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | grep "test result: FAILED"` produces NO output
- [ ] `grep -c "create_rejects_mixed_script_corruption\|create_accepts_multilingual_valid_content\|create_is_idempotent_per_path\|returned_id_resolves_on_lookup\|legacy_records_without_integrity_marker_still_load" <your test file(s)>` == 5
- [ ] PR body shows, as literal evidence, the BEFORE (content accepted) and AFTER (rejected or
      sanitised) behaviour for the two corrupted samples
- [ ] PR body shows BEFORE (2 search candidates for one path) and AFTER (1 candidate)
- [ ] `git status --porcelain` shows NO `*.db`, `*.sqlite`, `*-wal`, `*-shm` staged
- [ ] `cargo fmt --all -- --check` produces no diff
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/memory/` (write path + search index sync — locate precisely with grep FIRST) | accepts any content; one path yields two index entries | Content validation + integrity marker + index dedup + id contract | MED |

You may need more than one file inside `src/memory/`, but **every file you touch must be under
`src/memory/`**. If the fix genuinely requires a file outside that directory, STOP and explain in
a comment on this issue instead of expanding scope.

## DO NOT touch (Anti-Regression)

- `src/server/mcp/**` — islands of WAVE-29.02 / 29.10. The MCP memory tools call into
  `src/memory/`; keep their signatures compiling, do not edit them.
- `src/health/mod.rs`, `src/self_manage/mod.rs`, `src/context/**`,
  `src/cli/handlers/doctor.rs`, `src/cli/commands/improve.rs`, `src/auto_improvement/**`,
  `src/cli/handlers/system.rs`, `src/cli/handlers/code.rs`, `code-graph/**` — all other islands.
- `crates/xavier-core-logic/**`, `crates/xavier-wasm/**` — shared with other consumers.
- `vendor/maloca-core/**` — vendored third-party code.
- `.gitcore/features.json`, `Cargo.toml`, `Cargo.lock`, `tests/**`, `.github/workflows/**`.
- **Do NOT run `xavier cleanup`, `memory prune`, or any destructive store operation**, and do not
  delete existing memories to "clean up" the duplicates. Fix the code path; leave the data alone.
- Do NOT add a new dependency for Unicode segmentation. Hand-roll the narrow check you need.

## Anti-Hallucination Guard ⚠️

1. **READ before write, and REPORT what you found.** The exact function that inserts a memory and
   the one that writes the search index are not named in this issue. Find them with grep. If the
   store is split across crates, say which crate is canonical in your PR body.
2. **Do NOT build a language filter.** Xavier stores Spanish, English, Japanese, Chinese and code.
   A naive non-ASCII rejection would destroy the corpus. Your `create_accepts_multilingual_valid_content`
   test is the guard rail — make it meaningful, with real valid content in several languages.
3. **Do NOT silently mutate stored content.** If you sanitise, the caller must be told. Silent
   rewriting of an agent's memory is worse than a rejection.
4. **Do NOT "fix" duplicates by deleting rows.** No destructive operation. The write path must
   stop producing them.
5. **Schema changes must be additive and must not require manual migration** of the existing
   on-disk `data/*.db` for the service to keep working.
6. **`cargo check` gate**: 0 errors before commit.

## PR Delivery Requirements (ANTI-EMPTY-PR) — OBLIGATORIO

- [ ] `git status --porcelain` shows only files under `src/memory/` modified, BEFORE opening the PR
- [ ] `git diff --stat HEAD` is NOT empty
- [ ] `git show HEAD --name-only | grep -c "src/memory/"` >= 1
- [ ] `grep -c "#\[test\]\|#\[tokio::test\]"` across your files shows 5 new tests (report before/after)
- [ ] PR body documents the exact validation rule, with the rationale for each clause
- [ ] PR body documents which id model you chose (single id vs documented primary/alias) and why
- [ ] PR body confirms no database file is included in the diff
- [ ] If the work could not be completed: DO NOT open a PR — comment the blocker on this issue.

## Verification

```bash
cargo fmt --all -- --check
cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep "^error" | wc -l   # expect 0
cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep "^error" | wc -l  # expect 0
cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | tail -20
```

## Dependencies & Merge Order

- **Depends on:** nothing. Merge order **9/10**.
- **Parallel with:** 29.01, 29.02, 29.03, 29.04, 29.05, 29.06, 29.07, 29.08, 29.10.
- **Expected effort:** Medium 2-4h

## Failure Recovery

| If this happens | Action |
|----------------|--------|
| `cargo check` fails | Fix errors, do NOT commit broken code |
| Validation cannot distinguish corruption from valid multilingual text | NARROW the rule. A rule that only catches mixed-script-within-a-word is safe; a rule that rejects non-ASCII is not. Ship the narrow rule |
| The index duplicate originates in a hybrid-search/RRF layer outside `src/memory/` | Do NOT expand scope. Comment on this issue with the file and line so the orchestrator can route it |
| Dedup requires a unique index that existing data violates | Make the lookup path dedup defensively at query time; do not mutate the table |
| An existing test writes arbitrary junk fixtures and now fails | That fixture encodes the old behaviour. Update it, but ONLY if the new content is genuinely corrupt — otherwise your filter is too aggressive |
| `id` is a ULID/ULID-like minted elsewhere | Trace the mint site; do not change the id format |
