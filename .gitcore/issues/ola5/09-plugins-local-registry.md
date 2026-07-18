# [Ola 5 · 09] Plugin registry: in-repo plugins.json fixture + schema

> Advances #489 without requiring org create rights in Jules sandbox

## Exact Technical Context
- `code-graph/src/plugin/registry.rs` expects RegistryIndex shape
- Add `code-graph/fixtures/xavier-plugins/plugins.json` + schema matching RegistryEntry
- Optional env `XAVIER_PLUGIN_REGISTRY_URL` defaulting to fixture file:// or mock HTTP in tests
- Document that live `swal/xavier-plugins` still needed for production

## Acceptance Criteria
- [ ] Valid plugins.json for parser-python placeholder entry
- [ ] plugins.schema.json
- [ ] Test loads index via Mock/fixture
- [ ] cargo test plugin registry paths
- [ ] DO NOT claim live GitHub org created unless actually done

## Merge order
Parallel with 10.
