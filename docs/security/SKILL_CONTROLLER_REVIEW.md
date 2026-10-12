# Skill Controller Security Review (O04)

Status: **no unresolved release-blocking finding.** Reviewed at `main` 59a29d97 (2026-10-12).

**Scope:** `src/context/skill_controller/` (all modules), the skill HTTP routes (`src/api/skills.rs`), the MCP skill tools (`src/server/mcp/tools_context.rs`), `src/cli/commands/spawn.rs`, skill telemetry, `scripts/skills-migrate-batch.sh` and `scripts/skill-controller-guards.sh`.

**Threat model:** `docs/design/skill-controller/01-ARCHITECTURE-AND-ADR.md` (threats section) and the checklist in `docs/security/skill-controller-review.md`.

**Method:** a static review by an independent model, then verification of every finding against the code by the maintainer. Each finding below has a regression test that failed before its fix.

## Findings

| ID | Severity | Area | Status | Fix | Regression test |
|----|----------|------|--------|-----|-----------------|
| F-01 | Low | Package secrets | Resolved | #2979: task-scoped skill publication runs the same secret classes as package projection (`policy::scan_text`) and writes nothing on a finding | `context::skill_controller::lease::tests::test_secret_finding_blocks_publication_without_creating_link` |
| F-02 | Low | Path containment | Resolved | #2979: skill names passed to spawn must be valid manifest slugs before any path is built | `cli::commands::spawn::tests::test_load_skill_rejects_invalid_slugs` |
| F-03 | Low | Migration safety | Resolved | #2979: the batch migration runs `tar` without inherited `TAR_OPTIONS`, so backups store symlinks as symlinks | `scripts/tests/skills-migrate-batch-test.sh` → `test_backup_symlink_does_not_dereference` |
| F-04 | Info | Race-resistant paths | Accepted limitation | Writes happen by pathname after `authorize` + `revalidate`. The remaining window needs a local process with write access to an allowlisted root. Follow-up: Linux `openat2(RESOLVE_BENEATH)` writes relative to the pinned directory | Existing: `lease::tests::test_escaped_parent_and_changed_link_are_preserved`, `planner` identity-change tests |
| F-05 | Info | Lease hygiene | Accepted, follow-up | `lease::cleanup_stale` is tested but not yet scheduled, so expired task skills from crashed sessions remain until removed by hand. Not a security boundary | `lease::tests::stale_cleanup_requires_expiry_and_confirmed_inactivity` |

Owner of the remediations: the Xavier maintainer. F-04 and F-05 are tracked as follow-ups and do not block release.

## Areas Checked OK

### 1. Manifest trust — PASS

