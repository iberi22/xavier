# Skill Controller — Codebase Impact Analysis

Scope: read-only analysis for the design phase. No production code changed.

## (a) Files to CREATE / EDIT / REFACTOR

| File | Action | Reason | Size |
|---|---|---|---|
| `src/context/skill_projector.rs` | CREATE | Projector: manifest -> symlink reconcile (idempotent, dry-run, backup, never delete regular files). No write path exists today. | M |
| `src/context/skill_manifest.rs` | CREATE | Declarative placement type: `canonical_store`, per-tool/per-project target dirs, allowlist. Parsed with existing `serde_yaml`. | S |
| `src/context/skill_ephemeral.rs` | CREATE | Ephemeral composer: `<worktree>/.agents/skills/eph-<task>/SKILL.md` + `.claude/skills` link from registry + memories + Code-Graph; cleanup on task close. | M |
| `src/context/skill_audit.rs` | CREATE | Drift audit: dead paths/ports/endpoints, version contradictions, missing frontmatter, cross-skill contradictions, secret-pattern scan -> report + diff proposal only. | M |
| `src/context/skill_telemetry.rs` | CREATE | Dispatch/invocation log (skill, task hash, confidence, timestamp) to drive pruning. Reuses existing telemetry DB path convention. | S |
| `src/context/mod.rs` | EDIT | Register 5 new modules + re-exports (current registry: `src/context/mod.rs:18-20,43-47`). | S |
| `src/context/skill_registry.rs` | REFACTOR | Make scan paths configurable (see §c). Keep `reindex`, `search*`, `embed_missing` untouched. | S |
| `src/api/skills.rs` | EDIT | Add `POST /api/skill/project` (reconcile), `GET /api/skill/drift`, `GET /api/skill/usage`; keep `dispatch_skill` (`src/api/skills.rs:61`), `list_skills` (`src/api/skills.rs:180`) byte-compatible. | M |
| `src/server/mcp/tools_context.rs` | EDIT | Add `xavier_skill_project` (dry-run default), `xavier_skill_ephemeral_compose`, `xavier_skill_drift_report`, `xavier_skill_usage`. Keep `xavier_dispatch_skill` (`src/server/mcp/tools_context.rs:99-111`), `xavier_skill_list` (`src/server/mcp/tools_context.rs:112-119`) schemas untouched. | M |
| `src/cli/server.rs` | EDIT | Register new REST routes next to existing skill routes (`src/cli/server.rs:1050-1059`). No auth/layer changes. | S |
| `src/cli/commands/enums.rs` | EDIT | Add `Skill { Project/Reconcile, Ephemeral, Drift, Usage }` subcommand enum; `Command` enum pattern at `src/cli/commands/enums.rs:33-480`. Alternative: extend `ScanCommand` (`src/cli/commands/enums.rs:1374-1392`). | S |
| `src/cli/commands/spawn.rs` | EDIT | Hook ephemeral compose before agent spawn; `load_skill` (`src/cli/commands/spawn.rs:265-279`) stays as fallback reader. | S |
| `src/server/http/context.rs` | EDIT | Feed ephemeral composition into Maximum-level fusion alongside existing dispatch (`src/server/http/context.rs:177-245`); Minimal/Medium byte-identical. | S |
| `.env.example` | EDIT | Add `XAVIER_SKILL_*` vars (see §c). | S |
| `.gitcore/features.json` | EDIT (via wave PRs only) | New ledger entries `feat-skill-projector`, `feat-skill-ephemeral`, `feat-skill-drift-audit`, `feat-skill-telemetry` as `planned`; never hand-promote status. | S |
| `Cargo.toml` | EDIT only if needed | Prefer zero new deps (std symlink + `walkdir` + `serde_yaml` already present). Add nothing unless audit needs a secret-pattern crate (use `regex`, already at `Cargo.toml:338`). | S |
| `src/context/skill_dispatcher.rs` | NO CHANGE | Read-only reuse; thresholds stay. | — |
| `src/context/skills.rs` | NO CHANGE | Keep fixed validator (`src/context/skills.rs:64-68`); do not fork a second frontmatter parser. | — |
| `src/context/builder.rs`, `src/context/pipeline.rs` | NO CHANGE | Reuse `build` (`src/context/builder.rs:43`), `regenerate_context` (`src/context/pipeline.rs:131`); fusion already feeds them. | — |

