# [Ola skill-injection] feat-skill-loader-fix — SkillLoader Fate Decision

> Ola skill-injection — Local-first intelligent skill injection, 100% offline (no Jev API).
> Labels: `wave-skill-injection`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` muestra los archivos nuevos/modificados ANTES de abrir el PR
- [ ] `git diff --stat HEAD` lista los archivos (NO vacío)
- [ ] El PR DEBE contener ≥1 archivo: verificar con `git ls-files` antes de push

## Current State (MEDIBLE)
- `src/context/skills.rs` (100 lines) — `SkillLoader::validate_skill` (lines 60-64) accepts content only if it contains `"# Purpose"` or `"## Purpose"`.
- Verified 2026-09-20: zero real `SKILL.md` files (sampled across `~/.hermes/skills`, 349 skills) contain that marker — real format is YAML frontmatter (`name:`/`description:`) + markdown body. Effective load count over the real store: **0**.
- `rg -n "SkillLoader" src/` shows no production callers (only its own tests). Effective dead code.

## Desired State (DELTA — implementer decides, documents why)
- **Option A (fix)**: `validate_skill` accepts frontmatter `name:` (+ dir-name fallback, mirroring `skill_registry::parse_frontmatter`), `load_all` targets `SKILL.md` files; probe test loads the real store shape (fixture, not `$HOME`) and returns >0.
- **Option B (remove)**: delete `src/context/skills.rs`, drop `pub mod skills` from `src/context/mod.rs`, migrate any unique coverage into `skill_registry` tests.
- Either way: no dead code remains, `cargo test --workspace` fully green.

## 🌐 Web Research Required
**MANDATORY — 4-6 queries (small, honest).**
1. search: "Agent Skills spec frontmatter name description SKILL.md"
2. search: "Rust dead code removal migrate unit tests checklist"
3. search: "ripgrep count markdown frontmatter fields validation"

## 🔬 Agent Session Prompt
"Before implementing, please:
1. Run `rg -n SkillLoader src/ crates/ --max-count=2` yourself and confirm the caller list.
2. Sample 5 real SKILL.md frontmatters and confirm the `# Purpose` claim.
3. State Option A vs B with one reason grounded in what you found."

## Existing Code Patterns (DEBES seguir estos)
- `src/context/skill_registry.rs::parse_frontmatter` → the real format parser to mirror (Option A).
- `src/context/mod.rs` → module registration to clean (Option B).

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] Option A: fixture-dir probe test returns >0 skills AND `validate_skill` rejects frontmatter-less garbage
- [ ] Option B: `rg -n "SkillLoader|context::skills" src/ crates/ tests/` → 0 hits outside history
- [ ] `cargo test --workspace` — green (or `--workspace --exclude` only with reason in PR)
- [ ] `cargo clippy --all-targets -- -D warnings` — 0 warnings; `cargo fmt --check` — clean

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/context/skills.rs` | Exists, 100 lines | Fix validator OR delete | LOW |
| `src/context/mod.rs` | Exists | Update `pub mod` only under Option B | LOW |

## DO NOT touch (Anti-Regression)
- `src/context/skill_registry.rs` behavior (only mirror its parser).
- Anything outside `src/context/`.

## Anti-Hallucination Guard ⚠️
1. **Probe, don't assume**: the 0-load claim must be re-verified by the implementer's own command, quoted in the PR.
2. **One option only**: do not half-fix AND half-delete.

## Verification
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p xavier --lib context::skills
cargo test -p xavier --lib context::skill_registry
```

## Dependencies & Merge Order
- **Parallel with:** #301, #302
- **Blocks:** nothing (hygiene)
- **Merge order within wave:** 3
- **Expected effort:** Small 1h
