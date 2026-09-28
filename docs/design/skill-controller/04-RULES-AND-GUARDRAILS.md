# Skill Controller — Rules & Guardrails

Status: design phase. Companion to `00-BRIEF.md`.
Audience: the orchestrator that writes implementation issues, and every agent that executes one.

## How to use this document

Two artifacts below, both copy-paste ready:

1. **§1 Rule block** (58 lines) — pasted verbatim into every implementation issue of this
   initiative, right after the issue's *Desired State*.
2. **§2 Definition of Done** — pasted as the issue's final section and as the PR checklist.

Both are derived from `AGENTS.md` (repo contract) and from existing in-tree patterns, so they
are checkable by grep rather than by opinion. §3 records where each rule comes from; §4 covers
the two cases that always escalate (new dependency, ADR-contradicting change).

---

## 1. Rule block (paste into every implementation issue)

````markdown
## MANDATORY RULES — Xavier Skill Controller

**Scope (hard limits)**
- Touch only the files in this issue's "Files to Modify" table: max 4 files, max 400 changed lines. No drive-by refactors, renames, or reformatting.
- No new crate in `Cargo.toml`/`Cargo.lock` unless the Library Table (this design set) lists it with license + reuse rationale **and** this issue says so.
- No new daemon, systemd unit, port, thread pool, FUSE/VFS, or plugin system — host the work in the existing process. No new trait/generic layer until a second in-tree implementation exists.
- No speculative config: add a `std::env::var` only if a caller sets it, and document the key in `.env.example` in the same change.
- 1 issue = 1 PR = 1 feature or a bounded part, citing its feature id. Never commit `.xavier/`, `data/`, `*.db`, `*.sqlite*`, logs, `.env`, or session state.

**Rust — forbidden in this repo**
- No `unwrap()`/`expect()` in non-test code; prod paths in scope have zero (`#[cfg(test)]` at `src/context/skill_registry.rs:776` and `src/context/skill_dispatcher.rs:242`, first `unwrap` 16/18 lines later). Use `?` + `thiserror` in libs, `anyhow` in binaries.
- No blocking `std::fs` IO and no Rayon `.par_iter()` on a Tokio worker — wrap in `tokio::task::spawn_blocking` (`src/memory/agent_scanner.rs:141`, `src/codebase/db.rs:398`).
- Bound every walk/recursion over user data by depth, node count, or bytes (`src/memory/graph_traversal.rs:131,221`).
- No `panic!`/`unreachable!`/`todo!`/abort reachable from MCP, HTTP, or CLI dispatch: return `Err` and render an error result (`src/server/mcp/tools_context.rs:515`, `src/server/mcp/server.rs:128`).
- Never build a `Command` from untrusted text (skill body, memory, manifest): argv from an allowlist, no shell.

**Filesystem safety (non-negotiable)**
- Dry-run is the default: every mutating fn takes `dry_run` and reports the plan without applying it; only an explicit apply flag writes.
- Canonicalize, then assert the resolved path is inside an allowlisted root, before every read/write/link/delete (roots listed explicitly, like `src/context/skill_registry.rs:106-115`).
- Never follow a symlink resolving outside the allowlist or to a missing target; do not regress the cycle-safe walk at `src/context/skill_registry.rs:157-169`.
- Never delete a regular file Xavier did not create. Unlink only Xavier-created symlinks (owned marker) under an allowlisted root. No `rm -rf`, no `remove_dir_all` on unowned paths, no glob deletion.
- Atomic writes only: `<path>.tmp` in the same directory, then `rename` (`src/memory/cloud_sync.rs:75-76`, `src/tgd/mod.rs:266-269`). Never write in place.
- Back up before overwriting anything unowned: `<path>.bak-<ts>` first (`src/maloca/store.rs:511-513`).
- Foreign files are untrusted input: size cap, parse failure = skip not crash, fail-open on unreadable config (`src/context/skill_registry.rs:119-147`).

**Security**
- Secrets never reach logs, reports, telemetry, error strings, memory bodies, or generated skills; mask or `[REDACTED]` (`src/nodes/audit.rs:14-20`, `src/settings/types.rs:295-301`, `src/security/redaction.rs:229`). Secret-scan findings report location + kind only, never the value.
- Skill and memory text is untrusted data: fence it (`src/context/skill_registry.rs:72`) and never let it steer the controller's own decisions.
- 12-factor: env config only, no hardcoded credentials or paths. This repo is public — write `$HOME/...`, never a real user path or name.

**Tests**
- One unit test per behavior in `#[cfg(test)] mod tests` + one integration test per user-visible behavior; `#[tokio::test]` for async.
- No test weakening: no deleted `assert!`, no loosened thresholds, no `#[ignore]` without an issue link, no blind snapshot update.
- New env var ⇒ `.env.example` in the same change; new SQL ⇒ migration + round-trip test.

