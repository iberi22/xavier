# Skill Controller — reconciled decisions and scope

Orchestrator's reconciliation of documents 01–04 (2026-09-27). Where documents disagree, this
file wins; each decision names the source it follows. Implementation issues cite decision IDs.

## Decisions

| ID | Decision | Follows | Rejected alternative (why) |
|---|---|---|---|
| D1 | Canonical store: dedicated Git source outside every CLI discovery root, located by `XAVIER_SKILL_STORE` (example `$HOME/.local/share/xavier/skill-store`). Hermes stays authoritative until an approved migration completes. | 01 §4, ADR-034 option C | `~/.agents/skills` as store (publishes the whole catalog to Codex/OpenCode); keep Hermes (couples catalog to one harness) |
| D2 | Publication: **one symlink per selected skill directory**; never symlink a whole discovery directory; copies only as a measured per-adapter fallback. | 01 §4 | Whole-directory links (no per-tool selection, displaces tool-owned entries) |
| D3 | Manifest: **TOML v1** (`toml` crate already in `Cargo.toml`), schema in 01 §3. Explicit placements only, no globs in v1. | 01 §3, 03 | YAML (02): second YAML surface on a deprecated crate; globs (03): implied install-all |
| D4 | Mutation surface: **CLI only** (`xavier skills plan|apply|rollback|compose|release|audit|usage`). REST/MCP stay read-only: drift report, usage summary, existing dispatch. `plan` is default and writes nothing; `apply --plan <file>` rechecks preconditions. | 01 §2, §6 | Mutating REST/MCP endpoints (02): widens the attack surface to network callers |
| D5 | Env vars (12-factor, all added to `.env.example`): `XAVIER_SKILL_STORE`, `XAVIER_SKILL_MANIFEST`, `XAVIER_SKILL_SOURCE_ROOTS`, `XAVIER_SKILL_TARGET_ROOTS` (owner-supplied allowlists; repo content cannot widen them). Defaults reproduce today's scan set exactly. | 01 §3 | 02's `XAVIER_SKILL_DRY_RUN` (plan-by-default makes it redundant), `XAVIER_SKILL_CANONICAL_STORE` naming |
| D6 | Code location: new module `src/context/skill_controller/` (`manifest`, `policy`, `projector`, `composer`, `audit`, `telemetry`) in the `xavier` crate; nothing in `crates/xavier-core-logic`. Requires a narrow ADR-019 amendment inside ADR-034. | 01 §2 | Flat files in `src/context/` (02): fine too, but a submodule keeps the controller's private helpers private |
| D7 | Refactor first: (a) configurable store/config paths with identical defaults; (b) one `build_skill_registry(workspace)` used by REST, MCP and session fusion; (c) name and document both confidence constants (0.40 dispatch, 0.5 fusion). | 02 §c | — |
| D8 | Dependencies: reuse `toml`, `walkdir`, `regex`, `aho-corasick`, `sha2`, `pulldown-cmark`; promote `tempfile` to `[dependencies]`; add `similar` (unified diffs) only in the drift-proposal issue. No `notify` expansion, no new YAML crate in this initiative. | 03 §2, §5 | Replacing `serde_yaml` crate-wide (03): real need, separate initiative — out of scope here |
| D9 | Atomic symlink swap: create the link at a unique temporary name in the target directory (`symlink(src, dir/.tmp-<rand>)`), then `rename` over the destination; fsync the parent directory. (Correction to 03 §3.2, which placed a link over a `NamedTempFile` path.) | 01 §6 | — |
| D10 | Secret scanning: native `RegexSet` scanner over the whole package (SKILL.md + references/scripts/assets) and every composed output, patterns mirrored from `scripts/check-secrets.sh`; findings carry location + kind only. Publication blocked on any finding. | 03 §3.3, 01 §6 | Shelling to gitleaks at runtime (not installed everywhere; git-only) |
| D11 | Ephemeral composition is deterministic (no LLM), bounded by `ephemeral.max_tokens`, provenance-stamped, and scoped by an authorized workspace — the caller's `project` string is not authorization. Memory/decision queries must filter by that scope. | 01 §5 | LLM-written ephemeral skills (unbounded, injection-prone) |
| D12 | Telemetry: local SQLite via the existing migration framework, opaque IDs, no task text/paths/secrets, stages `selected/delivered/invoked/completed`, 30-day retention; never used for automatic deletion. | 01 §5 | Reusing the maintainer-wallet telemetry schema |
| D13 | Drift audit is report-only: offline checks by default; proposed diffs for human approval; never auto-edits secrets, safety or approval rules. | 01 §5, brief | Autonomous rewrites |
| D14 | Rules block and DoD of 04 are mandatory in every implementation issue (cap: 4 files; 400 non-test + 400 test + 50 fixture changed lines per issue, enforced by `swal-preflight review`; amended 2026-09-28 by agent panel). | 04 | — |

## Scope

In: D1–D14; ADR-034 (with ADR-019 amendment); feature specs + ledger entries (`planned`);
adapter acceptance tests per CLI (startup discovery with Xavier stopped); migration runbook.

Out: skill marketplace, remote sync, auto git push, daemons, FUSE/VFS, DSLs, plugin systems,
LLM rewrites of canonical skills, crate-wide YAML replacement, cross-user isolation, changing any
CLI's own discovery configuration.

## Pre-conditions before implementation starts
1. ADR-034 accepted (needs the `skill_controller_store` simulation model — 01 §ADR — or an explicit
   waiver recorded in the ADR).
2. Repo doc conflict fixed: `AGENTS.md` line 92 says `.skills/`, the real dir is `skills/`
   (fix already prepared in the main checkout, not committed).
3. REQ-060 in `docs/SRS/REQUIREMENTS.md` updated to allow a non-Hermes canonical store.

## Ledger entries to add (status `planned`)
`feat-skill-store-config` (D5, D7a), `feat-skill-registry-unify` (D7b, D7c),
`feat-skill-projector` (D1–D4, D9), `feat-skill-ephemeral` (D11), `feat-skill-drift-audit`
(D10, D13), `feat-skill-usage-telemetry` (D12).
