# Decision trace and public administration dashboard

Status: proposed design, 2026-09-27. This adds a **visibility** layer: it records every decision the autonomous pipeline makes, the model's stated reasoning, and the artifacts that decision produced, and publishes a read-only view of that trace. It grants no control. Owner actions (final product acceptance, ruleset activation, public release, spending) stay authenticated, separate and owner-only, exactly as reserved in `AGENTS.md:96-99`. Read with [02-AUTONOMOUS-PIPELINE](02-AUTONOMOUS-PIPELINE.md), [03-DONE-PROTOCOL](03-DONE-PROTOCOL.md), [07-ROLLBACK-GUARDIAN](07-ROLLBACK-GUARDIAN.md) and [01-DESIGN](01-DESIGN.md).

**Two referenced inputs do not exist in this repo and were not read:** `docs/adr/ADR-034-skill-controller.md` and `docs/design/skill-controller/panel/ADR-034-panel.md`. ADR-035 records that Skill Controller remains advisory and its rollout independent (`01-DESIGN.md:58`). The panel-record shape below is therefore specified from `AGENTS.md:96-99` and pipeline stage contracts, not copied from a real ADR-034 panel record. If a canonical panel record format already exists elsewhere, Phase 0 must reconcile this schema with it rather than fork it.

## 1. Boundary: visibility, never control

| Property | This design | Not in this design |
| --- | --- | --- |
| Read | Public, unauthenticated, cached | Owner-only actions, secrets, private repo internals |
| Write | Append-only records in git and CI; anyone may propose a correction | Editing a published record, deleting history, force push |
| Act | None | Merge, rollback, release, ruleset, spend, deploy — all stay in the authenticated operator path |
| Trust | Records are *evidence of what was decided*, not proof the decision was right | Self-reported quality; the DONE verifier still governs (`03-DONE-PROTOCOL.md:7`) |

The dashboard must never be a second control plane. A published record is a mirror of a decision that a gate already produced; if the dashboard and the gate disagree, the gate wins and the record is corrected by supersession, never by editing.

## 2. Decision Record schema

One JSON document per decision, versioned, schema-validated in CI. Files are git-tracked under `docs/decisions/` for public projects; private projects keep the same schema but a private path and are excluded from the public index by an explicit `visibility` field, not by path guessing.

```json
{
  "schema_version": 1,
  "id": "dec-2026-09-27-adr-035-accept-r2",
  "type": "panel-verdict",
  "subject": {
    "adr": "ADR-035",
    "issue": null,
    "pr": 412,
    "task_ids": ["T-018"],
    "deployment": null,
    "sha": "ccff27f2"
  },
  "actors": [
    { "model": "gpt-6-astra", "backend": "codex", "effort": "low",
      "role": "reviewer", "author_backend": "codex" },
    { "model": "deepseek-v4-flash", "backend": "opencode-go", "effort": "medium",
      "role": "reviewer", "author_backend": "codex" }
  ],
  "inputs": [
    { "kind": "claim-digest", "digest": "sha256:9f1c…", "length_chars": 1840,
      "artifact": "artifacts/reviews/adr-035-r2.md", "visibility": "public-safe" },
    { "kind": "xavier-search", "query_digest": "sha256:41ab…",
      "memory_refs": ["swal/xavier/proj#1831"], "result_count": 4 }
  ],
  "decision": { "verdict": "APPROVE", "blocking_defects": 0, "round": 2 },
  "rationale": "Panel approves on resolved simulation variance and CI evidence. Residual: docs-site build time still 4m12s p90, tracked in TASK_v1-panel-stabilization.",
  "evidence": [
    { "kind": "file", "ref": "docs/adr/ADR-035-trunk-based-ship-show-ask.md:19-35" },
    { "kind": "ci", "ref": "https://github.com/iberi22/xavier/actions/runs/9912" },
    { "kind": "commit", "ref": "ccff27f2" }
  ],
  "timestamps": { "created_at": "2026-09-27T14:02:11Z", "subject_reviewed_sha": "ccff27f2" },
  "supersedes": null, "superseded_by": null,
  "redaction": { "status": "reviewed", "policy": "public-v1", "removed_fields": [] },
  "visibility": "public",
  "signature": { "algo": "sha256", "value": "sha256:6c0e…", "canonical_fields": "all_except_signature" }
}
```

