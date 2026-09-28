# Trunk-based development: evidence and fit

Date: 2026-09-27. Status: proposal; see [the design](01-DESIGN.md) and [ADR-035](../../adr/ADR-035-trunk-based-ship-show-ask.md). `facts/FACTS.md` is the measured snapshot tracked in this directory, with a reproducible evidence section. Targets below are goals, not measured results.

## Current state

| Observation | Evidence | Consequence |
| --- | --- | --- |
| `main` has no classic protection; two active rulesets only prevent deletion and force pushes. There are no required checks or reviews. | `facts/FACTS.md:2`, Evidence (reproducible, captured 2026-09-27), lines 11-20 | A direct push currently reaches `main` without a server-side gate. |
| The main workflow runs on PRs and `main` pushes. Its Rust path filter covers source, manifests, tests, build scripts and its own workflow. | `.github/workflows/ci.yml:3-8`, `.github/workflows/ci.yml:15-48` | Filtering exists, but there is no single required check that reports a result for every path. |
| Rust format, check/Clippy, MSRV, root library tests and one integration target run as separate jobs. Root library tests run serially because tests share process environment. | `.github/workflows/ci.yml:50-182` | The root test job cannot simply be sharded or moved unchanged into a three-minute gate. |
| The frontend job runs on every PR and push, including backend changes, after a path-filtered smoke test missed a backend regression. It builds, checks token leakage, and runs browser smoke. | `.github/workflows/ci.yml:198-269` | Moving browser smoke after merge creates a known cross-layer detection gap; the token canary needs a pre-merge path for relevant UI changes. |
| Secret scan and version checks run in the main workflow. The separate parity PR workflow explicitly reports only and ignores docs/UI paths. | `.github/workflows/ci.yml:184-196`, `.github/workflows/ci.yml:271-292`, `.github/workflows/pr-gate.yml:1-13`, `.github/workflows/pr-gate.yml:23-52` | Neither workflow currently defines a protected fast gate. |
| CI wall time across 30 recent successful runs was p50 9.4 min, p90 11.1 min. On Rust runs, median root tests were 9.7 min, integration 6.0 min, Clippy 3.2 min; there was no flaky signal in this sample. | `facts/FACTS.md:4-6`, Evidence (reproducible, captured 2026-09-27), lines 21-22 | A three-minute gate needs a different test scope and proof against missed failures. Flaky-test work is conditional, not a presumed prerequisite. |
| The feature ledger is supposed to be verified by a green pipeline on every PR, but the workflow never invokes `verify-pipeline.sh`. Full mode compiles several test targets and executes every declared filter. | `AGENTS.md:44-49`, `.github/workflows/ci.yml:14-292`, `scripts/verify-pipeline.sh:119-247` | The written rule and implementation disagree. Full ledger verification cannot fit the proposed fast gate. |
| Husky 9 is mandated. The pre-commit hook checks staged strategy leakage and DB paths, but runs workspace-wide `cargo fmt --all --check` and package-scoped Clippy (`-p xavier -p code-graph --all-targets`), full UI build and tracked-file secret scan when related files are staged. | `AGENTS.md:86-94`, `.husky/pre-commit:7-64`, `scripts/check-secrets.sh:3-33` | Preserve Husky; make local checks genuinely staged-aware and keep independent server checks. |
| Jules feedback skips failures on `main`. Coverage is report-only, including Rust tarpaulin with a known parser issue. | `.github/workflows/jules-ci-feedback.yml:22-40`, `.github/workflows/rust-coverage.yml:16-53` | Post-merge failure handling must be added; coverage cannot become a required gate yet. |
| Releases run on tags/manual dispatch. Docs deploy to Pages after related `main` pushes. The binary runs as a local systemd service, without continuous deploy. | `.github/workflows/release.yml:3-8`, `.github/workflows/deploy-docs.yml:3-12`, `.github/workflows/deploy-docs.yml:53-63`, `facts/FACTS.md:9` | Merge-to-deploy and automatic binary rollback do not exist. Docs changes still have real Pages exposure. |

## Gaps against the owner's proposal

1. **Fast gate:** the measured Rust tests and Clippy exceed the proposed budget; current PR checks are not required. A green `main` sample does not prove a smaller gate is safe (`facts/FACTS.md:4-6`, Evidence (reproducible, captured 2026-09-27), lines 21-22; `.github/workflows/pr-gate.yml:23-52`).
2. **Direct Ship/Show:** an agent push would bypass all server-side checks today (`facts/FACTS.md:2`, Evidence (reproducible, captured 2026-09-27), lines 11-20). A required status check is produced after a PR commit is uploaded; it does not pre-approve a direct local push. Eligible non-publishing agent Ship and Show changes use short PRs and auto-merge; rollback PRs and Pages-triggering changes require pre-merge panel review.
3. **Post-merge safety:** the present Jules notifier exits on `main`, and the PR parity workflow can succeed despite a failed command (`.github/workflows/jules-ci-feedback.yml:29-40`; `.github/workflows/pr-gate.yml:23-52`). An async lane without a tracked failure response would increase time-to-detect.
4. **Policy:** `AGENTS.md:80-84` says one PR per feature and PR CI runs format, Clippy, tests, ledger verification and secret scan; the current ledger workflow gap already violates `AGENTS.md:44-49`. Moving full verification after merge requires an explicit policy update alongside ADR acceptance, not a silent reinterpretation.
5. **Local hook tooling:** Lefthook conflicts with the Husky-only directory rule (`AGENTS.md:86-94`). `scripts/check-secrets.sh:13-33` scans tracked files, so staged newly added content needs its own indexed-content scan in the hook; server-side scan remains authoritative.
6. **Deploy/rollback:** binary release is tag-driven and local install is separate (`.github/workflows/release.yml:3-8`; `facts/FACTS.md:9`). Automatically reverting a GitHub commit cannot undo a running binary or a published release. Pages deployment is automatic for docs paths (`.github/workflows/deploy-docs.yml:3-12`).