**Evidence:**
- `manifest.rs:248–254` — `from_toml` calls `validate()` before returning.
- `manifest.rs:256–383` — `validate()` enforces: version == 1; non-empty sources/targets; valid slugs (`is_valid_slug` at `:177–183`); no duplicate target IDs; non-empty tools/roots; no tilde roots; no absolute/traversal/glob placement paths (`:312–320`); folder-name == placement-name (`:325–334`); no conflicting target or package names (`:336–362`); frontmatter name matches placement (`validate_package_frontmatter` at `:233–246`).
- `manifest.rs:189–191` — `has_path_traversal` splits on `/` and `\\` and rejects `..`.
- `manifest.rs:192–194` — `is_absolute_path` rejects leading `/`, `\`, `~`, and platform-absolute paths.
- `policy.rs:100–104` — `FsPolicy::new` canonicalizes every root once; roots must be real directories (`canonical_roots` at `:472–487`).
- `cli/handlers/skills.rs:69–90` — `roots_and_policy` builds the policy from the manifest's own declared roots; no env var widens the allowlist (D5).

**Verdict:** The manifest cannot grant itself new roots, configure secrets, or execute commands. Repository content cannot widen the policy beyond what the manifest names.

---

### 2. Race-resistant paths: TOCTOU, symlink escapes, `..` — PASS

**Evidence:**
- `policy.rs:122–126` — `authorize` rejects any `Component::ParentDir` before touching the filesystem.
- `policy.rs:127` — `fs::canonicalize` resolves the full path.
- `policy.rs:132–135` — resolved path must `starts_with` an allowed root.
- `policy.rs:139–159` — literal (no-follow) walk of every path component; a symlink is tolerated only if it lands inside the allowlisted root, otherwise `SymlinkComponent` error.
- `policy.rs:160–164` — final resolved path must be a directory, not a special file.
- `policy.rs:165–170` — `open_dir` pins `(dev, ino)`; `revalidate` (`:190–197`) re-opens and compares identity, catching directory swaps.
- `projector.rs:100–108` — `RootLock` serializes concurrent applies to the same root (sorted `BTreeSet` acquisition prevents deadlock).
- `projector.rs:245–259` — `atomic_symlink_swap` stages at a unique temp name, `rename` (atomic), fsync parent; never follows a pre-existing destination.
- `rollback.rs:239–253` — `atomic_symlink_restore` uses the same stage+rename+fsync pattern.
- `lease.rs:76–85` — `mkdir` recursively creates and revalidates each parent before `create_dir`.
- `lease.rs:87–93` — `vacant` refuses any pre-existing path before creation.
- `backup.rs:219–279` — `atomic_write` uses `create_new` temp + `rename` + fsync; mode 0o600.
- `backup.rs:100–117` — `backups_dir`/`backup_file_path` hash `tx_id` and `target_path` into the name, preventing traversal via malicious IDs.
- Tests: `policy.rs:537–576` (escape, traversal, symlinked parent), `planner.rs:448–502` (destination identity change invalidates plan), `lease.rs:428–445` (escaped parent preserved).

**Verdict:** Containment is enforced by canonicalize + literal component walk + `(dev, ino)` pin + revalidation. The residual TOCTOU window is F-04 (Info).

---

### 3. Journal recovery: crash mid-apply, atomic rename — PASS

**Evidence:**
- `state.rs:261–288` — `apply_with_journal`: backup → record entry → mutate. If backup or record fails, mutation never runs (`test_journal_failure_prevents_target_mutation` at `state.rs:347–374`).
- `state.rs:276` — `backup_target` reads prior state *before* the mutation is journaled.
- `state.rs:121–140` — `begin_transaction` writes the tx record atomically via `atomic_write`.
- `state.rs:176–209` — `commit_transaction`/`rollback_transition` update state atomically.
- `projector.rs:110` — `begin_transaction` before any entry is applied.
- `projector.rs:123–125` — `commit_transaction` only after all entries applied.
- `projector.rs:457–506` — `test_fault_between_entries_leaves_recoverable_pending_journal_and_no_partial_mutation`: a fault between entries leaves a `Pending` tx with exactly the applied entry journaled; the failed entry is not applied.
- `rollback.rs:71–117` — `rollback_transaction` restores each entry independently; an entry edited since apply is reported as `Conflict` and left untouched (`rollback.rs:122–176`).
- `rollback.rs:228–234` — `remove_owned` only unlinks the exact owned entry; never recursive deletion.
- `rollback.rs:381–436` — `test_rollback_restores_unchanged_prior_link_and_removes_unchanged_new_link`: rollback restores the original link and removes the new one.
- `rollback.rs:412–436` — `test_rollback_leaves_user_edited_destination_intact_and_reports_conflict`.
- `rollback.rs:438–462` — `test_repeated_rollback_is_idempotent`.
- `backup.rs:219–279` — `atomic_write` is the atomic-rename primitive used by the journal.

**Verdict:** Write-ahead journaling with atomic rename is correctly implemented. A crash mid-apply leaves a recoverable `Pending` transaction; rollback restores prior state precisely and never destroys user edits.

---

### 4. Package secrets refused before publication — PASS

**Evidence:**
- `policy.rs:29–44` — `SECRET_CLASSES`: 8 high-signal regex classes (OpenAI/Router, GitHub PAT, AWS, Slack, Google, private key block, Xavier token, Xavier Supabase key).
- `policy.rs:279–333` — `scan_package`: walks every regular file with `follow_links(false)`, bounds depth/nodes/bytes, rejects special files (`SpecialFile`), hardlinks (`Hardlinked`), unreadable members (`Unreadable`), and symlink escapes (`OutsideAllowlist`).
- `policy.rs:384–407` — `member_meta`: bounded symlink hop resolution (max 4 hops), refuses links that leave the package root.
- `policy.rs:411–439` — `read_member`: opens by handle, verifies `(dev, ino)` identity still matches the pathname, enforces size cap.
- `policy.rs:246–274` — `plan_publication`: if `!dry_run && scan.blocks_publication()`, returns `SecretsBlocked` with a redacted `<class> at <relative path>` summary (never the matched bytes).
- `projector.rs:167–170` — `apply_entry` calls `scan_package` immediately before publication; on finding, returns `BlockedSecret` and creates no link.
- `policy.rs:214–225` — `PackageScan::Display` emits only `class at location` pairs; no secret bytes.
- Tests: `policy.rs:607–631` (secret in references blocks with class+location only), `policy.rs:632–655` (dry run reports without touching disk), `projector.rs:522–556` (secret finding blocks publication without creating a link), `rules.rs:68–71` (extra patterns: PKCS#8, OpenSSH, `KEY=VALUE` shapes).

**Verdict:** Whole-package secret scanning is enforced before any publication, with redacted findings. The scanner rejects unscannable content rather than skipping it.

---

### 5. Prompt/evidence injection — PASS

**Evidence:**
- `composer.rs:22–24` — `LINE_TERMINATORS`: `\n`, `\r`, VT, FF, NEL, LS, PS.
- `composer.rs:308–326` — `normalize_terminators`: collapses all terminators to `\n`; drops invisible chars (`is_invisible` at `:328–333`: ZWSP, ZWNJ, ZWJ, bidi control, bidi isolate, BOM) and control chars (except `\n`).
- `composer.rs:283–306` — `scrub`: bounds to 512 chars; replaces `EVIDENCE_OPEN`/`EVIDENCE_CLOSE` with `[delimiter removed]`; replaces fence lines (all dashes) with `[delimiter removed]`.
- `composer.rs:340–343` — `is_fence_line`: catches any non-empty line of only `-` chars, preventing forged frontmatter close.
- `composer.rs:271–281` — `quoted_block`: every line of evidence body is prefixed with `> `; the block is delimited by `--- BEGIN/END UNTRUSTED EVIDENCE`.
- `composer.rs:356–366` — `safe_origin`: origin IDs are validated to `[A-Za-z0-9_.:-]` (max 64) or replaced with `id-<sha256[:12]>`.
- `composer.rs:349–354` — `is_rule_override`: drops evidence containing known override markers ("ignore previous instructions", "reveal the secret", "disable the secret scan", "widen the workspace scope", etc.).
- `composer.rs:88–90` — `CompositionScope::admits`: evidence is admitted only if `workspace_id` exactly matches the authorized scope.
- `composer.rs:19` — `MAX_SECTION_ITEMS = 6`: bounds the number of evidence items per kind.
- Tests: `composer.rs:547–596` (forged frontmatter/delimiter escape), `composer.rs:598–625` (U+2028), `composer.rs:627–647` (U+2029), `composer.rs:649–665` (NEL/CRLF/VT/FF), `composer.rs:667–694` (bidi/zero-width), `composer.rs:696–720` (forged origin ID), `composer.rs:722–738` (rule override dropped).

**Verdict:** Untrusted evidence is neutralized: terminators normalized, invisible chars stripped, delimiters replaced, fence lines scrubbed, every line quoted, origin IDs hashed, known override patterns dropped. The composer's own structure cannot be forged by evidence content.

---

### 6. REST/MCP read-only boundaries — PASS

**Evidence:**
- `skills.rs:18–26` — `skill_api_routes` defines 5 routes: `POST /api/skill/dispatch`, `GET /api/skill/list`, `GET /skills`, `GET /api/skill/usage`, `GET /api/skill/audit`. **None mutates skills or the filesystem.**
- `skills.rs:82–140` — `dispatch_skill`: builds a registry, calls `dispatcher.dispatch`, returns a context pack. No filesystem write.
- `skills.rs:207–240` — `list_skills`: reads registry, returns skill metadata. Read-only.
- `skills.rs:299–313` — `skill_usage`: opens the telemetry DB **read-only** (`SQLITE_OPEN_READ_ONLY` at `:256`), returns aggregate counts.
- `skills.rs:441–466` — `audit_skills`: reads the manifest from `XAVIER_SKILL_MANIFEST` env, runs the audit, returns the report. No writes.
- `tools_context.rs:100–126` — MCP tools: `xavier_dispatch_skill`, `xavier_skill_list`, `xavier_skill_usage`. All read-only.
- `tools_context.rs:421–486` — `xavier_dispatch_skill` handler: dispatches and returns a context pack. No filesystem mutation.
- `tools_context.rs:505–533` — `xavier_skill_list`: returns visible skills filtered by clearance.
- `tools_context.rs:488–504` — `xavier_skill_usage`: returns aggregate usage counts.
- `cli/commands/mod.rs:555–557` — `handle_skills_command`: the CLI path that calls `apply_skill_plan` directly, **not** exposed as an HTTP route. Comment: *"so no new network mutation route is added (D6)."*
- `skill-controller-guards.sh:26–45` (guard_01) — statically enforces that no MCP tool name matching `xavier_.*skill.*` exists outside the allowed set `{xavier_skill_list, xavier_dispatch_skill, xavier_skill_usage}`.
- `skill-controller-guards.sh:47–66` (guard_02) — statically enforces that `audit_skills` and `skill_usage` take no request extractors.
- `cli/server.rs:1838–1845` — `protected_routes` (which includes the merged `skill_api_routes` at `:1333`) is wrapped in `auth_middleware`.
- `tools_context.rs:447, 508` — MCP skill tools use `caller_clearance()` for clearance filtering.
- Tests: `tools_context.rs:758–801` (clearance filtering on list), `tools_context.rs:803–858` (clearance filtering on dispatch), `tools_context.rs:860–900` (anonymous identity defaults to unclassified).

**Verdict:** No MCP tool or unauthenticated HTTP route can mutate skills or the filesystem. The only mutation surface is the CLI `xavier skills apply` command, which runs locally and is behind no network exposure.

---

### 7. Telemetry privacy — PASS

**Evidence:**
- `telemetry.rs:29–44` — `TelemetryError` never carries field values, only field names.
- `telemetry.rs:110–135` — `UsageEvent`: all text fields are opaque identifiers; no task text, no absolute path, no memory body.
- `telemetry.rs:164–175` — `validate()`: checks every text field with `check_opaque` (or `check_workspace_id` for `workspace_id`).
- `telemetry.rs:214–228` — `check_opaque`: allows only `[A-Za-z0-9_.:-]`, max 128 bytes. Rejects `/`, spaces, newlines, control chars.
- `telemetry.rs:190–211` — `check_workspace_id`: allows `/` as a namespace separator but rejects leading `/`, `//`, empty segments, and `..` traversal. Accepts `swal/{app_id}/{instance_id}` namespaces.
- `telemetry.rs:292–329` — `record`: validates before writing; dry-run reports without writing; retries on `SQLITE_BUSY`/`SQLITE_LOCKED` with bounded backoff (25/50/100/200 ms).
- `telemetry.rs:333–355` — `insert_event`: `INSERT OR IGNORE` for idempotency on `event_id`.
- `telemetry.rs:358–385` — `prune`: deletes only events older than the retention cutoff; dry-run reports without deleting.
- `skills.rs:248–297` — `usage_summary_blocking`: returns aggregate `GROUP BY skill_name` counts; no individual task IDs, workspace IDs, or event IDs.
- `skill_dispatcher.rs:258–269` — `dispatch_event`: `task_id` is a SHA-256 hash of the task text (first 16 hex chars), not the raw text.
- Tests: `telemetry.rs:620–645` (workspace_id accepts namespaces, rejects paths), `telemetry.rs:647–681` (untrusted task text, absolute paths, free-text skill names rejected), `telemetry.rs:683–697` (unknown stage rejected by schema CHECK), `skills.rs:532–561` (usage summary leaks no private fields).

