# Ledger auto-loop plan (2026-10-29)

Status: approved 2026-10-29 (owner decisions in section 8). Repository variable `LEDGER_PROMOTER_AUTO=true` is set; it takes effect when L1 lands.

Goal: the Ledger Promoter runs by itself. Nobody decides "run it for real" by hand. GitHub Actions evaluates the ledger after every main CI run and on a schedule, opens and merges a promotion PR when features have earned it, keeps one GitHub issue per feature gap, hands those issues to Jules, closes them when the feature is promoted, and tags stable milestones with a non-release tag. Only the promoter changes `status` and `progress_pct` in `.gitcore/features.json`; this plan does not change that rule.

## 1. What exists today (main at `6bf66f3f7`)

`.github/workflows/ledger-promote.yml` ("Ledger Promoter", merged in #3617):

| Item | Today |
|---|---|
| Triggers | `workflow_dispatch` only (inputs `dry_run`, default `true`; `apply_demotions`, default `false`). No `workflow_run`, no `schedule`. |
| Guard | job `evaluate` runs only if `vars.LEDGER_PROMOTER_ENABLED == 'true'` (set to `true` since 2026-10-16) and the ref is `main`. |
| evaluate | checks out main, runs `scripts/ledger-promote-evidence.py` (same-SHA evidence from push runs of `ci.yml` job "Full Ledger Verification" plus the `ledger-logs` artifact, and `e2e-segmented.yml`; history = last 3 first-parent commits), then `scripts/ledger-promote.py --dry-run`; uploads `ledger-promoter-evidence`. |
| propose | only when `inputs.dry_run == false`: `--apply` (plus `--apply-demotions` only if asked), refuses a stale snapshot, pushes `ledger/promote-<sha>`, opens "Promote verified ledger features", then dispatches `ci.yml` and `fast-gate.yml` on the branch (GITHUB_TOKEN events do not start workflows by themselves). No auto-merge, no docs sync, no issues, no tags. |
| Promotion rules | `beta -> stable` needs progress 100, no pending steps/honesty markers/open_gaps, declared tests passing in gating evidence, linked integration/E2E evidence, and a green streak >= 3 (`--streak`). `planned/in_progress -> beta` needs the same evidence and streak but allows incompleteness (except `open_gaps`). |

Repository facts that shape the design:

- Ruleset `r1` on the default branch: required checks "CI Gate" and "Shadow Fast Gate", PR required with 0 approvals, admin role bypass. Repo `allow_auto_merge` is on. Actions may create and approve PRs.
- A commit pushed or merged with `GITHUB_TOKEN` does not trigger `push` workflows. A ledger-only merge made by the bot therefore has no push CI run, and the evidence collector (push runs only) would treat it as a gap and reset the streak right after every promotion.
- `release.yml` fires on tags `v*`. Tags created with `GITHUB_TOKEN` do not trigger workflows either, but the loop must still never create a `v*` tag.
- The `jules` label already starts a Jules task through the Jules GitHub app (issues #2997-#2999 were picked up by `google-labs-jules`). That path runs on one Jules account and bypasses our 15-per-account capacity scheduler and task-body rules.
- `jules-ci-feedback.yml` already files `tbd-incident` issues for red main runs (marker `<!-- tbd-incident sha=... workflow=... -->`). The loop reuses that dedupe pattern.

## 2. Loop overview

```
main push -> Xavier CI / CD + E2E Segmented complete
        |                                   (also: cron every 6 h, manual dispatch)
        v
Ledger Promoter: evaluate (always dry) ---- report.json artifact
        |                       \
        | eligible > 0 and       \--> Ledger Gaps workflow (workflow_run on Ledger Promoter)
        | LEDGER_PROMOTER_AUTO        create/update/close issues `ledger-gap:<feature-id>`
        v                             labels `ledger-gap` + `jules` (non-security only, max 5/run)
propose: --apply, docs sync,                |
branch ledger/promote-<sha>, PR,            v
dispatch CI Gate + Shadow, auto-merge   Jules GitHub app starts a task from the `jules` label
        |                               -> Jules PR -> Codex judge -> merge -> next CI run
        v
main gets the ledger commit
        |
        v
Ledger Tag workflow: non-v tag `ledger-stable-<n>` + dispatch CI on main so the
ledger commit gets evidence and the streak continues
```

## 3. Triggers (task L1)

`ledger-promote.yml` gains:

- `workflow_run` on `["Xavier CI / CD", "E2E Segmented"]`, `types: [completed]`, filtered in the job `if` to `head_branch == 'main'`, `event == 'push'` (or `workflow_dispatch` on main, see L4), and `head_sha` equal to the current main HEAD (otherwise skip: a newer commit will produce its own run).
- `schedule: cron: '17 */6 * * *'` as a safety net when CI runs are cancelled or superseded.
- `workflow_dispatch` keeps its inputs. Manual runs stay dry by default.
- `concurrency: ledger-promoter`, `cancel-in-progress: false` (unchanged).

Mode resolution, in one step that writes `mode` to the job outputs:

| Event | `LEDGER_PROMOTER_AUTO` | Mode |
|---|---|---|
| `workflow_dispatch` | any | `inputs.dry_run` (default dry) |
| `workflow_run` / `schedule` | `true` | apply, demotions OFF (hard-coded; `--apply-demotions` is only reachable from a manual dispatch) |
| `workflow_run` / `schedule` | unset or other | dry (evaluate + gaps only) |

`evaluate` publishes `eligible_count` (features with `eligible: true`) and `demotion_count` (features with `review_flagged: true`) from `report.json`. `propose` runs when mode is apply and either `eligible_count > 0`, or the run is a manual `workflow_dispatch` with `apply_demotions` and `demotion_count > 0` (a manual demotion-only run must still work). With the streak at 3 this means: nothing happens until main has three consecutive green full runs, then the next completion triggers the promotion by itself.

## 4. Promotion PR, docs sync and auto-merge (tasks L1 + L2)

- After `--apply`, `scripts/ledger-docs-sync.py` (L2) regenerates the generated blocks between `<!-- ledger:summary:start -->` / `<!-- ledger:summary:end -->` markers in `docs/FEATURE_STATUS.md` (counts by status, stable %, per-category table) from `.gitcore/features.json`. It never edits the ledger and never touches text outside the markers. `--check` exits 1 when a block is stale (used by a test, optional CI step later).
- The PR (branch `ledger/promote-<sha>`) contains only `.gitcore/features.json` and the synced docs. Body = promoter Markdown report + list of promoted features + `Closes #N` for each open `ledger-gap:<feature-id>` issue of a promoted feature.
- The workflow dispatches `ci.yml` and `fast-gate.yml` on the branch (existing), then runs `gh pr merge --auto --squash --match-head-commit <sha>`. Ruleset `r1` still enforces "CI Gate" and "Shadow Fast Gate"; auto-merge only merges after both are green. No branch-protection change is needed.
- Guard: the propose step refuses to continue if the diff touches anything outside `.gitcore/features.json` and `docs/FEATURE_STATUS.md`, or if any feature moved to a status other than the evaluated transitions (no demotions in auto mode).
- `gh pr merge --auto` only arms auto-merge and returns while checks are pending, so a finished Promoter run does NOT mean its PR merged. Nothing downstream (tags, CI dispatch on main) may assume it did; section 7 detects the merge from main itself.

## 5. Gap issues (task L3)

New workflow `ledger-gaps.yml` (`workflow_run` on "Ledger Promoter" completed, plus `workflow_dispatch`), permissions `issues: write`, `actions: read`, `contents: read`. It downloads the `ledger-promoter-evidence` artifact of that run and calls `scripts/ledger-gap-issues.py`:

- Candidates: features with status `beta`, `in_progress` or `planned` whose report reasons are about tests: `declared tests failed`, `declared test missing from index`, `no linked integration/E2E evidence`, `linked test is BUG-ignored`, `declared test evidence differs from ledger`. A reason that is only `green streak N < 3` is not a gap (it is time, not work).
- One issue per feature, found by the body marker `<!-- ledger-gap:<feature-id> -->` and label `ledger-gap`. Create when missing, update the body (reasons, failing tests, last evaluated SHA, run link) when present, close with a comment when the feature is `stable` on main or no longer has a test gap.
- Title: `ledger gap: <feature-id> — <short reason>`. Body: feature name, `implemented_in`, declared `tests`, reasons, and the task rules Jules needs (test registration in `tests/suite_NN.rs`, `isolate_test_process!`, append-only ledger `tests`, never edit status/progress).
- Labels: `ledger-gap` plus `jules` (Jules' native GitHub integration starts a task when the label is applied). Security features (`category == "security"`, 11 today) get NO public issue and no label: the script only lists them in the job summary as "needs owner". An existing issue labeled `security` or `security-report` is never edited, relabeled or closed by the script. Security work never goes to Jules.
- Rate limit: at most 5 `jules` label applications per run (`--max-jules`, default 5), counting new issues and reopened issues that get the label again, oldest-first by feature id. The Jules app runs on one account and ignores our 15-session limit, so this cap is what protects it. Gaps beyond the cap wait for the next run. Body updates and closes are not limited and never re-apply the label to an issue that already has it.
- Red main: if the evaluated run is red, the script updates existing issues but creates none (the `tbd-incident` issue already covers the red run).

## 6. Jules pickup

Decision (2026-10-29): the GitHub `jules` label, i.e. Jules' native GitHub integration (already used for #2997-#2999, answered by `google-labs-jules`). No Jules secret in the repo.

- The Ledger Gaps workflow applies `jules` when it creates (or reopens) a gap issue, within the per-run cap of section 5. Jules comments on the issue and opens a PR.
- `jules-wave.py triage` on the PC does NOT pull `jules`-labeled issues (it skips any issue carrying `jules`), so a gap is never dispatched twice. Its `jules-queue` pull stays as an optional manual path (both accounts, capacity-aware) for issues a human labels `jules-queue` without `jules`.
- A Jules API path from Actions (`JULES_API_KEY` repo secret) is not used.

Jules PRs go through the normal lane: Codex judges, merge on PASS plus green CI Gate and Shadow Fast Gate.

## 7. Tags (task L4)

New workflow `ledger-tag.yml`. A `GITHUB_TOKEN` auto-merge fires no `push` event, and the Promoter run ends before its PR merges, so the workflow is an idempotent reconciler driven by the state of `main`, not by the Promoter run: triggers `push` to `main` (paths `.gitcore/features.json`, covers human merges), `schedule` `'47 * * * *'` (hourly; picks up bot merges within an hour), `workflow_run` on "Ledger Promoter" (cheap early check, usually a no-op) and `workflow_dispatch`. Each run:

- Confirms the merge from main: reads `.gitcore/features.json` at `origin/main` HEAD and the last commit that touched it (`git log -1 -- .gitcore/features.json`). Tags only reflect what is on main; an open or unmerged promotion PR changes nothing.

- When the stable count on main is higher than on the newest existing `ledger-stable-*` tag, create an annotated tag `ledger-stable-<stable>` (e.g. `ledger-stable-74`) on that commit. Message: stable/total, stable %, promoted feature ids, promoter run URL.
- When stable % crosses a multiple of 10 (60, 70, ...), also tag `ledger-milestone-<pct>` (e.g. `ledger-milestone-70`).
- Hard guard in the script: refuse any tag name matching `^v`. The loop never creates release tags or GitHub releases. When all core features (core 1.0 definition in `CORE-REFACTOR-2026-10-29.md`) are stable, it opens one issue "Release candidate: core 1.0 ready" for the owner; any `v*` tag stays manual.
- CI after a bot merge: when main HEAD was authored by `github-actions[bot]`, touches only the ledger files, and has no `Xavier CI / CD` or `E2E Segmented` run for its SHA yet (any event), the workflow dispatches `ci.yml` and `e2e-segmented.yml` on `main`; the existing-run check makes repeated hourly runs a no-op. Also `scripts/ledger-promote-evidence.py` accepts `workflow_dispatch` runs on `main` with the same `head_sha` as evidence (today it reads `event=push` only). Without this, every bot-merged ledger commit breaks the 3-run streak.

## 8. Owner decisions (2026-10-29)

1. Auto mode: yes. Repository variable `LEDGER_PROMOTER_AUTO=true` is set. Demotions stay manual-only (`workflow_dispatch` with `apply_demotions`).
2. Auto-merge: yes, with the default `GITHUB_TOKEN`; ruleset `r1` unchanged (CI Gate and Shadow Fast Gate still required). L4's dispatch on main keeps the streak after bot merges.
3. Jules intake: the `jules` label (section 6), max 5 labeled issues per run, strict security exclusion.
4. Tags: `ledger-stable-<n>` and `ledger-milestone-<pct>` (no `v` prefix).

## 9. Tasks (small PRs)

| Id | PR | Files | Depends on |
|---|---|---|---|
| L1 | ci(ledger): run the promoter on main CI completion and on a schedule | `.github/workflows/ledger-promote.yml`, `scripts/tests/test_ledger_promote_workflow.py` | none |
| L2 | feat(ledger): sync generated docs blocks from the ledger | `scripts/ledger-docs-sync.py`, `scripts/tests/test_ledger_docs_sync.py`, `docs/FEATURE_STATUS.md` (markers) | none |
| L3 | feat(ledger): keep one issue per ledger gap | `scripts/ledger-gap-issues.py`, `scripts/tests/test_ledger_gap_issues.py`, `.github/workflows/ledger-gaps.yml` | none (reads the existing report artifact) |
| L4 | feat(ledger): tag stable milestones and keep the streak after bot merges | `.github/workflows/ledger-tag.yml`, `scripts/ledger-tag.py`, `scripts/tests/test_ledger_tag.py`, `scripts/ledger-promote-evidence.py` | none |

L1 calls L2's script only if the file exists, so the four PRs can land in any order.
