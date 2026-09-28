# ADR-034 acceptance panel — 2026-09-27

## Reviewer 1 — Codex gpt-6-sol, round 1

VERDICT: REJECT

EVIDENCE

- docs/adr/ADR-034-skill-controller.md:75-89 matches the reconciled D1–D14 decisions in docs/design/skill-controller/05-DECISIONS-AND-SCOPE.md:10-23.
- docs/adr/ADR-034-skill-controller.md:17-25 confines the proposed ADR-019 exception to the application crate; docs/adr/ADR-019-plugin-first-boundary.md:90-98 retains the storage-core membership rule.
- docs/adr/ADR-034-skill-controller.md:51-56 matches the simulation report’s figures and labels the estimates as assumptions.
- docs/adr/ADR-034-skill-controller.md:112-115 requires adapter discovery checks **for human acceptance (P03)**. docs/design/skill-controller/06-TASK-DAG.md:17,22,43-44 places P03 before implementation and those checks after implementation.

BLOCKING DEFECTS

- docs/adr/ADR-034-skill-controller.md:112-115 creates an acceptance dependency that the planned sequence cannot satisfy: implementation waits for P03, while the required adapter checks wait for implementation. The ADR must move that gate to rollout or arrange checks before P03.

NON-BLOCKING NOTES

- docs/adr/ADR-034-skill-controller.md:45-51 gives a reproduction command that needs the cited simulator commit checked out; the current simulator checkout lacks that model. Running the cited commit in memory reproduced the reported figures.
- docs/design/skill-controller/01-ARCHITECTURE-AND-ADR.md:156 notes that OpenCode reads a union of discovery directories, so per-tool placement does not provide strict catalog isolation.
## Reviewer 1 — Codex gpt-6-sol, round 2 (after fix)

VERDICT: ACCEPT

EVIDENCE (file:line)
- Decision tracks D1–D14 and identifies the current hardcoded Hermes scan: `docs/adr/ADR-034-skill-controller.md:75-89`; `docs/design/skill-controller/05-DECISIONS-AND-SCOPE.md:10-23`; `src/context/skill_registry.rs:95-114`.
- The ADR-019 amendment is limited to an application-crate module and preserves the storage-core and sidecar boundaries: `docs/adr/ADR-034-skill-controller.md:17-25`; `docs/adr/ADR-019-plugin-first-boundary.md:54-57,90-98`.
- Simulation results are identified as estimates with assumption-based inputs, not field validation: `docs/adr/ADR-034-skill-controller.md:41-56,67-70`.
- P03 acceptance no longer depends on adapter tests. Those tests run as O01/O02 before migration approval O05: `docs/adr/ADR-034-skill-controller.md:110-118`; `docs/design/skill-controller/06-TASK-DAG.md:17,43-47`.

BLOCKING DEFECTS
None.

NON-BLOCKING NOTES
`docs/adr/ADR-034-skill-controller.md:120` says the ADR remains proposed if pilot validation is incomplete on the review date; clarify that wording after P03 acceptance.
## Reviewer 2 — opencode/deepseek-v4.1-flash (co-agent-review.sh verify)

VERDICT: ACCEPT

EVIDENCE:
- ADR-034 decision vs D1–D14: every decision clause maps 1:1 to `docs/design/skill-controller/05-DECISIONS-AND-SCOPE.md:10-23` (D1 store/env, D2 per-skill symlink, D3 TOML v1, D4 CLI-only mutation, D5 four env vars, D6 `src/context/skill_controller/`, D7 refactor-first, D8 deps, D9 atomic swap, D10 secret scan, D11 scoped ephemeral, D12 telemetry, D13 report-only drift, D14 governance). No clause contradicts the reconciled decisions.
- Cited code facts verified: `src/context/skill_registry.rs:106-114` (scan roots = workspace `skills/`, `.agents/skills`, `$HOME/.hermes/skills`); `src/context/skill_dispatcher.rs:79` `MIN_DISPATCH_CONFIDENCE = 0.40` and `src/server/http/context.rs:15` `SKILL_CONFIDENCE_THRESHOLD = 0.5`; `src/context/skill_dispatcher.rs:207-210` decision query filters `kinds: Decision` with no workspace scope (exactly what D11/ADR §10 says); `src/storage/mod.rs:26` `Migration` framework; `src/settings/mod.rs:123` `resolve_data_dir`; `src/codebase/codegraph_client.rs:108` `find_symbols` over UDS; `Cargo.toml:262` `toml = "0.8"`.
- `src/api/skills.rs:61,180` expose only read-only `dispatch_skill`/`list_skills`; `src/server/mcp/tools_context.rs:99-119` only read-only tools — consistent with the CLI-only mutation decision.
- ADR-019 amendment is narrow (`docs/adr/ADR-034-skill-controller.md:19-25`): explicit in-process exception for one application-crate module; explicitly not storage-core, keeps the membership rule (`docs/adr/ADR-019-plugin-first-boundary.md:90-98`) and C1/C2/C3 (`:66-72`) intact.
- Simulation honesty: report and ADR numbers match exactly (`docs/adr/ADR-034-skill-controller.md:53,55,56` vs the supplied report: C .800/.01604/23.97, B .451/.5614/7.888, A .093/.8616/16.51). I independently reproduced C=0.800/B≈0.451/A≈0.093 using min-max normalization with the stated weights (0.35/0.25/0.2/0.2), so the composite is not inflated. ASSUMPTION inputs are disclosed (`:59-71`), trade-off stated (`:55`), and limits stated ("not proof of CLI compatibility", `:56`).
- No blocking pre-implementation gap: the two non-acceptance pre-conditions in `05-DECISIONS-AND-SCOPE.md:34-39` are already resolved in-branch — `AGENTS.md:92` now reads `skills/` + `XAVIER_SKILL_STORE`, and `docs/SRS/REQUIREMENTS.md:1125` now allows a configurable (non-Hermes) store (commit 680a5dd1). Remaining unknown (symlink discovery per CLI) is planned as adapter acceptance tests O01/O02 (`06-TASK-DAG.md:43-44`); the DAG gates implementation S01 on P03/P04/P07 only.

RISKS:
- The central mechanism still rests on an unverified ASSUMPTION (per-skill symlink discovery by Codex/Claude/OpenCode) — `docs/adr/ADR-034-skill-controller.md:67,114,107`. If O01/O02 fail, the store choice stands but the publication strategy is invalidated; acceptance should be read as conditional on that gate.
- Minor citation drift (not blocking): `skill_registry.rs:105` is a doc-comment; actual Hermes hardcoding is `:96-98,112`. `00-BRIEF.md:58` cited for "file-based/local-first" is the Projector bullet; the local-first statement is `:11` (`SWAL_GOAL.md:8` does support the vision). ADR §Supuestos is otherwise correct.
- `AGENTS.md:92` and `REQ-060` (`REQUIREMENTS.md:1125`, marked `implemented`) document/claim `XAVIER_SKILL_STORE` configurability, but no code reads it yet (grep: 0 hits in `src/`). This is a doc/ledger-ahead-of-code risk owned by `feat-skill-store-config` (planned), not by ADR-034; it should not be allowed to promote that feature to `stable` before its tests exist.
- The supplied report's second table ("weighted across scenarios") duplicates the composite values verbatim; harmless here, but it should not be cited as an independent cross-scenario computation.
- `release` appears in the CLI command list (ADR §4) only via `feat-skill-ephemeral` (E02); ensure its scope is pinned in that issue to avoid an unimplemented command surface.
