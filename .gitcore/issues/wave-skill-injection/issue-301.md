# [Ola skill-injection] feat-skill-scan-paths — Registry Scans Canonical Store

> Ola skill-injection — Local-first intelligent skill injection, 100% offline (no Jev API).
> Labels: `wave-skill-injection`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` muestra los archivos nuevos/modificados ANTES de abrir el PR
- [ ] `git diff --stat HEAD` lista los archivos (NO vacío)
- [ ] El PR DEBE contener ≥1 archivo: verificar con `git ls-files` antes de push

## Current State (MEDIBLE)
- `src/context/skill_registry.rs:68-74` — `SkillRegistry::with_defaults` scans only `workspace_root/skills` + `workspace_root/.agents/skills`.
- Live evidence 2026-09-20: `GET /skills` → `{"count":0,"skills":[]}`; every `POST /api/skill/dispatch` → `skill_name:"_none"`, `confidence:0.0`.
- Canonical store `~/.hermes/skills` holds 349 active skills and is NEVER scanned.
- `reindex()` (`skill_registry.rs:77-111`) uses `WalkDir` with no cycle protection; a symlink cycle (cf. karpathy-principles 2-cycle, fixed 2026-09-20) would loop forever.

## Desired State (DELTA)
- **Update**: `src/context/skill_registry.rs` — resolve `$HOME` via env (never hardcode `/home/belal`), add `~/.hermes/skills` to default scan paths.
- **Update**: traversal tracks visited `(device, inode)` pairs and skips already-seen dirs (cycle-safe), still follows file symlinks (system convention: 228 symlinks under `.agents/skills`).
- **Update**: when readable, parse Hermes `config.yaml` `skills.disabled` list and exclude those names from the index.
- **Keep**: sha256 incremental reindex behavior unchanged.

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "WalkDir symlink loop detection device inode Rust"
2. search: "Rust expand home directory without hardcoding dirs crate"
3. search: "sqlite-vec incremental index hash change detection pattern"
4. search: "Hermes agent skills.external_dirs skills.disabled config semantics"

## 🔬 Agent Session Prompt
"Before implementing, please:
1. Read `src/context/skill_registry.rs` fully (345 lines) — do not assume the API.
2. Check how `workspace_root` is derived at the call sites in `src/api/skills.rs:69-71,184`.
3. Verify no personal paths land in code (repo rule: 12-factor, `.env.example` for new env vars)."

## Existing Code Patterns (DEBES seguir estos)
- `src/context/skill_registry.rs` → `reindex` + `index_skill_file` + `parse_frontmatter` patterns.
- Error style: `anyhow` in binary paths (AGENTS.md §8).

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `GET /skills` against a HOME containing the real store returns `count >= 300`
- [ ] `cargo test -p xavier --lib context::skill_registry` — all green, incl. new `test_registry_symlink_cycle_terminates` (tempdir with A→B→A links finishes <5s)
- [ ] `rg -n "/home/belal" src/context/skill_registry.rs` → 0 hits (no hardcoded paths)
- [ ] `cargo clippy --all-targets -- -D warnings` — 0 warnings
- [ ] `cargo fmt --check` — clean

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/context/skill_registry.rs` | Exists, 345 lines | Scan paths + cycle guard + disabled filter | MED |
| `src/api/skills.rs` | Exists | Only if call-site root resolution must change | LOW |

## DO NOT touch (Anti-Regression)
- `src/memory/*`, `src/server/mcp/*`, `src/codebase/*` — File Islands boundary.
- `.env`, `XAVIER_TOKEN`, any secret handling.
- Live `GET /skills` behavior contract (`{ok,count,skills[]}` shape stays).

## Anti-Hallucination Guard ⚠️
1. **READ before write**: `skill_registry.rs` + both call sites in `api/skills.rs`.
2. **No network**: registry scan is pure filesystem; tests must use `tempfile::tempdir`, never `$HOME` real dirs.
3. **Fail-open preserved**: missing scan dir → `debug!` + skip (existing behavior), never error.

## Verification
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p xavier --lib context::skill_registry
curl -s -m 20 "http://localhost:8006/skills" -H "X-Xavier-Token: $XAVIER_TOKEN" | python3 -c "import json,sys; print(json.load(sys.stdin)['count'])"
```

## Dependencies & Merge Order
- **Parallel with:** #303
- **Blocks:** #302 (embeds at index time), #304, #305
- **Merge order within wave:** 1
- **Expected effort:** Medium 2-3h