### Field rules

- **`id`**: `<type-prefix>-<date>-<subject-slug>-r<round>`. Stable forever; the whole trace indexes on it. Duplicate IDs fail validation (`03-DONE-PROTOCOL.md:9`).
- **`type`**: `panel-verdict`, `adr-acceptance`, `task-plan`, `orchestrator-decision`, `rollback-decision`, `final-acceptance`. The set is closed in v1; an unknown type fails CI.
- **`actors[]`**: model id, backend, effort tier, and `role` (`author` | `reviewer`). Independence is machine-checkable: no `reviewer` may share `author_backend` with the change under review (`02-AUTONOMOUS-PIPELINE.md:52`).
- **`inputs[]`**: a **digest plus a link**, never the full prompt. `claim-digest` stores a SHA-256 and a character count; the human-readable safe excerpt lives in a scanned artifact. A raw prompt containing private context must never be committed — see §6.
- **`decision.verdict`**: exact token (`APPROVE` / `REJECT` / `PARTIAL` / `ROLLBACK` / `ESCALATE` / `ACCEPTED` / `PENDING_OWNER`). `PARTIAL` is a real value and is **not** a pass, mirroring the adapter rule that converts `PARTIAL` to `reject` (`02-AUTONOMOUS-PIPELINE.md:32`).
- **`rationale`**: the model's reasoning **as written in its answer** — the verdict text, findings, cited evidence, residual risk. It is a quote or faithful summary of the produced report, bounded to ~600 characters plus a link to the full scanned report.
- **Hidden chain-of-thought is out of scope by construction.** The pipeline invokes external CLIs; no CoT is available. Fabricating a "reasoning trace" field is prohibited, and a validator must reject any rationale longer than the cap with no artifact link, since that pattern indicates invented filler.
- **`evidence[]`**: `file` refs use `path:line` and are resolved at validation time; a dangling ref fails. `ci` refs are run URLs. `commit` refs are full SHAs, never abbreviations (`03-DONE-PROTOCOL.md:25`).
- **`timestamps.subject_reviewed_sha`**: binds the record to one reviewed revision. Any head change invalidates it and forces a new round (`02-AUTONOMOUS-PIPELINE.md:33`).
- **`supersedes` / `superseded_by`**: corrections are append-only. A wrong record is superseded, never edited; the timeline shows both with the newer one marked current.
- **`redaction.status`**: `pending` | `reviewed` | `redacted`. A `panel-verdict` with `redaction.status != "reviewed"` cannot reach the public index.
- **`signature`**: digest over all fields except itself, so tampering is detectable by a third party. It is an integrity check, **not** a cryptographic attestation of authorship; do not describe it as one on the board.

### Where records live

| Project visibility | Path | Public index |
| --- | --- | --- |
| Public SWAL repo | `docs/decisions/<id>.json` | Yes |
| Private repo | `.private/decisions/<id>.json` (git-ignored, private mirror) | No; only a count and an opaque digest appear, if at all |
| Cross-repo feed | each repo emits its own index; the aggregator concatenates | Union, with `repo` field |

Public JSON under `docs/` changes the public Pages build trigger, which is an Ask-class change requiring pre-merge panel review and recorded owner pre-authorization of the publication class (`01-DESIGN.md:35`, `.github/workflows/deploy-docs.yml:3-12`). That gate is the reason this design has no silent publication path.

## 3. Linking decisions to the work they produced

Linking is the whole point; a decision record with no artifact link is rejected.

