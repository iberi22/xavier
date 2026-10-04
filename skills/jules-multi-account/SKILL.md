---
name: jules-multi-account
title: Jules Multi-Account REST Orchestration Protocol
description: Canonical protocol for high-throughput, multi-account Jules dispatch (JULES_API_KEY + JULES_API_KEY_2) with deterministic REST API routing, fail-secure PR verification, and disjoint file islands.
version: 2.0.0
author: Hermes + BELA + Antigravity
tags:
  - jules
  - multi-account
  - rest-api
  - wave-orchestration
  - gitcore
  - swal
category: orchestration
---

# Jules Multi-Account REST Orchestration Protocol (v2.0.0)

> **MANDATORY FOR ALL SWAL AGENTS (Hermes, Jules, Antigravity, Claude, Codex)**
> Use this protocol whenever dispatching or orchestrating tasks across Google Labs Jules accounts to achieve maximum parallel throughput without triggering rate limits or context collisions.

---

## 1. Credentials & Key Discovery

Keys reside **strictly** in `~/.hermes/.env` (mode 600) or environment variables. **NEVER commit keys or log full values.**

| Key Alias | Environment Variable | Role | Quirk / Behavior |
|-----------|----------------------|------|------------------|
| **Cuenta A** | `JULES_API_KEY` | Frontend, UI, a11y, Realtime Hooks | Standard account. Supports `/sources` and `/sessions` list. |
| **Cuenta B** | `JULES_API_KEY_2` | Backend, Hardening, CLI, MCP, E2E | Quirk: `/sessions` list can timeout/empty. Must query sessions via direct `GET /sessions/{id}`. |

### Safe Key Discovery Pattern:
```bash
for n in JULES_API_KEY JULES_API_KEY_2; do
  v="${!n}"; [ -n "$v" ] && echo "$n prefijo=${v:0:14}... len=${#v}"
done
```

---

## 2. Why REST API over CLI / GitHub Labels?

1. **CLI (`jules new`)**: Uses a single local OAuth token; completely ignores secondary API keys in the environment.
2. **GitHub Labels (`gh issue edit --add-label jules`)**: Non-deterministic routing. You cannot control which account or queue takes the task.
3. **Deterministic REST API**: The only verified method for multi-account execution. One API key = One isolated Google Labs VM = 100% predictable workload distribution.

---

## 3. Canonical Session Creation Payload

```http
POST https://jules.googleapis.com/v1alpha/sessions
X-Goog-Api-Key: <KEY>
Content-Type: application/json
```

```json
{
  "title": "[WAVE-27.XX] feat-kebab-case — Human readable summary",
  "prompt": "Resolve GitHub Issue #<NUM> in repository <owner>/<repo>.\n\nSPECIFICATION:\n<FULL_CANONICAL_ISSUE_BODY>",
  "sourceContext": {
    "source": "sources/github/<owner>/<repo>",
    "githubRepoContext": { "startingBranch": "main" }
  },
  "automationMode": "AUTO_CREATE_PR"
}
```

> ⚠️ **CRITICAL RULES**:
> 1. `githubRepoContext.startingBranch`: **Mandatory**. Omitting it returns `400 INVALID_ARGUMENT`.
> 2. `automationMode`: Must be `"AUTO_CREATE_PR"`.
> 3. Response: Contains `{"name": "sessions/<id>", "url": "https://jules.google.com/session/<id>"}`. The agent must comment on the corresponding GitHub issue linking this URL.

---

## 4. Multi-Account Dispatch Tool (`jules-multi-dispatch.py`)

A centralized, hardened CLI tool lives in `~/.hermes/scripts/jules-multi-dispatch.py`:

```bash
# Dispatch batch to Cuenta A (Frontend):
python3 ~/.hermes/scripts/jules-multi-dispatch.py \
  --repo iberi22/xavier --account A --start 2379 --end 2393 \
  --wave-dir .gitcore/waves/wave-27 --offset-file 0

# Dispatch batch to Cuenta B (Backend):
python3 ~/.hermes/scripts/jules-multi-dispatch.py \
  --repo iberi22/xavier --account B --start 2394 --end 2408 \
  --wave-dir .gitcore/waves/wave-27 --offset-file 15
```

---

## 5. Automated Interaction & Unblock Daemon (`jules-auto-unblock.py`)

When Jules agents pause or enter `AWAITING_USER_FEEDBACK`:
1. **Never wait for human interaction**: Agents must not stay blocked waiting for a human. Use the centralized daemon `~/.hermes/scripts/jules-auto-unblock.py`.
2. **NEVER send '?' or ambiguous one-character messages**:
   > ⚠️ **CRITICAL LESSON (Session 3320258804012123236 Post-Mortem)**:
   > Sending `"?"` or generic probes after a pause or build failure causes the Jules Google VM container to restart from scratch or time out, throwing `"Jules encountered an error when preparing the virtual machine environment for the task"`.
   > Always send **declarative, actionable instructions** directing Jules to proceed immediately to code editing, run tests, and open the PR.