No other new services, daemons, DSLs, or plugin systems (brief constraint).

## (b) Existing functions/types to REUSE

- Walker: `collect_skill_files` (`src/context/skill_registry.rs:185-241`) — symlink-cycle-safe via `dir_identity` (`src/context/skill_registry.rs:174-183`), file-symlink following, `is_skill_file_name` (`src/context/skill_registry.rs:150-155`). Projector MUST reuse this traversal, plus an allowlist of target dirs.
- Frontmatter: `parse_frontmatter` (`src/context/skill_registry.rs:716-740`) + validators `is_valid_skill_slug` (`src/context/skill_registry.rs:244-250`), `is_valid_description` (`src/context/skill_registry.rs:253-255`); legacy `SkillLoader::load_all` / `validate_skill` (`src/context/skills.rs:30-68`) and `frontmatter_field` (`src/context/skills.rs:72-90`). Audit and ephemeral composer reuse these; no new parser.
- Embeddings/rank: `skill_embed_text` (`src/context/skill_registry.rs:618-621`), `cosine_similarity` (`src/context/skill_registry.rs:625-641`), `rank_vectors` (`src/context/skill_registry.rs:652-676`), `search` (`src/context/skill_registry.rs:496-513`), `search_with_vector` (`src/context/skill_registry.rs:519-539`), `search_semantic` (`src/context/skill_registry.rs:544-567`, `spawn_blocking` at `src/context/skill_registry.rs:557`), `reindex_with_embeddings` (`src/context/skill_registry.rs:414-444`), `embed_missing` (`src/context/skill_registry.rs:447-474`), `EmbeddingPort::embed` (`src/ports/outbound/embedding_port.rs:24-32`).
- Dispatcher: `SkillDispatcher::dispatch` (`src/context/skill_dispatcher.rs:75-167`, `MIN_DISPATCH_CONFIDENCE = 0.40` at `src/context/skill_dispatcher.rs:79-85`), `gather_context` (`src/context/skill_dispatcher.rs:170-229`), `IndexedSkill::compacted_content` (`src/context/skill_registry.rs:43-75`, UNTRUSTED boundary), `ContextPack` (`src/context/skill_dispatcher.rs:51-60`).
- MCP registration: `get_xavier_context_tools` (`src/server/mcp/tools_context.rs:22-121`), `handle_context_tool` (`src/server/mcp/tools_context.rs:136-517`), `build_skill_registry` (`src/server/mcp/tools_context.rs:125-133`); fail-open error shape (`src/server/mcp/tools_context.rs:479-487`).
- REST: `dispatch_skill` (`src/api/skills.rs:61-113`), `list_skills` (`src/api/skills.rs:180-205`), routes (`src/cli/server.rs:1050-1059`).
- CLI wiring: `Command` enum (`src/cli/commands/enums.rs:33-480`), `ScanCommand` (`src/cli/commands/enums.rs:1374-1392`), `VerifyCommand::Features` (`src/cli/commands/enums.rs:762-770`), `load_skill` fallback (`src/cli/commands/spawn.rs:265-279`).
- Fusion gates: `SKILL_CONFIDENCE_THRESHOLD = 0.5` (`src/server/http/context.rs:15`), `is_trivial_prompt` (`src/server/http/context.rs:19-67`), Maximum-only injection (`src/server/http/context.rs:181-245`).
- Config/env pattern: `home_dir` (`src/context/skill_registry.rs:89-93`), `default_scan_paths` (`src/context/skill_registry.rs:106-115`), `with_defaults`/`with_home` (`src/context/skill_registry.rs:275-290`), `load_disabled_from_config` (`src/context/skill_registry.rs:119-147`). New `XAVIER_SKILL_*` vars follow the `std::env::var` + `.env.example` pattern in §c.
- Deps already in `Cargo.toml`: `walkdir` (`Cargo.toml:321`), `serde_yaml` (`Cargo.toml:364`), `sha2` (`Cargo.toml:343`), `regex` (`Cargo.toml:338`), `tempfile` dev (`Cargo.toml:388`).