| Mechanism | Form | Enforced by |
| --- | --- | --- |
| Commit trailer | `Decision-Record: dec-2026-09-27-adr-035-accept-r2` and `Reviewed-by-Panel: gpt-6-sol,deepseek-v4.1-flash` | `scripts/validate-decisions.sh`; CI fails a commit claiming a nonexistent record |
| PR comment | Post-merge bot comment with record id, verdict, panel models and evidence URLs; one comment per round, never edited | `03-DONE-PROTOCOL.md:46-57` package generator |
| Issue | Decision id appended to the closing comment of the issue/PR it gated | Atlas evidence string |
| Atlas task evidence | `Decision-Record <id> @ sha <sha>` alongside bundle URL/SHA (`03-DONE-PROTOCOL.md:61`) | `atlas --json verify` evidence rule |
| Diffs | Record id in the commit body, so `git log --grep` reconstructs the trace | Travis |

Reverse links are computed, not stored: given a record, resolve `subject.pr` → changed files, `subject.sha` → commits, `subject.task_ids` → Atlas tasks, and the incident issue for that SHA. Missing target → the record renders with a visible `unlinked` marker instead of silently disappearing.

## 4. Pipeline: validate, index, publish, retain

New workflow `.github/workflows/decisions-index.yml`, triggered on `main` push when `docs/decisions/**`, `docs/adr/**` or the validator changes, plus scheduled rebuild.

1. **Validate.** Schema check (required fields, closed `type` set, id format, full-length SHAs, `rationale` cap with artifact link). Independence check: no reviewer shares the author backend. Link check: every `evidence` ref resolves (file exists at that line, CI run reachable, SHA is an ancestor). Signature recompute. Duplicate id detection.
2. **Scan.** Reuse existing guards rather than new scanners: `scripts/check-secrets.sh` (gitleaks) and `scripts/check-strategy-leak.sh` over added lines and full new text blobs; personal-path patterns for `/home/<user>/`, `/Users/<user>/`, `C:\Users\<user>\`, `/tmp/claude-*`, allowing `$HOME` and relative paths (`02-AUTONOMOUS-PIPELINE.md:42`). Fail closed when a scanner is unavailable — an absent gitleaks is not a pass (`01-DESIGN.md:45`). A secret report may be linked; matches never appear in a public artifact (`03-DONE-PROTOCOL.md:27`).
3. **Emit the public index** to `docs/site/public/decisions/index.json` plus `feed.json` (Atom). The index is denormalized: one array of records with the full trace, and one `graph.json` of edges `commit↔decision↔task↔adr↔pr` so the client never parses git. Deterministic ordering by `created_at` then `id`, so rebuilds produce no diff noise. Output includes a `generated_at` and the `main` SHA it was built from.
4. **Publish** through the existing static stack: the site is Astro 7 + Starlight 0.42 (`docs/site/package.json`), so the board is static-first and ships from the same GitHub Pages artifact. No new runtime service, no repo-local database, consistent with `02-AUTONOMOUS-PIPELINE.md:7`.
5. **Cache/API.** The static `index.json` *is* the read-only API: a `Cache-Control: public, max-age=300, stale-while-revalidate=86400` edge header on the artifact, plus a versioned copy `index.<sha>.json` for reproducible fetches. Clients (web, Flutter) fetch JSON only; the server never computes anything. A Cloudflare Worker is optional later for query/pagination at the edge — not required for v1, and it must remain a pure cache with no write path.
6. **Retention.** Records are permanent and append-only. Superseded records stay in the feed with `superseded_by` set; the index marks them non-current. Deletion happens only for a record that leaked material — and then as a `redaction` action with an audit entry stating what was removed and why, never a silent file removal.
7. **Corrections.** A wrong rationale is fixed by a new record with `supersedes: <id>`. The validator rejects any diff that mutates an existing record's `decision`, `rationale` or `evidence`; such a diff must add a file.

Failure behavior: any validator failure blocks the index build and the Pages deploy for that SHA, and opens or updates one `tbd-incident` issue keyed by SHA (`01-DESIGN.md:23`). A red index never silently serves a stale one as current: the feed keeps the last good `generated_at` and states the reason.

## 5. Clients

### 5.1 Public web board

Static-first, no auth, no cookies, no per-user state. Reads `index.json` at build time for the timeline and at runtime for deep links.

| Screen | Content | Source |
| --- | --- | --- |
| Timeline | Reverse-chronological records, filter by `type`, repo, model, date; superseded records visibly marked | `index.json` |
| Decision detail | Verdict, panel models, rationale, redaction status, full evidence list with working links | `index.json` |
| Trace graph | `commit ↔ decision ↔ task ↔ ADR ↔ PR` for one ADR or feature; SVG, no runtime graph lib | `graph.json` |
| Panel stats | Per model: accept rate, reject rate, rounds to acceptance, blocking defects found, records authored vs reviewed | derived client-side |
| Rollback incidents | Guardian decisions: target, detection, attribution, action, result, MTTR, false positives, unresolved (`07-ROLLBACK-GUARDIAN.md:39`) | `index.json` type `rollback-decision` |
| Pipeline health | Last green `main` SHA, current `tbd-incident` count, index freshness, per-class panel latency (`01-DESIGN.md:35`) | `index.json` + `status.json` |

Panel stats are descriptive, never a leaderboard or a model ranking. Rounds-to-acceptance and defects-found are shown per model **and** per class, because a model that reviews more Ask-class changes will naturally show more rounds; a bare accept rate is misleading.

### 5.2 Flutter mobile app (owner)

The Flutter app is the **operator** client, not a mirror of the public board. It is where the owner accepts, so it must not imply that public visibility grants authority.

- **Read screens** mirror §5.1 timeline, detail, trace, stats and incidents.
- **Offline cache**: `index.<sha>.json` fetched opportunistically, stored in app-private storage, served with a visible `as of <generated_at>` stamp and a `stale` badge beyond the freshness window. Never present cached data as current.
- **Push notifications**: only for `rollback-decision` and incident records (and only when the guardian escalated or acted). A `panel-verdict` never pushes. Notification payload carries record id and target, not content; the app deep-links to detail.
- **Deep links**: `swal://decisions/<id>` and a universal web link resolving to the public board page, so a shared link works for anyone and actions stay gated in-app.
- **Auth boundary**: any mutating action (accept product, authorize a publication class, activate rulesets) uses the existing authenticated owner path and is never triggered by a public deep link or a notification action. Unauthenticated clients get read-only, server-enforced, not merely hidden in the UI.

