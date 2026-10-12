# Skill Controller Migration Guide

This document defines the procedure for migrating existing skill stores and tool configurations to the unified Skill Controller, ensuring safe atomic swaps and providing a deterministic rollback path.

## Preconditions

Before initiating the migration procedure, ensure the following preconditions are met:

- Xavier MUST be stopped for the CLI checks before and after each batch. Because skill packages are static files, the migration can be safely performed offline.
- Run the Xavier-stopped static guards to ensure invariants hold before and after each batch:

```bash
bash scripts/skill-controller-guards.sh
```

## Migration Steps

Migrations are executed in bounded batches. Each batch must contain ≤ 4 package files and ≤ 400 changed lines, repeated until every inventoried source is accounted for.

### 1. Inventory

Before migrating any files, generate a comprehensive projection plan and inventory report using the Xavier Skill Controller CLI across all declared tool roots in the manifest.

### 2. Full-package hash

Capture a full-package hash of the source files to verify integrity and detect tampering.

### 3. Secret quarantine

Scan for and quarantine any identified secrets in the target configurations.

### 4. Divergent-name review

Perform a divergent-name review to ensure all projected names align correctly.

### 5. Independent backup

Create an independent backup of the skill directory. Note that automatic `git push` is forbidden during the active migration window to prevent remote inconsistencies. Never delete prior stores during the migration.

### 6. Preview

Preview the batch execution using a dry-run.

```bash
bash scripts/skills-migrate-batch.sh --manifest manifest-batch-1.toml --batch 1 --state "$HOME/.xavier/migration-state"
```

### 7. Apply

Apply the batch changes.

```bash
bash scripts/skills-migrate-batch.sh --manifest manifest-batch-1.toml --batch 1 --state "$HOME/.xavier/migration-state" --execute
```

### 8. Verification

Verify the migration was successful by checking the destination paths and running the static guards again:

```bash
bash scripts/skill-controller-guards.sh
```

### 9. Rollback

If a migration batch fails or introduces unexpected regressions, immediately execute the rollback procedure.

### 10. Retention

Following a successful migration, retain the previous stores and backups. Deletion of prior stores is strictly forbidden.

## Adapter Compatibility

The following table tracks the discovery root and status for each adapter based on measured compatibility tests.

| Adapter | Discovery Root | Project Root | Measured Result | Status |
|---|---|---|---|---|
| OpenClaw | `$HOME/.openclaw/skills` | `<project>/.agents/skills` | Symlinks followed successfully | `verified` |
| Codex | `$HOME/.agents/skills` | `<project>/.agents/skills` | Symlinks followed successfully (only verified by the ignored native test run with --ignored) | `verified` |
| Hermes | `$HOME/.hermes/skills` | `<project>/.agents/skills` | Hermes launcher attempts dependency downloads in offline startup; native discovery unverified | `unsupported` |
| Claude Code | `$HOME/.claude/skills` | `<project>/.claude/skills` | Claude Code 2.1.296 has no verified offline catalog command; authenticated model calls are not hermetic | `unsupported` |
| OpenCode | `$HOME/.config/opencode/skills` | `<project>/.agents/skills`, `<project>/.claude/skills` | OpenCode 0.26 debug has no skill subcommand; native discovery unverified | `unsupported` |
| agy | `$HOME/.gemini/config/skills` | `<project>/.agents/skills` | installed agy wrapper ignores HOME and writes real account cache; unsafe to launch hermetically | `unsupported` |
| Gemini CLI | `$HOME/.gemini/skills` | `<project>/.agents/skills` | gemini not installed | `unavailable` |