## (c) What must be refactored FIRST

1. **Configurable skill store (12-factor).** `hermes_store_path` (`src/context/skill_registry.rs:96-98`), `hermes_config_path` (`src/context/skill_registry.rs:101-103`), and `default_scan_paths` (`src/context/skill_registry.rs:106-115`) hardcode `$HOME/.hermes/skills` and `$HOME/.hermes/config.yaml` (only `HOME` itself is env-resolved, `src/context/skill_registry.rs:89-93`). Before any projector work: add env overrides with current behavior as default, document in `.env.example` in the same PR (AGENTS.md §7), keep `with_home` as the testable seam:
   - `XAVIER_SKILL_CANONICAL_STORE` (default `$HOME/.hermes/skills`)
   - `XAVIER_SKILL_CONFIG` (default `$HOME/.hermes/config.yaml`, disabled list source)
   - `XAVIER_SKILL_SCAN_PATHS` (colon-separated extra roots; empty = defaults)
   - `XAVIER_SKILL_TARGET_ALLOWLIST` (colon-separated dirs the projector may write; default: per-tool global dirs + `<workspace>/.agents/skills` + `<repo>/skills` link targets only)
   - `XAVIER_SKILL_TELEMETRY_DB_PATH` (default: reuse `XAVIER_TELEMETRY_DB_PATH` when set)
   - `XAVIER_SKILL_DRY_RUN` (default `1`; projector writes require explicit `0`/flag)
2. **Single construction site.** `SkillRegistry::with_defaults` + `reindex` is duplicated in `src/api/skills.rs:69-71,184-185`, `src/server/mcp/tools_context.rs:125-133`, `src/server/http/context.rs:196-198`. Extract one `build_skill_registry(workspace_root)` helper (in `skill_registry.rs`) and call it from all three; eliminates ranking drift between REST/MCP/fusion.
3. **Unify dispatch thresholds.** `MIN_DISPATCH_CONFIDENCE = 0.40` (`src/context/skill_dispatcher.rs:79`) vs `SKILL_CONFIDENCE_THRESHOLD = 0.5` (`src/server/http/context.rs:15`) is intentional (dispatch vs fusion) but undocumented. Promote both to named, documented constants before telemetry compares "dispatched" vs "injected" counts.
4. **Do not touch yet:** ranking math, `compacted_content` budgets, MCP/REST response shapes, `SkillLoader` validator.

## (d) Tests: exist vs to add

Exists (must stay green; ledger `stable` in `.gitcore/features.json:2396-2575`):
- Registry: `parses_yaml_frontmatter`, `infers_domains_from_content`, `scores_skill_match`, `test_registry_scans_hermes_canonical_store`, `test_registry_symlink_cycle_terminates`, `test_registry_honors_disabled_list`, `registry_rejects_non_skill_entries`, `registry_rejects_empty_description` (`src/context/skill_registry.rs:776-1153`).
- Semantic: `test_cosine_similarity_properties`, `test_confidence_calibration_range`, `test_ranking_offline_no_network`, `test_keyword_fallback_when_vector_store_empty`, `test_reindex_with_embeddings_embeds_at_index`, `test_semantic_rank_recall_at_3` (`src/context/skill_registry.rs:966-1312`).
- Dispatcher: `context_pack_serializes`, `dispatch_result_serializes`, `dispatch_returns_no_match_below_confidence_threshold`, `dispatch_never_reports_full_confidence_for_weak_match` (`src/context/skill_dispatcher.rs:242-343`).
- Loader: `test_loader_loads_real_skill_format_or_module_removed`, `falls_back_to_parent_dir_name`, `rejects_frontmatter_less_garbage` (`src/context/skills.rs:92-161`).
- Fusion/MCP (by spec): `test_fusion_*` (4), `test_mcp_dispatch_tool_roundtrip`, `test_mcp_skill_list_announced` (FEATURE-feat-skill-context-fusion.md, FEATURE-feat-skill-mcp-tool.md).