3. **Automated Feedback Format**:
```http
POST https://jules.googleapis.com/v1alpha/sessions/{id}:sendMessage
X-Goog-Api-Key: <KEY>
Content-Type: application/json

{
  "prompt": "Direct concrete feedback: proceed with the implementation in the designated target file, follow existing patterns, run tests, and create the PR now."
}
```
> ⚠️ **CRITICAL**: The field name **MUST be `"prompt"`** (`"message"` or `"content"` returns `400 INVALID_ARGUMENT`).
4. **Daemon usage**:
```bash
# Sweep all active sessions across Cuenta A and B and auto-unblock:
python3 ~/.hermes/scripts/jules-auto-unblock.py

# Continuous watchdog:
python3 ~/.hermes/scripts/jules-auto-unblock.py --watch --interval 30
```

---

## 6. Micro-Tasking Architecture & Internal Issue Phases (Anti-Failure Strategy)

To maximize completion rates and prevent agent timeouts or VM container reinitialization:
1. **Micro-Scopes**: Do NOT overload single issues with multiple modules. Each issue must target **ONE isolated file**.
2. **Mandatory 3-Phase Internal Workflow**:
   Every issue body must prescribe an explicit 3-step internal execution plan for Jules:
   - **Phase 1: Contracts & Types**: Define data structures (`struct`, `enum`), error types (`thiserror`), and module exports. Keep changes minimal.
   - **Phase 2: Core Logic & Storage**: Implement methods, state mutations, and pure business logic adhering to existing patterns.
   - **Phase 3: Isolated Tests & PR Delivery**: Add isolated unit tests (`#[cfg(test)]`), run `cargo check` / `cargo test`, and immediately create the PR.
3. **Web Research on Blockers**:
   - If Jules encounters an unexpected compilation error or API mismatch, the issue prompt mandates searching the web for the exact error signature before pausing or requesting clarification.
   - Any blocker resolution findings must be summarized directly in the Pull Request body.
4. **Self-Contained Imports**: Ensure that any dependencies required by the issue already exist in `Cargo.toml` or `package.json` in `main`.

---

## 7. Disjoint File Island Guarantee (Anti-Collision)

When running concurrent sessions across two accounts on the **SAME repository**:
1. **Zero Intersection**: No two issues may touch the same file path.
2. **Pre-requisites in Main**: All shared types/primitives must already be merged into `main` before dispatching dependents.
3. **No Features Reconcile**: Subagents must NEVER touch `.gitcore/features.json`. Only the orchestrator reconciles the ledger post-merge.


## Field report 2026-09-29 (measured on 14 real sessions, both accounts)

| Repo | Sessions | Result | Typical time |
|---|---|---|---|
| atlas-saas (JS/TS, Cloudflare) | 3 | 3/3 COMPLETED with PR | 12–14 min |
| swal-agent-runner (TS) | 2 | 2/2 COMPLETED with PR | ~20 min |
| swal-vault (Flutter) | 2 | 1 COMPLETED with PR, 1 AWAITING_USER_FEEDBACK | 77–91 min |
| iberi22/xavier (large Rust workspace) | 7 | 0/7 — "error when preparing the virtual machine environment" / "encountered an error" | — |

Rules derived from the data:
1. **Do not send iberi22/xavier to Jules** until its Jules environment is fixed (Jules web → repo settings: no heavy setup script). The runner enforces this with `jules_blocked_repos` in `~/.hermes/scripts/pipeline.toml`; those tasks run on the local free agent and CI verifies (xavier is public, CI works).
2. **Jules is strong on small JS/TS/Flutter tasks**: ≤3 files, ≤150 changed lines, one clear acceptance list, exact file table. That profile succeeded 6/7.
3. **Never tell Jules to run heavy builds** it cannot run; per-wave `jules_rules` in the wave file override the default rules (e.g. "do not build, CI verifies").
4. **A FAILED session usually still contains the code** (`artifacts[].changeSet.gitPatch`): the runner salvages the last patch and runs the gates instead of redoing the task.
5. **Stop or replace a wave → archive its Jules sessions** (`POST sessions/{id}:archive`), or they sit in AWAITING_USER_FEEDBACK holding slots and can open duplicate PRs.
6. **Scheduled persona agents with open-ended prompts** (Bolt/Sentinel/Palette) wait forever for feedback; give them a concrete target and "finish without asking" or disable them in the Jules web UI.
7. The hook that runs clippy on every commit killed Jules' internal commit; xavier's pre-commit is now light (PR #2718, `XAVIER_HOOK_FULL=1` to run clippy).
8. Private repos: GitHub Actions is blocked by a billing issue since 2026-08-18, so CI-verify mode does not work there; use local gates or `act` (loop v5, VERIFY_MODE=act).
