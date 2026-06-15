# Fine-tuning Readiness Safety Gates

This document outlines the safety checks that must pass before any shared data can be consumed for model fine-tuning.

## Fine-tuning Remains Blocked
**Fine-tuning remains blocked until these checks pass.** No data should be used for training purposes unless a training bundle has been validated and marked as "READY" by the `xavier data-commons validate` command.

## Required Checks

A training bundle directory must include the following files and satisfy these conditions:

1.  **Anonymization Audit Exists**: A file named `anonymization_audit.json` must be present, indicating that an audit of the anonymization process has been performed.
2.  **Dataset is Reproducible**: The `bundle_manifest.json` must contain a `reproducibility_seed` (non-zero) to ensure deterministic data splits and sampling.
3.  **Consent Metadata is Present**: Every record in the data files (e.g., `.jsonl`) must have `consent_given: true` in its metadata.
4.  **Revoked/Private Records are Excluded**: No records with `is_private: true` or `revoked: true` in their metadata are allowed in the training bundle.
5.  **Train/Eval Splits are Valid**: The manifest must specify non-empty counts for both `train` and `eval` splits.
6.  **Usage Policy is Present**: The `bundle_manifest.json` must contain a non-empty `usage_policy` string.

## Validation Command

To verify a training bundle, use the following CLI command:

```bash
xavier data-commons validate <BUNDLE_PATH>
```

Example:
```bash
xavier data-commons validate ./data/training/my-bundle-v1
```

If any check fails, the command will list the errors and exit with a non-zero status code.
