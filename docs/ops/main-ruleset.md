# Main Branch Ruleset

This document describes the `main-protection` GitHub ruleset applied to the `~DEFAULT_BRANCH`.

## What it Enforces
- **Deletion**: Prevents deletion of the branch.
- **Non-fast-forward**: Prevents force pushing.
- **Pull Request**: Requires a PR (0 approvals required, dismiss stale reviews off).
- **Required Status Checks**: Requires `CI Gate` and `Shadow Fast Gate` (the job name GitHub reports; the workflow is named `Fast Gate (Shadow)`).

## Dry-Run and Apply
To test what changes will be applied without mutating the API:
```bash
bash scripts/apply-main-ruleset.sh
```

To apply the ruleset (using the `evaluate` enforcement by default) and remove subset duplicates (`r1`, `ruleone`):
```bash
bash scripts/apply-main-ruleset.sh --execute
```

To enforce the ruleset immediately (sets enforcement to `active`):
```bash
bash scripts/apply-main-ruleset.sh --execute --enforce
```

To enable auto-merge on the repository:
```bash
bash scripts/apply-main-ruleset.sh --execute --enable-auto-merge
```

## Roll Back
The script automatically saves the previously existing ruleset JSON files into the `.cache/rulesets/` directory before overwriting or deleting them.

To roll back a deletion (e.g. `r1`):
```bash
gh api -X POST repos/iberi22/xavier/rulesets --input .cache/rulesets/r1-previous.json
```

To roll back `main-protection` (e.g., if you accidentally modified it):
```bash
gh api -X PUT repos/iberi22/xavier/rulesets/<ID> --input .cache/rulesets/main-protection-previous.json
```

## Owner Emergency Bypass
The ruleset is configured with `bypass_actors` allowing the repository admin role (Role ID: 5) to bypass rules. Only the owner keeps an emergency bypass.
