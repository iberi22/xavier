# Measured facts (2026-09-27) — Xavier CI / git policy
- Branch protection on `main`: NONE classic protection. Two active rulesets (r1, ruleone) on ~DEFAULT_BRANCH only block deletion and non-fast-forward (force push). No required status checks, no required reviews, no bypass actors. => direct pushes to main are already technically possible with zero gates.
- Workflows: ci.yml (push, pull_request, workflow_dispatch; jobs: changes, rust-fmt, rust-clippy, rust-msrv, rust-test, rust-integration, secret-scan, frontend-build-audit, version-gate), pr-gate.yml, jules-ci-feedback.yml, coverage.yml, rust-coverage.yml, release.yml, deploy-docs.yml.
- ci.yml last 30 runs: 30/30 success (19 PR, 11 push). Wall time p50 9.4 min, p90 11.1, max 11.8.
- Job medians on runs where Rust ran (n=10): rust-test 9.7 min, rust-integration 6.0 (observability_contract), rust-clippy 3.2, frontend-build-audit 2.8, rust-msrv 1.7, rust-fmt 0.3, secret-scan 0.2, version-gate 0.1, changes 0.1. No flaky signal in the sample.
- Local hooks: Husky 9 (`.husky/pre-commit`, `.husky/pre-push`). pre-commit runs strategy-leak check, cargo fmt --check, cargo clippy -p xavier -p code-graph --all-targets -D warnings (package-scoped to xavier and code-graph, all targets; not staged-scoped), secret scan, docs/RAG step. AGENTS.md §10 mandates Husky 9 and forbids other hook dirs.
- Repo rules (AGENTS.md): waves, ledger promoted only by green scripts/verify-pipeline.sh, ADR required for non-obvious decisions and for changes contradicting an ADR, "1 PR = 1 feature", CI runs fmt/clippy/tests/feature verification/secret scan on every PR.
- Contributors are mostly AUTONOMOUS AGENTS: Google Jules (opens PRs, cannot see CI), CLI agents (codex/agy/opencode) orchestrated by Claude Code which reviews diffs before committing, plus one human owner. Observed today: agents sometimes write outside their assigned scope (agy edited a second checkout), leak personal paths, or claim tests pass without running them.
- Public GitHub repo; release via release.yml; deploy is a local systemd user unit + ~/.local/bin binary (not continuous deploy).

## Evidence (reproducible, captured 2026-09-27)

- Rulesets: `gh api repos/iberi22/xavier/rulesets` then `gh api repos/iberi22/xavier/rulesets/<id>`; captured result:

```json
{"bypass": [], "conditions": ["~DEFAULT_BRANCH"], "enforcement": "active", "name": "r1", "rules": [{"p": {"required_approving_review_count": null, "required_status_checks": [], "strict_required_status_checks_policy": null}, "type": "deletion"}, {"p": {"required_approving_review_count": null, "required_status_checks": [], "strict_required_status_checks_policy": null}, "type": "non_fast_forward"}]}
{"bypass": [], "conditions": ["~DEFAULT_BRANCH"], "enforcement": "active", "name": "ruleone", "rules": [{"p": {"required_approving_review_count": null, "required_status_checks": [], "strict_required_status_checks_policy": null}, "type": "deletion"}, {"p": {"required_approving_review_count": null, "required_status_checks": [], "strict_required_status_checks_policy": null}, "type": "non_fast_forward"}]}
```

- Classic protection: `gh api repos/iberi22/xavier/branches/main/protection` -> HTTP 404 `Branch not protected`.
- ci.yml wall-time sample: `gh run list -R iberi22/xavier --workflow ci.yml --limit 30 --json databaseId,conclusion,event,createdAt,updatedAt,status`; run IDs: 36336129538, 36335203888, 36334908209, 36334844728, 36334751693, 36334621929, 36312871830, 36298447232, 36298407586, 36291934367, 36291922065, 36291910469, 36291899246, 36291877579, 36290838357, 36290530359, 36290529525, 36290265822, 36290256886, 36289887891, 36289528596, 36289526321, 36288706443, 36288204542, 36287843300, 36283375520, 36283372499, 36283369299, 36280621626, 36278605237.
- Per-job medians: `gh api repos/iberi22/xavier/actions/runs/<id>/jobs` over the 10 most recent ci.yml runs whose `Parallel Rust Tests` job was not skipped (selected from the last 80 runs); duration = completed_at - started_at.
- Local hooks: `.husky/pre-commit`, `.husky/pre-push` at the cited commit.

### Rust-run job durations (seconds) — the 10 runs behind the medians

| run id | tests | integration | clippy | frontend | msrv | fmt |
|---|---|---|---|---|---|---|
| 36290265822 | 584 | skipped | 195 | 178 | 105 | 16 |
| 36290529525 | 550 | skipped | 205 | 178 | 106 | 16 |
| 36290530359 | 586 | 361 | 168 | 188 | 98 | 18 |
| 36290838357 | 550 | skipped | 217 | 167 | 106 | 17 |
| 36291877579 | 580 | skipped | 200 | 175 | 115 | 13 |
| 36291899246 | 563 | skipped | 190 | 140 | 99 | 19 |
| 36291910469 | 582 | skipped | 175 | 117 | 114 | 22 |
| 36291922065 | 588 | skipped | 193 | 177 | 102 | 18 |
| 36291934367 | 587 | 378 | 203 | 159 | 93 | 17 |
| 36298407586 | 454 | 204 | 171 | 139 | 90 | 16 |
| median (min) | 9.7 | 6.0 | 3.2 | 2.9 | 1.7 | 0.3 |

Recompute: `gh api repos/iberi22/xavier/actions/runs/<run id>/jobs --jq '.jobs[] | [.name, ((.completed_at|fromdate)-(.started_at|fromdate))]'`.
