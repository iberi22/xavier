# Handoff prompt — next agent (post Ola 10/11)

Copy everything below the line into a new agent session.

---

You are continuing work on **Xavier** (`iberi22/xavier`).

## Preflight (mandatory)

1. `git fetch origin main && git checkout main && git pull --ff-only origin main`
2. Confirm clean tree: `git status -sb` → on `main`, no open ship PRs
3. Read:
   - `.gitcore/issues/SESSION-CLOSEOUT-2026-07-31.md`
   - `docs/devlog/2026-07-31-ola10-ship-close.md`
   - `docs/devlog/2026-07-31-ola11-harden-close.md`
   - `AGENTS.md` + Xavier memory protocol (Fat Search → Page-In → Persist)
4. Open issues should be **only** Mesh EPIC [#115](https://github.com/iberi22/xavier/issues/115) unless new work was filed

## Context already done (do not redo)

- **Ola 10** (#1098): stabilize ship — Jules PRs merged; notifications/auth/MCP explore+trace/code find filters/dead adapters/headless 501 then upgraded in Ola 11
- **Ola 11** (#1128, PR #1142): orchestrator implemented headless real `code_*`, hermetic `--lib` fixes, panel HTML stub, web onboarding without mandatory Tauri invoke, WiX without `xavier-gui`, ops docs for NixOS Docker + local CI
- **Ola 8** (#980–#993): closed not planned (unmerged Jules PRs); do not revive unless Belal asks

## Your mission options (pick with Belal; default = A)

### A — Mesh progress (default if no other ask)

- Scope: EPIC [#115](https://github.com/iberi22/xavier/issues/115) only
- First: Fat Search Xavier for mesh/Data Commons decisions; read EPIC body + `docs/SWAL/` goal docs
- Propose a **small disjoint Jules/orchestrator wave** (≤8 issues, file islands, no `features.json` until close) OR implement one concrete vertical slice if Belal wants agent-owned work
- Do **not** expand into Ola 8 memory features in the same wave

### B — Verification / CI hardening

- Run host: `CARGO_TARGET_DIR=target_local cargo test -p xavier --lib` and triage any new fails
- If Docker needed on NixOS: follow `docs/ops/nixos-docker.md` + `docs/ops/local-ci-with-agent-priv.md` (agent-priv Accept dialog; NoNewPrivs)
- Prefer fixing real regressions over weakening tests

### C — Packaging / panel assets

- Ensure `panel-ui` build path documented and smoke `/panel` with built assets
- Windows packaging truth already in FEATURE_STATUS; only change if evidence contradicts

## Guardrails

- Max **20 files** / **3 concerns** per commit (`.git-atomize.yml` / repo policy)
- No `TODO`/`FIXME`/`HACK` in added lines (pre-commit)
- No secrets in commits; use `.env` / config placeholders
- GHA may be budget-exhausted → local verify + `--admin` merge only after diff review (same as Ola 10/11)
- Create Jules issues **without** `jules` label first; apply label only after island harness

## Success criteria for your session

- `main` stays green locally for the surface you touch
- Any new wave has EPIC + ownership map under `.gitcore/issues/`
- Persist durable decisions to Xavier memory after completion
- Leave a short closeout note if you open a new ola folder

## First message to Belal

Ask which track (A/B/C) and whether work should be **orchestrator-implemented** or **Jules-dispatched**.
