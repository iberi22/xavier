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
