# Xavier Training Bundle Format

The training bundle is a JSON file containing anonymized telemetry data, deterministically split into training and evaluation sets, along with a manifest and an audit summary.

## Schema Version 1.0.0

### Top-level Structure

- `manifest`: Metadata about the bundle generation.
- `train_split`: A list of anonymized telemetry records for training.
- `eval_split`: A list of anonymized telemetry records for evaluation.
- `audit_summary`: Statistics about the records included and excluded.

### Manifest Fields

- `schema_version`: Version of the bundle schema (e.g., "1.0.0").
- `generated_at`: ISO 8601 timestamp of bundle generation.
- `anonymized_sources`: List of 16-character hex strings representing the anonymized source IDs (wallets) included in the bundle.
- `consent_policy`: The policy under which data was collected.
- `revocation_policy`: Information on how users can revoke their data.
- `usage_policy`: Permitted uses for the data in this bundle.
- `seed`: The seed used for deterministic shuffling and anonymization.

### Anonymization Policy

Source wallet addresses are anonymized using a SHA-256 hash keyed with the generation seed. The resulting hash is truncated to 16 characters. This ensures:
1.  **Consistency**: Multiple bundles generated with the same seed will have consistent IDs for the same source.
2.  **Irreversibility**: Without the original wallet address and the seed, it is computationally infeasible to recover the source ID.
3.  **Privacy**: No personally identifiable information (PII) is included in the bundle.

### Usage in Fine-Tuning

The `train_split` and `eval_split` are ready to be consumed by training scripts. In a Google Colab or similar environment, you can load the JSON bundle and access the splits directly:

```python
import json

with open('training_bundle.json', 'r') as f:
    bundle = json.load(f)

train_data = bundle['train_split']
eval_data = bundle['eval_split']

print(f"Loaded {len(train_data)} training and {len(eval_data)} eval records.")
```

## Reproducibility

To reproduce a specific bundle, use the same source telemetry database and the same seed. The `ChaCha8` RNG ensures that the shuffling and train/eval split are deterministic across different platforms.

## Bundle directory (MX-03)

`write_bundle_to_dir` writes `<data_dir>/<id>/`:

- `bundle_manifest.json`: manifest fields above plus `clearance`, `language`, `segment`,
  `privacy_level` ("P0".."P4"), `local_only` (true for P4), `sources`, `workspace`, `domain`.
- `train.jsonl`, `eval.jsonl`: one record per line.
- `anonymization_audit.json`: see below. Consumers (e.g. `scripts/training/expert_core.py`)
  refuse P4/`local_only` bundles for remote backends and require `"passed": true` for P3.

### Sources

`TrainingExporter` composes `TrainingSource` implementations per request
(`src/data_commons/sources/`):

| name | reads | record |
|------|-------|--------|
| `telemetry` | Data Commons telemetry DB (decrypted logs) | original payload + `metadata` |
| `memory` | memory store, one workspace, optional `kinds`/`namespace`/`domain` | `{instruction, response, metadata}` |
| `challenge` | HumanChallenge `curation_votes` with `training_eligible=1` (accept/refine) | `{instruction, response, metadata}` |

Memory pairing: response = memory content (<= 4000 chars); instruction is synthesized from
`metadata.title`, else `metadata.summary`, else the path, using a per-kind template
(decision, file/symbol/repo/branch/harness, task, procedural, default). Soft-deleted, encrypted,
clearance above Internal, label-less and too-short (< 20 chars) memories are skipped and counted.
Challenge pairing: instruction = event description, response = `curated_content` else the
event response; events flagged `privacy_p4_local_only` are only exported at P4.

### Privacy

- P0/P1/P2: PII scrubbed (emails, paths, API keys, wallets, owner names). P3: scrub plus Laplace
  noise on numeric fields (epsilon configurable, default 1.0). P4: raw, local only.
- k-anonymity (default k=5, configurable): records are grouped by the quasi-identifiers
  `source, kind, domain, workspace, namespace` read from `record.metadata` (missing = `*`);
  every group must hold at least k records.

### `anonymization_audit.json`

```json
{
  "schema_version": "1.0.0", "generated_at": "RFC3339",
  "privacy_level": "P3", "local_only": false,
  "k": 5, "epsilon": 1.0, "dp_applied": true,
  "sources": ["memory"], "workspace": "ws1", "domain": null,
  "counts": {"total_found": 8, "included": 8, "train": 6, "eval": 2,
             "excluded_no_consent": 0, "excluded_revoked": 0, "excluded_other": {"memory:content_too_short": 1}},
  "scrubbed": {"emails": 0, "paths": 0, "api_keys": 0, "wallets": 0, "entities": 0},
  "residual_pii": {"emails": 0, "paths": 0, "api_keys": 0, "wallets": 0, "entities": 0},
  "numeric_fields_noised": 0,
  "k_anonymity": {"k": 5, "quasi_identifiers": ["source","kind","domain","workspace","namespace"],
                  "equivalence_classes": 1, "min_class_size": 8, "violations": [],
                  "violating_records": 0, "satisfied": true},
  "passed": true, "reasons": []
}
```

`passed` means "may leave the node". It is false (with `reasons`) for P4 (`local_only` true), empty
bundles, residual PII, k-anonymity violations, or a non-positive epsilon at P3.

### HTTP (`/v1/training/*`)

`POST /v1/training/bundles` and `POST /v1/training/export` accept, besides `seed`/`eval_ratio`:
`sources` (default `["telemetry"]`), `workspace` (required for `memory`), `domain`,
`privacy_level` (default P2), `kinds`, `namespace`, `k`, `epsilon`.
`GET /v1/training/bundles/{id}/audit` returns the audit file.