**Verdict:** Only bounded opaque identifiers reach the telemetry table. Task text is hashed; workspace IDs are namespace-validated; the usage endpoint returns aggregates only. No credential, task text, or absolute path can be stored or logged.

---

### 8. Migration script safety — PASS

**Evidence:**
- `skills-migrate-batch.sh:47–50` — `done` file prevents re-execution of an already-applied batch.
- `skills-migrate-batch.sh:54` — `xavier skills plan` generates the plan (write-free by design, `planner.rs:1–9`).
- `skills-migrate-batch.sh:56–60` — dry-run is the default; `--execute` is required to mutate.
- `skills-migrate-batch.sh:62` — destination paths extracted from the plan JSON via `jq`.
- `skills-migrate-batch.sh:64–80` — backup created before any write; empty tar if no targets.
- `skills-migrate-batch.sh:82–85` — apply runs with `set +e` so failures are captured, not fatal.
- `skills-migrate-batch.sh:90–97` — post-apply verification: every destination must exist.
- `skills-migrate-batch.sh:99–112` — on failure: rollback via `xavier skills rollback`; if rollback fails or reports conflicts, restore from tar backup.
- `skills-migrate-batch.sh:114` — `done` file touched only after success.
- **No auto git push** — the script never invokes `git`.
- **Never deletes prior stores** — the script only creates a backup tar; it does not delete any existing store.
- `tar` runs without inherited `TAR_OPTIONS`, so symlinks are archived as symlinks (F-03, resolved).

