# [PAGEINDEX.06] Xavier glue: features, settings, tool logic, .env.example

> Wave pageindex — F1 | Feature: `feat-pageindex-tree-core` | ADR-036 | Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`
> Depends on: 05 | Parallel group: G3 (alone; owns root Cargo.toml + lib.rs) | Risk: MED | Effort: Medium 3-4h
> Worktree/branch: dedicated `feat/pageindex-06` from `feat/pageindex` (or main once merged). One agent, one session.

---

## Current State (MEDIBLE)

- Xavier has no dependency on the crate. `src/lib.rs` lists top-level modules.

## Desired State (DELTA)

Add dependency `xavier-pageindex` (path, optional) and features `pageindex` (added to `default` and `ci-safe`), `pageindex-pdf`, `pageindex-pdfium` (spec sec. 4). New module `src/pageindex_glue/`: `settings.rs` (`PageIndexSettings::from_env`, ALL env reads live here), `state.rs` (shared `Arc<PageIndex<SqliteStore>>`, lazy open), `tools.rs` (protocol-agnostic: `tool_specs() -> Vec<ToolSpec{name,description,input_schema}>` and `async call(name, args, ctx{workspace, can_write}) -> serde_json::Value` returning `{ok:false,error,hint}` envelopes, never panicking; `pageindex_index_document` denied when `!can_write`; `path` must be inside `XAVIER_PAGEINDEX_INGEST_ROOTS`). Add every env var of spec sec. 8 (F1 subset) to `.env.example`. Includes the legal-chunker parity test.

## Files to Modify

| File | Change | Risk |
|------|--------|------|
| `Cargo.toml (root)` | add optional dep + 3 features + `ci-safe`/`default` wiring | MED |
| `src/lib.rs` | add `pub mod pageindex_glue;` (cfg feature `pageindex`) | MED |
| `src/pageindex_glue/mod.rs` | new | MED |
| `src/pageindex_glue/settings.rs` | new | MED |
| `src/pageindex_glue/state.rs` | new | MED |
| `src/pageindex_glue/tools.rs` | new | MED |
| `.env.example` | append PAGEINDEX vars | MED |

## Tests that prove it

- `test_tool_specs_names_and_required_params`
- `test_tool_errors_are_envelopes_not_panics`
- `test_ingest_tool_denied_for_readonly_role`
- `test_ingest_path_outside_roots_rejected`
- `test_settings_from_env_defaults`
- `test_legal_parity_with_legal_chunker_fixture`

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo test -p xavier --lib --features ci-safe pageindex_glue`
- [ ] `cargo clippy -p xavier --all-targets --features ci-safe -- -D warnings`
- [ ] `cargo check -p xavier (default features)`
- [ ] `scripts/check-secrets.sh`
- [ ] `git status --porcelain` lists ONLY the files in the table above
- [ ] Tests listed above exist and pass (no `#[ignore]` except pdfium-backed ones in issue 10)

## Read first

- `src/rag/llm_adapter.rs` (env style), `src/skills`/`src/server/mcp/tools_context.rs` skill tools (envelope convention `{ok:false}`), `.env.example`
- `AGENTS.md` (fmt, clippy -D warnings, English comments, `thiserror` in libs, no hardcoded env, Rayon/Tokio rule)

## DO NOT touch (Anti-Regression)

- `src/server/mcp/{mod,server,tools_core}.rs` (other wave), `src/context/**` (other wave)
- `.gitcore/features.json` — status promoted only by a green `scripts/verify-pipeline.sh`, never by hand.
- `src/server/mcp/{mod,server,tools_core}.rs` unless this is issue 08 (V6 secrets wave owns them).
- `src/context/skill_controller/*`, `src/context/mod.rs` (another wave).
- Any file assigned to a parallel issue of this wave (keep file islands disjoint).

## Anti-Hallucination Guard

1. READ before write; verify every referenced symbol with grep.
2. No invented crates: new deps only as listed in the spec (sec. 4); keep default/`ci-safe` free of native libs.
3. New `std::env::var` => add key to `.env.example` in the same PR (read only in `pageindex_glue/settings.rs`).
4. Note: main CI has a pre-existing intermittent failure in "Rust Integration (Observability Contract)" (#2701); do not chase it here.

## PR Delivery Requirements

- [ ] 1 PR = this issue only; conventional commit `feat(pageindex): ...`; no co-author/AI trailers.
- [ ] `git diff --stat origin/main` is non-empty and matches the files table.
- [ ] If blocked, do not open an empty PR; comment the blocker on the issue.
