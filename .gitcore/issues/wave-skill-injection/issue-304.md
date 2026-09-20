# [Ola skill-injection] feat-skill-context-fusion — Session Fusion Injects Skills

> Ola skill-injection — Local-first intelligent skill injection, 100% offline (no Jev API).
> Labels: `wave-skill-injection`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` muestra los archivos nuevos/modificados ANTES de abrir el PR
- [ ] `git diff --stat HEAD` lista los archivos (NO vacío)
- [ ] El PR DEBE contener ≥1 archivo: verificar con `git ls-files` antes de push

## Current State (MEDIBLE)
- `src/server/http/context.rs:128` — `builder.build(level, &selected_docs, &[], &[])` with comment *"memories and skills are empty as they'll be integrated later in fusion step"*.
- `src/context/builder.rs:43` — `build(level, docs, memories, skills)` already accepts skill payloads; the wire is built, nothing feeds it.
- `src/context/classifier.rs` — `ContextLevel::Maximum` already means "full retrieval + skills" (keyword + length trigger). `SkillDispatcher::dispatch` (`skill_dispatcher.rs`) returns `ContextPack { system_instructions, relevant_memories, prior_decisions, total_tokens }` + `confidence`.
- `IndexedSkill::compacted_content(max_tokens)` (`skill_registry.rs:39-47`) already truncates to budget.

## Desired State (DELTA)
- **Update**: `src/server/http/context.rs` — on `ContextLevel::Maximum` only, call the dispatcher with the session prompt; on `confidence >= 0.5`, feed `compacted_content(remaining_budget)` + pack memories into `builder.build`; below threshold feed nothing (explicit no-skill verdict, Jev-inspired).
- **Update**: trivial-prompt ack gate before dispatch (greetings/acks/short confirmations skip dispatch entirely, 0 ms path) — document the exact predicate in code + spec.
- **Update**: remove/replace the line-128 TODO comment when wired (strict pipeline scans stubs).
- **Keep**: Minimal/Medium levels byte-identical in behavior (no skill path).

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "RAG confidence threshold abstention no-retrieval verdict"
2. search: "token budget allocation retrieval vs instructions LLM context"
3. search: "trivial prompt detection acknowledgment classifier chatbot"
4. search: "Axum shared state registry per-request reindex background refresh"

## 🔬 Agent Session Prompt
"Before implementing, please:
1. Read `src/server/http/context.rs` around line 128 + `builder.build` signature + `dispatch()` in full.
2. Note the dispatcher currently reindexes per request — decide: reuse as-is for this issue, or flag follow-up (do NOT silently add background tasks here; that belongs to a new issue).
3. Define the ack-gate predicate from data (what counts as trivial), not from intuition."

## Existing Code Patterns (DEBES seguir estos)
- `src/api/skills.rs::dispatch_skill` → dispatcher construction pattern to mirror (registry + memory + dispatch).
- `src/context/classifier.rs` → level gating pattern.
- Token counting: existing `split_whitespace` convention used across context module (do not introduce tiktoken here).

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] E2E: `POST /session/compact` with architecture prompt returns context containing the expected skill's instructions within `token_budget`
- [ ] E2E: same endpoint with `"ok thanks"` returns skill-free context (ack gate)
- [ ] E2E: low-confidence task returns skill-free context (gate 0.5)
- [ ] `rg -n "integrated later in fusion" src/server/http/context.rs` → 0 hits
- [ ] `cargo test -p xavier --lib context` — green incl. 4 new fusion tests; clippy `-D warnings` clean; fmt clean

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/server/http/context.rs` | Exists, placeholder | Dispatch + gate + feed builder | MED |
| `src/context/builder.rs` | Exists | Only if skill-payload shaping needs changes | LOW |
| New/updated tests | — | 4 fusion tests | LOW |

## DO NOT touch (Anti-Regression)
- `Minimal`/`Medium` code paths.
- Dispatcher/registry internals (owned by #301/#302).
- Per-request reindex performance work (out of scope; file follow-up if measured slow).

## Anti-Hallucination Guard ⚠️
1. **READ before write**: full context.rs flow + builder.build + dispatch().
2. **Thresholds are constants with names**, not magic numbers scattered in logic.
3. **No cloud**: grep diff for network clients — must be absent.

## Verification
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p xavier --lib context
# live: POST /session/compact architecture-prompt vs "ok thanks" (commands in PR)
```

## Dependencies & Merge Order
- **Depends on:** #301 (non-empty registry), #302 (calibrated confidence)
- **Parallel with:** #305
- **Merge order within wave:** 4
- **Expected effort:** Medium 2-3h