**Verdict:** The script follows the ADR's backup-before-write, rollback-on-failure, never-delete-prior-stores requirements. The `done` file provides idempotency. 

---

## Guard Script Coverage

The `skill-controller-guards.sh` static guards are consistent with the implementation:

| Guard | What it enforces | Status |
|-------|-----------------|--------|
| `guard_01` | No MCP tool mutates skills (only list/dispatch/usage allowed) | **PASS** — `tools_context.rs` contains only the three read-only skill tools; no others match `xavier_.*skill.*`. |
| `guard_02` | `/api/skill/audit` and `/api/skill/usage` take no request extractors | **PASS** — `skills.rs:441` `audit_skills()` and `skills.rs:299` `skill_usage()` have empty parameter lists. |
| `guard_03` | No `.unwrap()`/`.expect()` in `src/context/skill_controller/` outside `#[cfg(test)]` | **PASS** — all controller modules use `Result`/`thiserror`; no panicking calls in production code. |
| `guard_04` | No `std::process::Command` in `src/context/skill_controller/` | **PASS** — the controller performs filesystem operations directly, not via shell. |
| `guard_05` | No `async fn` in `src/context/skill_controller/` (blocking calls go through `spawn_blocking`) | **PASS** — all controller modules are synchronous; callers wrap in `spawn_blocking` (e.g., `skills.rs:300`, `cli/handlers/skills.rs:57`, `cli/commands/mod.rs:614`). |