## 6. Privacy and licensing

### 6.1 Never public

Secrets, tokens, private keys; private-repo internals (file names, diffs, issue text, architecture not already public); personal absolute paths (use `$HOME`); customer or production data; raw prompts containing private context; internal reasoning padding or invented CoT; anything a project marks `visibility: private`.

Enforcement is layered, and the pipeline validator plus the existing gitleaks/strategy-leak scanners are mandatory gates, not advisory review. Redaction is recorded, not silent: `redaction.removed_fields` names what was withheld so a reviewer can tell a short rationale from a censored one.

### 6.2 Licensing: an owner decision, stated plainly

The actual license of this repo is **dual**: `SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Xavier-Commercial` (`LICENSE:1-8`). That is not the same as "marked commercial/FSL-pending", and the distinction matters here, because the public board is a network-published work.

- Under AGPL-3.0, offering the board over a network obliges you to offer the corresponding source to users. Fine for the board, if the board is AGPL.
- A component published only under the commercial arm is not AGPL-compatible for a public AGPL board. Serving the public board from a commercial-only component would create a licensing conflict, and the public board's source-offer duty would be unmet.

**Recommendation (owner decides):** host and publish the public board from the **AGPL-3.0-only** side — the docs site and the index builder, which is where §4 already places them — and keep the operator app (private repo, commercial/FSL path, authenticated owner actions) as a separate client that consumes the public read-only index. Consequences: the board stays publishable and the source offer is satisfied; the operator app keeps its commercial terms; the only cost is that no commercial-licensed code may be linked into the board, so the board must stay static and dependency-light.