To add (per component, `tempfile` fixtures, offline, no `$HOME`/network):
- Projector: cycle-fixture reconcile terminates; idempotence (second run = zero changes); dry-run writes nothing; backup created before overwrite; regular file never deleted; write outside allowlist rejected; broken-symlink skipped.
- Manifest: parses minimal YAML; unknown tool key rejected; absolute-path escape rejected.
- Ephemeral: composes `SKILL.md` under `<worktree>/.agents/skills/eph-<task>/` within budget; cleanup removes only `eph-*` dirs; live-session path returns pack via dispatcher with no files.
- Audit: fixture with dead path, missing `description`, contradictory routing pair, fake secret pattern -> report lists all four, proposes diff, rewrites nothing; secret values never printed.
- Telemetry: dispatch increments per-skill counter; `_none` verdicts recorded; concurrent dispatches do not corrupt counts.
- Config: `XAVIER_SKILL_CANONICAL_STORE` override respected; unset `HOME` fails open (workspace paths only).
- Parity: REST `GET /skills` count == MCP `xavier_skill_list` count on same fixture (extends existing parity test).

## (e) Risks to current stable features + guards

| Risk | Affected stable feature | Guard |
|---|---|---|
| Projector overwrites/deletes user skills | feat-skill-scan-paths | Dry-run default (`XAVIER_SKILL_DRY_RUN=1`); allowlist-only targets; backup before change; never delete regular files; idempotence test. |
| Config refactor changes default scan set, `GET /skills` count drops | feat-skill-scan-paths, feat-skill-mcp-tool | Defaults reproduce current paths exactly; `test_registry_scans_hermes_canonical_store` + MCP/REST parity test on every PR. |
| Shared helper changes ranking between surfaces | feat-skill-semantic-rank, feat-skill-dispatch-confidence (beta) | Single construction site (§c.2); eval `test_semantic_rank_recall_at_3` Recall@3 ≥ 0.8 gate; confidence-calibration test. |
| Fusion behavior change on Maximum level | feat-skill-context-fusion | Minimal/Medium byte-identical; `SKILL_CONFIDENCE_THRESHOLD` unchanged; `is_trivial_prompt` unchanged; 4 `test_fusion_*` green. |
| New MCP tools break existing clients | feat-skill-mcp-tool | Existing tool names/schemas untouched; new tools additive; round-trip tests for old + new tools. |
| Secret scan leaks credentials into reports/logs | new (audit) | Report emits file:line + pattern class only, never matched values; `scripts/check-secrets.sh` (gitleaks) in CI per AGENTS.md §7. |
| Telemetry DB bloat / contention | none existing (new store) | Bounded schema + TTL/prune; default path under existing telemetry config; failure fails open (dispatch never blocks on telemetry write). |
| New deps break headless/CI builds | all (`ci-safe`) | Zero new deps preferred; any addition must keep `cargo test --workspace`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt` green. |
| Ledger drift (specs reference unshipped code) | feat-skill-ledger-docs | New entries start `planned`; status promoted only by green `scripts/verify-pipeline.sh` run, never by hand (AGENTS.md §4-5). |