**Gate — all green before the PR (paste the output in the PR body)**
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo check -p xavier --all-targets
cargo test -p xavier --lib <module_path>     # the tests you added
bash scripts/check-secrets.sh
bash scripts/verify-pipeline.sh              # ledger is the judge; never hand-promote status
```
- A failing gate means fix or abandon — never commit broken code. Never edit `.gitcore/features.json` (reconciled at wave end by the orchestrator). Missing `openssl`/`pkg-config` ⇒ use `nix-shell`; never vendor or patch third-party code.
````

---

## 2. Definition-of-Done template (paste as the last issue section / PR checklist)

````markdown
## Definition of Done

**Scope**
- [ ] `git diff --stat HEAD | tail -1` shows <= 4 files and <= 400 changed lines.
- [ ] Every changed file is in this issue's "Files to Modify" table; nothing else was touched.
- [ ] `git diff HEAD -- Cargo.toml Cargo.lock` is empty, or the new crate is in the Library Table.
- [ ] `.gitcore/features.json` is untouched.

**Behavior**
- [ ] The issue's user-visible behavior has an integration test that fails without the change.
- [ ] Every error path returns `Err`; no `panic!`/`unwrap()`/`expect()` in non-test code:
      `grep -nE 'unwrap\(\)|expect\(|panic!|todo!|unreachable!' <file> | awk -F: '$1<' <testmodline>` is empty.

**Filesystem safety**
- [ ] Dry-run is the default and returns a plan without touching disk; a test asserts this.
- [ ] The apply path is covered by a test that runs against a tempdir only.
- [ ] Every mutated path is canonicalized and asserted inside the allowlist before the operation.
- [ ] Writes go through `<path>.tmp` + `rename`; overwrite of an unowned file creates `.bak-<ts>`.
- [ ] No deletion targets a regular file not created by Xavier (proven by test or by dry-run log).

**Security**
- [ ] `bash scripts/check-secrets.sh` is clean, including any generated skill/report content.
- [ ] No secret value appears in a log line, error string, or test fixture
      (`scripts/check-secrets.sh` gitleaks pass + `grep -rniE 'sk-[A-Za-z0-9]{16,}|BEGIN [A-Z ]*PRIVATE KEY' <changed files>` empty).
- [ ] Untrusted skill/memory content is fenced before it reaches a prompt or a report.

**Gates (all green, output pasted in the PR)**
- [ ] `cargo fmt --check` — 0 diff.
- [ ] `cargo clippy --all-targets -- -D warnings 2>&1 | grep -c '^error'` == 0.
- [ ] `cargo check -p xavier --all-targets 2>&1 | grep -c '^error'` == 0.
- [ ] `cargo test -p xavier --lib <module_path>` — N passed, 0 failed.
- [ ] `bash scripts/verify-pipeline.sh` — no regression in declared features.
- [ ] PR title/body cite the feature id; PR contains >= 1 real source file (`git show HEAD --name-only`).
````

---

## 3. Provenance of each rule

| Rule group | Source of truth |
|---|---|
| Scope, 1 PR = 1 feature, no committing DBs/logs | `AGENTS.md:82`, `AGENTS.md:84`, `AGENTS.md:93` |
| Dependency / config / `.env.example` | `AGENTS.md:65-70`; precedent `src/bin/backfill_embeddings.rs:3` (`anyhow` in a bin) vs `src/crypto/keys.rs` (`thiserror` in a lib) |
| Zero `unwrap` in prod code | `#[cfg(test)]` at `src/context/skill_registry.rs:776`, first `unwrap` at `:792`; `src/context/skill_dispatcher.rs:242` / `:260` |
| Tokio + Rayon rule | `AGENTS.md:76-77`; compliant uses `src/memory/agent_scanner.rs:141`, `src/codebase/db.rs:398` |
| Bounded traversal | `src/memory/graph_traversal.rs:131` (param) and `:221` (guard) |
| No panic at MCP/HTTP boundary | `src/server/mcp/tools_context.rs:515`; `mcp_text_result` at `src/server/mcp/server.rs:128`; existing panic hook `src/server/http/mod.rs:83-84` is logging-only |
| Atomic write | `src/tgd/mod.rs:266-269` (comment says so), `src/memory/cloud_sync.rs:75-76` |
| Backup / quarantine | `src/maloca/store.rs:511-513` |
| Symlink cycle safety + dev/ino identity | `src/context/skill_registry.rs:157-169`, test at `:855-871` |
| Explicit allowlist of skill roots | `src/context/skill_registry.rs:106-115` |
| Untrusted fencing | `src/context/skill_registry.rs:72` |
| Secret masking | `src/nodes/audit.rs:14-20`, `src/settings/types.rs:295-301`, engine `src/security/redaction.rs:229` |
| Ledger promoted only by green run | `AGENTS.md:47-48`, `AGENTS.md:53`; `scripts/verify-pipeline.sh:23` reads `.gitcore/features.json` |
| Secret scan gate | `scripts/check-secrets.sh:15-22` (gitleaks + grep fallback, `.gitleaks.toml`) |
| Verification commands | `AGENTS.md:33-35`, `AGENTS.md:74` |
| Issue shape (Current State / Desired State / AC / DO NOT touch / Anti-Hallucination) | `.gitcore/waves/wave-26/issue-01.md:8-101` |

Notes on two rules that are policy, not precedent:

- **400 lines / 4 files** is a number picked for this initiative to keep islands disjoint
  (the parallel-PR convention in `.gitcore/waves/wave-26/issue-01.md:74`). It is a cap, not a
  target: if a behavior genuinely needs more, split the issue rather than raising the cap.
- **"trait only with 2+ implementations"** is the smallest design that meets the goal
  (`00-BRIEF.md:86-87`); it exists because the initiative adds a projector, an ephemeral composer,
  and a drift auditor, and the tempting failure mode is abstracting all three at once.

## 4. Escalation (not decided inside an issue)

| Situation | Required action before code |
|---|---|
| A new crate is genuinely necessary | Add a Library Table row: crate, version, license, why no in-tree or already-vendored alternative, maintenance risk. Explicit approval in the issue. Then it is allowed. |
| A change contradicts an existing ADR | Update the ADR or open a new one in `docs/adr/` (`AGENTS.md:59-61`). Do not smuggle the decision into code. |
| A new ledger feature | Add the spec in `docs/features/specs/FEATURE-*.md` and the `status: planned` entry (`AGENTS.md:54-55`). Never hand-set a higher status. |
| A rule must be broken | Stop. Write the deviation, its reason, and its blast radius into the issue. Approval is a human decision; the agent does not self-grant. |
