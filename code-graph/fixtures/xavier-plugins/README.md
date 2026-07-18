# Xavier plugin registry fixtures

In-repo **canonical fixtures** for the code-graph plugin registry (`RegistryEntry` / `PlatformEntry` in `code-graph/src/plugin/types.rs`).

## Files

| File | Purpose |
|---|---|
| `plugins.json` | Sample registry index with `parser-python` + a plugin-backed `Other("ruby")` language |
| `plugins.schema.json` | JSON Schema Draft 2020-12 documenting required fields |
| `README.md` | This file |

## Production override

These fixtures are **not** the live catalog. Production (or lab) deployments may point the registry client at a remote index, for example:

```text
https://raw.githubusercontent.com/swal/xavier-plugins/main/plugins.json
```

Override via the engine/registry configuration (or env) used by your deployment — do not hard-code the fixture path into production binaries.

Checksums in this fixture use the `sha256:<64 hex>` form expected by download verification. URLs under `example.invalid` are placeholders and must not be fetched in CI without a local mock.

## Tests

```bash
cargo test -p code-graph registry_fixture
```

The unit test loads `plugins.json` and deserializes each entry into `RegistryEntry`.
