# [Ola 5 · 10] Plugin template: parser-python stdin/stdout JSON

> #489 first plugin template (in monorepo)

## Exact Technical Context
- Protocol: PluginRequest/PluginResponse in code-graph/src/plugin/types.rs
- Add `plugins/parser-python/` or `code-graph/plugins/parser-python/` with plugin.py using tree-sitter if available OR stub symbols for health+parse skeleton
- health operation returns Success

## Acceptance Criteria
- [ ] plugin.py implements health + parse skeleton
- [ ] README with run instructions
- [ ] Example request/response JSON fixtures
- [ ] No broken workspace cargo check

## Merge order
After 09 preferred.