## Agent-specific failure modes and decisions

The measured contributor mix is mostly autonomous agents. Observed errors include writes in another checkout, personal paths in output, and unverified claims that tests passed (`facts/FACTS.md:7-8`). A self-applied Ship label cannot be treated as proof of low risk. PR automation must classify by changed paths and diff size, fail closed on unknown paths, and surface an auditable check result. For agents, repository permissions and branch rules prevent direct `main` pushes; the label only controls review timing. Each PR records a feature ID for code, exact check run links, and the owner of post-merge failures (`AGENTS.md:51-55`, `AGENTS.md:80-84`). Hooks help local CLI agents but do not cover cloud agents or bypassed hooks (`.husky/pre-commit:1-3`; `.husky/pre-push:1-5`).

| Proposal element | Disposition | Reason |
| --- | --- | --- |
| Two lanes and small branches | **Adopt with a measured gate** | Parallel scoped checks can improve lead time, but fail closed if impact is unknown and keep full evidence after merge. |
| Ship/Show/Ask | **Adapt** | Eligible non-publishing Ship/Show use PR auto-merge for agents; Ask requires pre-merge approval by two independent panel models, neither the author. Rollback PRs and outward-publishing changes also require pre-merge panel review. The owner gives final product acceptance. Human direct push is reserved for an audited emergency bypass, not routine Ship. |
| Staged local hooks | **Adopt within Husky 9** | Preserve the mandated hook system (`AGENTS.md:90`). |
| Lefthook | **Reject** | Adds a competing hook manager in conflict with `AGENTS.md:90`. |
| Broad direct push for agents | **Reject** | Current rules have no pre-push server gate (`facts/FACTS.md:2`, Evidence (reproducible, captured 2026-09-27), lines 11-20) and observed agent scope errors raise blast radius (`facts/FACTS.md:8`). |
| Branch by abstraction and flags | **Adopt selectively** | Use an interface only for a real migration seam; Cargo features and documented environment configuration already exist (`Cargo.toml:61-86`; `AGENTS.md:63-69`). |
| Auto-deploy and binary rollback on every merge | **Reject for now** | The runtime has no continuous binary deploy (`facts/FACTS.md:9`). Revert code in GitHub and handle local deployments separately. |

## Targets and GitHub measurement

Record baseline and weekly rolling p50/p90 for each metric. Exclude bot-only dependency noise only when the exclusion rule is fixed in advance. Count failed and cancelled checks separately. Do not claim improvement from a target alone.

| Metric | Target | GitHub data and calculation |
| --- | --- | --- |
| Fast-gate latency | p50 < 3 min and p90 < 3 min for eligible PRs after a two-week shadow period; report miss rate and cold-cache p90. | Actions workflow runs/jobs for the required `fast-gate` check: `completed_at - created_at` for PR head SHA, from first queued run through completion, including queue time. Slice by docs/UI/Rust/Ask and cache hit. |
| Lead time | p50 < 1 business day for Ship/Show; trend p90 against the pre-rollout baseline. | PR first commit timestamp to `merged_at`, using PR commits API and PR metadata. Report coding-to-merge separately from `created_at` to `merged_at` review time; report tag/release lag separately because merge is not binary deploy. |
| MTTR | p50 < 4 h for detected `main` regressions, plus p90 and unresolved count. | Open incident issue linked to failed `main` run and offending SHA; close only after a green run on the revert/fix SHA. `closed_at - failed run completed_at`; issue label `tbd-incident`, run URL and fix PR/revert PR required. |
| Change-failure rate | < 10% of merged changes over a rolling 30 days; include Pages failures and separately report release failures. | Count merged PRs (or audited direct pushes) linked to a `tbd-incident` within seven days, divided by merged PRs/direct pushes in that period. Deduplicate by source SHA. Use `tbd-incident` links and Actions outcomes; a red post-merge run creates an incident even if a later retry passes. |

These targets require a baseline for lead time and incidents; `facts/FACTS.md:4-6` (Evidence (reproducible, captured 2026-09-27), lines 21-22) supplies only CI timing and recent success, not DORA values. GitHub alone measures integration recovery and integration failures, not production DORA MTTR/change-failure rate for the locally installed binary (`facts/FACTS.md:9`). Link any real deployment incident to its release tag and issue, and report production metrics separately when that evidence exists.