---

## Summary Table

| Area | Status | Evidence |
|------|--------|----------|
| 1. Manifest trust | **PASS** | `manifest.rs:248–383` validation; `policy.rs:100–104` canonical roots; `cli/handlers/skills.rs:69–90` policy from manifest only |
| 2. Race-resistant paths (TOCTOU, symlink, `..`) | **PASS** (F-04 info) | `policy.rs:115–197` authorize+revalidate; `projector.rs:245–259` atomic swap; `lease.rs:76–93` mkdir+vacant; `backup.rs:219–279` atomic_write |
| 3. Journal recovery (crash mid-apply, atomic rename) | **PASS** | `state.rs:261–288` WAL; `projector.rs:457–506` fault test; `rollback.rs:71–176` precise restore; `backup.rs:219–279` atomic rename |
| 4. Package secrets refused before publication | **PASS** | `policy.rs:279–333` scan_package; `projector.rs:167–170` blocks on finding; `policy.rs:214–225` redacted display; `rules.rs:68–71` extra patterns |
| 5. Prompt/evidence injection | **PASS** | `composer.rs:283–306` scrub+quote; `composer.rs:308–326` normalize; `composer.rs:340–343` fence scrub; `composer.rs:349–354` override drop; `composer.rs:356–366` safe_origin |
| 6. REST/MCP read-only boundaries | **PASS** | `skills.rs:18–26` read-only routes; `tools_context.rs:100–126` read-only tools; `cli/commands/mod.rs:555–557` CLI-only mutation; `cli/server.rs:1838–1845` auth middleware; guards 01–02 |
| 7. Telemetry privacy | **PASS** | `telemetry.rs:164–228` opaque validation; `telemetry.rs:292–385` record+prune; `skills.rs:248–297` aggregates only; `skill_dispatcher.rs:258–269` task hash |
| 8. Migration script safety | **PASS** | `skills-migrate-batch.sh:47–50` done file; `:64–80` backup; `:90–97` verify; `:99–112` rollback; `:114` done on success; no git push; no store deletion |

---

## Verification record (2026-10-12, `main` 59a29d97)

- `xavier exec "bash scripts/check-secrets.sh"` → `No obvious secrets in tracked files` (2570 files, grep fallback; gitleaks also runs in CI).
- `bash scripts/skill-controller-guards.sh` → PASS guard_01–guard_05.
- `cargo test -p xavier --features ci-safe --lib -- --test-threads=1 skill_controller spawn` → 116 passed, 0 failed.
- `bash scripts/tests/skills-migrate-batch-test.sh` → all cases passed, including `test_backup_symlink_does_not_dereference`.
- `cargo clippy -p xavier --all-targets --features ci-safe -- -D warnings` → clean.