## 7. Phased implementation

Each task is ≤4 files and ≤400 changed lines; a larger item is split (`02-AUTONOMOUS-PIPELINE.md:7`). Every task is one PR referencing its feature ID, and anything under `docs/design/**`, `docs/decisions/**` or `.github/**` is Ask-class with pre-merge panel review (`01-DESIGN.md:33`).

| Phase | Task | Files (≤4) | Exit criterion |
| --- | --- | --- | --- |
| **P0 Schema** | T1 schema + JSON Schema draft + `redaction`/`visibility` semantics | `docs/decisions/SCHEMA.md`, `schema/decision-record.v1.json`, `docs/decisions/README.md` | Schema validates a hand-written record for all six types; ADR-034 panel format reconciled or absence confirmed in writing |
| | T2 validator CLI (`scripts/validate-decisions.sh` + Python core) | script, core module, 2 fixtures | Rejects: missing field, bad type, short SHA, dangling evidence, reviewer==author backend, over-cap rationale with no link, duplicate id |
| **P1 Capture** | T3 write a record from a real panel round (ADR-035 accept r2) | 1 record + 1 fixture test | Real record passes the validator; no invented fields |
| | T4 emit record from `co-agent-review.sh` output (adapter integration) | adapter, script patch, 1 test | Every review produces a record bound to the reviewed SHA; `PARTIAL` maps to `REJECT` |
| | T5 commit trailers + PR comment bot | pre-commit guard, workflow, 2 tests | A commit citing a nonexistent record fails; a PR gets one immutable comment per round |
| **P2 Publish** | T6 index/feed/graph builder | builder script, 2 fixtures | Deterministic output; no diff on rebuild; graph edges resolve both directions |
| | T7 validator + scanner in CI on `main` | workflow, schema pin, config | Red index blocks the Pages deploy and opens/updates one `tbd-incident` by SHA |
| | T8 cache headers + versioned index | config, 2 fixtures | `index.<sha>.json` fetchable; `Cache-Control` verified from the deployed URL |
| **P3 Board** | T9 static board: timeline + detail | 2 Astro pages/components, 1 fixture | Renders offline from `index.json`; no client fetch needed for the timeline |
| | T10 trace graph + panel stats | 2 components, 1 derivation module, 1 test | Per-model stats shown with class context; superseded records marked |
| | T11 incidents + pipeline health pages | 2 pages, 1 fixture | Rollback and incident records render with MTTR fields; freshness stamp visible |
| **P4 Mobile** | T12 Flutter read-only screens + offline cache | 3 screens/modules, 1 test | Cached data always shows `as of` and `stale`; no write path exists |
| | T13 push for rollback/incident only + deep links | 2 modules, config, 1 test | No push for a `panel-verdict`; deep link is read-only for unauthenticated state |
| **P5 Governance** | T14 superseding-correction flow + retention audit | validator patch, schema note, 1 test | Mutating a published record's `decision`/`rationale`/`evidence` fails CI |
| | T15 license decision recorded as an ADR | ADR, 2 index/builder files if needed | Owner-signed ADR states which arm serves the public board |

### Open questions for the owner

1. **Licensing** — confirm the public board is served from the AGPL arm (§6.2), or state the alternative and its obligations.
2. **Public pre-authorization** — is the `docs/decisions/**` publication class pre-authorized, or does every record PR wait in the Product Acceptance Package? The current rules make it wait (`01-DESIGN.md:35`).
3. **Panel record format** — does a canonical panel record exist outside this repo that the schema must match?
4. **Retention horizon** — permanent is the default here; confirm no record may be pruned even for noise, since the value of the trace is its completeness.
5. **Model stats disclosure** — publishing per-model accept rates is accountability, but it is also a standing comparison. Confirm that is intended for public display.
