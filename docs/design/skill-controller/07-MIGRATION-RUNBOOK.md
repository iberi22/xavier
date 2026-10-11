# Skill Controller — Migration and Rollback Runbook

This document defines the procedure for migrating existing skill stores and tool configurations to the unified Skill Controller, ensuring safe atomic swaps and providing a deterministic rollback path.

## Preconditions

Before initiating the migration procedure, ensure the following preconditions are met:

1. **Backups:** A complete tarball backup of the current skill directory must exist. Create a full archive of the existing tool roots.
2. **Environment State:** Xavier may be stopped during this process. Because skill packages are static files, the migration can be safely performed offline.
3. **Permissions:** Ensure the executor has appropriate filesystem permissions to create symlinks and rename directories in `$HOME`.
4. **Environment Variables:** The required `XAVIER_SKILL_*` variables must be exported or available in the `.env` file, matching the desired target state.
5. **Disk Space:** Verify sufficient disk space for the canonical store plus any temporary backup copies generated during the migration.
6. **No Active Development:** Communicate to the team that direct edits to the existing skill directories should be paused during the migration.

## Inventory

Before migrating any files, generate a comprehensive inventory of all discovered skill packages across known tool roots.

Run the inventory tool:

```bash
python3 scripts/skills-inventory.py --all-roots > inventory-report.json
```

**How to read conflicts:**
The JSON report will highlight duplicate skill names, version mismatches, and symlink loops.
- `duplicate_names`: Look for distinct skill directories attempting to claim the same command namespace.
- `unresolved_links`: Any symlinks that point outside authorized boundaries or are dangling.
- `manifest_drift`: Differences between the proposed TOML manifest and the observed filesystem state.

Resolve any identified conflicts in the `inventory-report.json` manually before proceeding to the batch migration phase.

## Go/No-go checklist

Evaluate this checklist (mapping to task O05) to determine if the migration can safely proceed:

- [ ] Inventory script (`scripts/skills-inventory.py`) completed without fatal errors.
- [ ] No unresolvable conflicts identified in the inventory report.
- [ ] Tarball backup of all tool roots has been successfully created and verified.
- [ ] Required tools are available and executable on `$PATH`.
- [ ] Target canonical store directory exists or can be created.
- [ ] Dry-run plan outputs match the expected placement for all packages.
- [ ] The rollback procedure has been reviewed and understood by the operator.
- [ ] Adapters that are critical for current operations have been tested (status `measured`).

## Batch procedure

To minimize risk, migrations are executed in bounded batches. One batch is defined as exactly one discovery root or a maximum of 25 packages.

1. **Define the batch:** Identify the target tool root (e.g., `$HOME/.hermes/skills`).
2. **Dry-run the batch migration:**
   Execute the migration script in dry-run mode to inspect the proposed changes.

   ```bash
   bash scripts/skills-migrate-batch.sh --dry-run --batch-size 25 --root "$HOME/.hermes/skills"
   ```

3. **Review the plan:** Ensure no regular files are scheduled for deletion, only atomic symlink swaps.
4. **Execute the batch:**
   Run the migration script to apply the changes.

   ```bash
   bash scripts/skills-migrate-batch.sh --apply --batch-size 25 --root "$HOME/.hermes/skills"
   ```

5. **Verify the batch:** Check that the symlinks were created correctly and point to the unified store.

## Verification per adapter

The following table tracks the discovery root and testing status of each tool adapter. Adapters marked as `pending` have not yet been measured live. Do not proceed with migration for an adapter unless its status is `measured`.

| Adapter | Discovery Root | Measured Result | Status |
|---|---|---|---|
| Hermes | `$HOME/.hermes/skills` | Symlinks followed successfully | `measured` |
| OpenClaw | `$HOME/.openclaw/skills` | Symlinks followed successfully | `measured` |
| Gemini CLI | `$HOME/.config/gemini/skills` | Not yet measured | `pending` |
| Codex | `$HOME/.codex/skills` | Not yet measured | `pending` |
| Claude Code | `$HOME/.claude/skills` | Not yet measured | `pending` |
| agy | `$HOME/.agy/skills` | Not yet measured | `pending` |

## Rollback

If a migration batch fails or introduces unexpected regressions, immediately execute the rollback procedure. The controller architecture is designed for a fail-safe, deterministic rollback.

1. **Controller rollback:**
   Use the Xavier CLI to revert the ownership journal and remove projected symlinks for the affected batch.

   ```bash
   xavier skills rollback --batch-id <failed_batch_id>
   ```

2. **Tarball restore:**
   If the controller rollback fails or is incomplete, manually restore the original tool roots from the tarball backup created in the Preconditions step.

   ```bash
   tar -xzf skills-backup.tar.gz -C "$HOME"
   ```

3. **Verify rollback:**
   Confirm that the tool roots have been restored to their exact pre-migration state.
   Run the inventory tool again to ensure no drift remains.

   ```bash
   python3 scripts/skills-inventory.py --all-roots
   ```

## Copy fallback

This fallback strategy is reserved exclusively for adapters that strictly require real files and exhibit a measured failing symlink test.
As per current testing, there are no adapters requiring this fallback (none so far).

If an adapter is discovered that cannot follow symlinks:
1. Reconfigure the manifest for that specific adapter to use a `copy` strategy instead of `symlink`.
2. Ensure the controller tracks the copied files for future updates.
3. Keep the use of this fallback to an absolute minimum.

## Telemetry & drift checks after migration

Once the migration is complete and the system is operating normally, perform ongoing telemetry and drift checks to ensure system integrity.

1. **Audit for drift:**
   Periodically run the audit command to detect any manual, out-of-band modifications to the tool roots or canonical store.

   ```bash
   xavier skills audit --report-only
   ```

   Review the generated report for unauthorized changes.

2. **Verify usage telemetry:**
   Use the usage command to confirm that migrated skills are being successfully selected, delivered, and invoked.

   ```bash
   xavier skills usage --summary
   ```

## Ownership and retention of previous stores

Following a successful migration and a stabilization period (e.g., 30 days):

1. **Ownership Transition:** The Skill Controller is now the definitive source of truth and sole mutation surface for the skill catalog. All future modifications must occur via the controller.
2. **Retention Policy:** The original tarball backups of the previous stores must be retained for at least 60 days before being safely archived or deleted.
3. **Clean-up:** Any old discovery roots that are no longer targeted by the manifest and have safely transitioned can be removed from `$HOME` after the retention period expires.
