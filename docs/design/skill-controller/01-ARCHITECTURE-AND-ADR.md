# Skill Controller: architecture and ADR-034 draft

Date: 2026-09-27. Design only; proposed paths, schemas and commands below are not implemented. Discovery assumptions come from `docs/design/skill-controller/00-BRIEF.md:31`; they are not fresh compatibility measurements.

## 1. Decision and existing integration points

Recommend a **local Git source repository outside CLI discovery roots**, selected per-skill symlinks for tool views, and a deterministic controller inside Xavier's application layer. Files remain usable without Xavier. Canonical edits require review; projection and task-local composition operate within an explicitly authorized filesystem policy.

Verified reuse and gaps:

| Existing code | Architectural implication |
|---|---|
| `src/context/skill_registry.rs:105` scans workspace `skills/`, workspace `.agents/skills`, and `$HOME/.hermes/skills`. | Preserve explicit project sources; add manifest-selected roots. The brief omits the first scan root. |
| `src/context/skill_registry.rs:185` tracks directory identities but follows symlinks at `:222`. | Cycle detection is reusable, but it is **not** an allowed-root security boundary. |
| `src/context/skill_registry.rs:350` hashes only the Markdown content; `:408` stores by name. | Controller needs whole-package hashes and explicit duplicate-name resolution before registry construction. |
| `src/context/skill_dispatcher.rs:75` performs keyword dispatch with a 0.40 threshold; semantic search is separately available at `src/context/skill_registry.rs:544`. | Reuse ranking; do not claim current dispatch invokes semantic search. |
| `src/context/skill_dispatcher.rs:109` builds instructions and memory context; `src/context/skill_registry.rs:71` wraps skill text as untrusted. | Reuse retrieval and serialization, with additional scope, provenance and trust controls. |
| HTTP and MCP construct registries independently (`src/api/skills.rs:69`, `src/server/mcp/tools_context.rs:125`). Session fusion does likewise (`src/server/http/context.rs:196`). | Route all three through one controller context builder, keeping existing response compatibility. |

## 2. Components and ownership

All new controller modules belong to the **`xavier` application crate**, proposed under `src/context/skill_controller/`; nothing is added to `crates/xavier-core-logic`. No new daemon, framework or plugin loader. Existing `xavier.service` may schedule reconciliation; the CLI can run it directly offline.

| Component / proposed module | Responsibility and boundary |
|---|---|
| `manifest.rs` | Deserialize versioned TOML into typed sources, targets and placements. Reject unknown fields, ambiguous names and unsafe paths. Produce a normalized desired state. |
| `projector.rs` | Inventory → immutable plan → apply → verify. Per-target locking, ownership journal, backups, atomic entry replacement and rollback. No LLM and no canonical content edits. |
| `composer.rs` | Reuse `SkillRegistry` and `SkillDispatcher`; assemble bounded, provenance-bearing task context from eligible skills, scoped memories and CodeGraph. One renderer serves files and MCP. |
| `audit.rs` | Read-only package validation, projection drift, secret detection, path/version checks and rule-based conflict findings. Produce redacted reports and reviewable safe diffs. |
| `telemetry.rs` | Emit selection, delivery and confirmed-invocation events; calculate coverage-aware usage summaries. No payload collection or automatic pruning. |
| `policy.rs`, `state.rs` | Shared filesystem authorization and transaction/lease state. Persist operational state beneath the existing resolved data directory. |

Thin delivery adapters: proposed `src/cli/handlers/skills.rs` for `xavier skills plan/apply/rollback/compose/audit/usage`; extend existing `src/api/skills.rs` and `src/server/mcp/tools_context.rs` for context delivery and telemetry. Filesystem mutation is initially local CLI/service-only, not exposed through the existing dispatch endpoint.

Reuse existing TOML, hashing and SQLite dependencies (`Cargo.toml:262`, `Cargo.toml:343`, `Cargo.toml:310`). Store controller events in the existing workspace database through the migration framework (`src/storage/mod.rs:26`); store rollback journals/backups under its resolved data directory (`src/settings/mod.rs:123`). Deployment must explicitly set `XAVIER_DATA_DIR` to the canonical runtime location: its current fallback is XDG-based (`src/settings/serialization.rs:37`), whereas ADR-016 prescribes deployment-local `data/` (`docs/adr/016-canonical-data-dir.md:28`). Do not create a second memory database.

**Architectural exception to resolve:** ADR-019 excludes non-storage concerns from core (`docs/adr/ADR-019-plugin-first-boundary.md:54`, `:90`). Merely naming this an application module does not satisfy that rule. ADR-034 proposes a narrow amendment permitting this in-process application controller while preserving storage isolation and external CodeGraph/mesh boundaries. Implementation awaits that amendment; this document does not silently override ADR-019.

### Data flow

```text
owner environment policy + selected manifest + Git/project packages
    → validate paths, metadata, secrets and package hashes
    → desired placements + observed filesystem + ownership journal
    → dry-run plan → authorized apply → tool directories → CLI startup

task + authorized workspace → eligible registry → dispatcher
    + scoped memory references + optional scoped CodeGraph symbols
    → safe bounded composition → worktree files BEFORE agent spawn
                              → existing MCP dispatch for live sessions

packages + projections → drift report / proposed diff → human review
dispatch + delivery + harness invocation acknowledgments → local usage events
```

The orchestrator waits for verified publication before spawning a CLI. In live sessions, use `xavier_dispatch_skill`; do not assume file hot reload (`docs/design/skill-controller/00-BRIEF.md:41`). CodeGraph is optional via the existing UDS client (`src/codebase/codegraph_client.rs:108`); require a workspace-bound endpoint and validate returned relative file references before inclusion. Missing graph/memory context is explicitly marked unavailable, never fabricated.

## 3. Manifest v1

Use **TOML**, separate from skill `SKILL.md` YAML frontmatter. Environment variables select configuration and machine-specific roots; the manifest describes portable placement data, never credentials or shell commands. Proposed variables: `XAVIER_SKILL_MANIFEST`, `XAVIER_SKILL_STORE`, `XAVIER_WORKSPACE_ROOT`, `XAVIER_SKILL_SOURCE_ROOTS`, `XAVIER_SKILL_TARGET_ROOTS`. The last two are owner-supplied JSON arrays of exact authorized roots; repository content cannot widen them. Document all new variables in `.env.example` during implementation.

| Field | Type / rule |
|---|---|
| `version` | Required integer, exactly `1`; unknown versions fail closed. |
| `sources` | Required map of source ID → directory string. IDs are unique slugs. |
| `targets[]` | Required records: unique `id`, nonempty `tools` enum list, `root`, `mode = "symlink"` or `"copy"`. Tools: `codex`, `claude`, `opencode`, `gemini`, `agy`, `hermes`, `openclaw`. |
| `placements[]` | Records with `source`, relative package `path`, frontmatter `name`, and nonempty target-ID list. No globs or implied install-all. |
| `ephemeral` | Optional: `enabled` (default false), `max_tokens` (default 4000), `ttl_seconds` (default 86400); positive bounded integers. TTL is stale-lease eligibility, not permission to delete active tasks. |
| `telemetry` | Optional: `enabled` (default true, local only), `retention_days` (default 30). Values above zero; disabling removes collection, not rollback state. |

Example: a project skill shared by Codex/OpenCode/Gemini/agy and Claude, plus one selected global skill. These are desired directories; the host must separately authorize each root.

```toml
version = 1

[sources]
canonical = "$XAVIER_SKILL_STORE"
project = "$XAVIER_WORKSPACE_ROOT/skills"

[[targets]]
id = "shared-project"
tools = ["codex", "opencode", "gemini", "agy"]
root = "$XAVIER_WORKSPACE_ROOT/.agents/skills"
mode = "symlink"

[[targets]]
id = "claude-project"
tools = ["claude"]
root = "$XAVIER_WORKSPACE_ROOT/.claude/skills"
mode = "symlink"

[[targets]]
id = "codex-global"
tools = ["codex"]
root = "$HOME/.agents/skills"
mode = "symlink"

[[placements]]
source = "project"
path = "xavier-rtk-execution"
name = "xavier-rtk-execution"
targets = ["shared-project", "claude-project"]

[[placements]]
source = "canonical"
path = "review"
name = "review"
targets = ["codex-global"]

[ephemeral]
enabled = true
max_tokens = 4000
ttl_seconds = 86400

[telemetry]
enabled = true
retention_days = 30
```

Expansion permits only the documented path variables plus `$HOME`, with no shell evaluation, recursive substitution, `~` guessing or unresolved variables. Reject absolute package paths, `..`, invalid slugs, source/target overlap and placement cycles. A package contains `SKILL.md` and optional `references/`, `scripts/`, `assets/`; frontmatter name must equal destination folder name. Apply the brief's metadata/body limits (`docs/design/skill-controller/00-BRIEF.md:38`).

Hash sorted relative paths, file bytes, entry types and executable bits for the entire package, not just `SKILL.md`. Identical duplicate placements coalesce; different packages targeting the same name fail with a conflict. Resolve workspace/global name collisions explicitly before dispatch; no scan-order winner. A manifest omission only removes an unchanged **controller-owned** entry. Missing/unreadable sources abort reconciliation rather than appearing as desired deletions.

`plan` is the default and performs no persistent writes; it emits JSON for the caller to save. It captures manifest/package hashes and destination preconditions. `apply --plan <file>` rechecks them and requires operator invocation or an already authorized service policy; autonomous does not mean silent widening of authorization.

## 4. Canonical store and CLI views

### Competing stores

| Option | Benefits | Costs / decisive limitation |
|---|---|---|
| A — `$HOME/.agents/skills` as Git repository | Native Codex discovery; simple migration destination; interoperable name. | Every canonical package becomes globally discoverable by Codex and potentially OpenCode, defeating selective global placement. Mixing durable sources and generated views complicates ownership. |
| B — retain `$HOME/.hermes/skills`, add Git history | Lowest migration cost; matches current registry and `AGENTS.md:92`. | Hermes' discovery directory remains the whole catalog; tool lifecycle and source ownership stay coupled. Other real sources still require consolidation. |
| C — new Git source at `$HOME/.local/share/xavier/skill-store` | No automatic CLI discovery; all consumers receive deliberate views; catalog size does not force global exposure. | One controlled migration; update registry configuration and canonical-path rules; source path must remain available. |

**Recommend C.** `XAVIER_SKILL_STORE` selects the actual location; the path above is a portable example, not a hardcoded personal path. This is authored source, distinct from runtime databases governed by ADR-016. Git history and an independent backup preserve recoverability; Git alone on the same disk is not a backup. No remote, commit or push is automatic.

Repository-maintained skills stay in their owning repositories; the manifest references them without duplicating authority in the global store. This repository has conflicting project conventions (`AGENTS.md:92` says `.skills/`, `AGENTS.md:107` says `skills/`); follow its actual `skills/` source and explicit manifest mapping. Migration must reconcile that documentation, REQ-060's Hermes requirement (`docs/SRS/REQUIREMENTS.md:1125`), and the global-store rule before rollout.

### Publication strategy

Use **one symlink per selected skill directory**, preserving references and scripts. Never link an entire discovery directory to the canonical catalog: it publishes everything, prevents per-tool selection, and displaces tool-owned entries. Projected copies are an explicit fallback for a harness/filesystem that fails symlink discovery checks; they cost space and require whole-package hash auditing. A detected edited copy becomes a conflict, never an upstream change or automatic overwrite.

| CLI | Proposed global view | Project / task view |
|---|---|---|
| Codex | `$HOME/.agents/skills/<name>` | `<worktree>/.agents/skills/<name>` |
| Claude Code | `$HOME/.claude/skills/<name>` | `<worktree>/.claude/skills/<name>` |
| OpenCode | `$HOME/.config/opencode/skills/<name>` only for additional tool-specific entries | Reuse `.agents/skills`; account for its additional `.claude/skills` discovery. |
| Gemini CLI | `$HOME/.gemini/skills/<name>` | Root `.agents/skills`; nested discovery remains unverified. |
| agy | `$HOME/.gemini/config/skills/<name>` | Root `.agents/skills`; validate the installed wrapper. |
| Hermes | `$HOME/.hermes/skills/<name>` after migration | Project discovery is not established by the brief; use explicit dispatch/context injection until tested. |
| OpenClaw | `$HOME/.openclaw/skills/<name>` | Same compatibility gate; do not invent a project discovery path. |

The first five rows use the brief's discovery matrix (`docs/design/skill-controller/00-BRIEF.md:34`); Hermes/OpenClaw global paths are observed trees only (`:46`). Check actual startup discovery and symlink support before enabling each adapter. `$HOME/.codex/skills` is a legacy compatibility target only when a measured installed version requires it.

OpenCode can see the union of `.agents`, `.claude` and its own directories: `tools` describes intended readers, **not access isolation**. Audit duplicate names across that union and avoid redundant projection. Strictly different catalogs require isolated harness homes/workspaces, outside v1. Never change discovery configuration implicitly.

Migration sequence: inventory all real sources and links; hash complete packages; report divergent names and secrets; obtain review of import/conflict resolutions; back up originals; import approved clean packages with provenance; preview and apply each view; verify every enabled harness with Xavier stopped. Preserve previous stores until verification and rollback retention expire. Existing regular skill directories require a separately reviewed migration; the routine projector refuses to replace them.

## 5. Ephemeral composition, drift and telemetry

**Composition:** before spawn, bind an opaque task ID to an authorized workspace/worktree and lease. Reuse selected skills plus scoped evidence; render deterministic Markdown without an LLM. Publish `<worktree>/.agents/skills/eph-<task>/SKILL.md` atomically and a per-skill `.claude/skills` link. Any additional view must pass its adapter check. Store source hashes, memory IDs, graph revision, task ID and expiry as provenance; never include raw prompts in descriptions. Exclude generated entries from future source selection to prevent recursive composition. If a complete safe instruction section cannot fit, omit it or return `_none`; do not truncate away trust boundaries or safety clauses.

The same composition is returned as the existing MCP context pack for live sessions. Approved base instructions and quoted evidence remain separate. The current decision-memory query filters only by kind (`src/context/skill_dispatcher.rs:207`), so the new path must enforce authorized workspace/project scope for **both** ordinary memories and decisions. Do not trust the caller's `project` string as authorization. The existing unused decision budget (`src/context/skill_dispatcher.rs:107`) is not a total-budget guarantee: enforce the final serialized composition budget explicitly.

Task close removes only journaled, unchanged generated files and links, then empty owned directories. Stale leases require confirmed task inactivity before cleanup; an active task renews its lease. Keep runtime composition out of Git without modifying tracked project ignore files. Missing memory/graph services permit skill-only composition with a degradation marker.

**Drift auditor:** compare package and projection hashes; validate frontmatter, referenced local files and repository version declarations. Report declared rule conflicts and suspicious prose separately: deterministic checks cannot prove arbitrary natural-language contradictions. Dead endpoints are `unreachable` or `unverified`, not necessarily retired. Default to offline checks; optional bounded probes require an explicit endpoint allowlist, block redirects outside it and never attach credentials. Reports contain rule IDs, package-relative `file:line`, severity and redacted proposed changes. Canonical rewrites require approval; secrets and safety/approval rules never receive automatic fixes.

**Telemetry:** append `{event_id, timestamp, workspace_id, task_id, tool, skill_name, package_hash, stage, outcome, latency_ms, token_estimate}` locally. Use opaque IDs; omit task text, memory content, absolute paths and secrets. Stages distinguish `selected`, `delivered`, `invoked`, `completed`; invocation/completion require explicit harness acknowledgments with idempotent event IDs. A dispatch is not proof of use; ordinary filesystem reads are invisible. Report coverage and unknown usage, so zero observations cannot justify deletion. Retain 30 days by default (design assumption), configurable/disableable. A bounded telemetry queue may drop events with a counter; it must not fail dispatch. Reuse SQLite migrations, not the maintainer-wallet telemetry schema (`src/data_commons/telemetry_db.rs:9`).

## 6. Failure, rollback and security

| Failure | Behavior / rollback |
|---|---|
| Invalid manifest, missing source, secret finding or changed package | Abort affected plan before publication; preserve existing views. Report, never auto-edit the source. |
| Unmanaged destination or externally modified owned copy/link | Conflict; leave it intact. Ownership requires journal identity plus expected current hash/link target, not merely its name. |
| Permission error, disk full, concurrent writer | Lock exact target roots in stable order; abort before changes if durable journal/backup cannot be written. No privilege escalation. |
| Crash between entry replacements | Write-ahead journal records old/new identities and transaction state. On restart, reconcile incomplete transactions and conditionally restore unchanged controller outputs. |
| Stale index or embedding outage | Rebuild from validated files; use lexical ranking. No canonical skill depends on a vector index. |
| Xavier stopped | Published files remain readable; new composition/audit/events pause. No symlink targets point into a process-owned temporary directory. |
| Store moved/deleted | Report broken sources; no cleanup inferred from disappearance. Restore store backup or approve a path migration. Symlinks do not survive loss of their source. |
| User edit during rollback | Stop at that entry and report conflict; never overwrite the user's change to complete rollback. |

Before mutation, persist a private backup and journal. Back up symlink objects with `readlink`, not by dereferencing them. Owned copies require content backups; unmanaged regular files/directories are untouched. Stage adjacent temporary entries, flush files/journal, rename atomically and flush parent directories. Atomicity is **per entry**, not across all tool roots: report partial application and roll back conditionally. Delay agent spawn until all required views verify. Cleanup uses exact owned paths, never recursive blanket deletion.

Security requirements:

- **Independent authorization:** host policy allows exact source roots, target directories and active worktrees. A repository manifest or recalled memory cannot grant new roots, configure secrets, execute commands or modify approval policy. Controller runs as the user, never root; mutation remains unavailable to unauthenticated HTTP/MCP callers.
- **Containment:** validate every source package/resource and target-parent component using directory handles and no-follow traversal. Resolve explicitly allowed links with bounded hops and reject escapes, cycles, special files and unexpected hardlinks. Revalidate identities at apply; a one-time `canonicalize` followed by pathname writes is insufficient against symlink swaps. On platforms lacking equivalent race-resistant operations, refuse mutation. Target roots must be real validated directories; an existing directory symlink is a migration conflict. Per-skill links may cross from a target root only to a separately allowed source root.
- **Secrets:** scan the full candidate package and composed output before publication, including references/scripts/assets; reject unreadable or unscannable content. The current repository scan is not a sufficient package gate (`scripts/check-secrets.sh:13`, `:25`). Use redacted findings only, with no matching credential bytes in reports, diffs, telemetry or backup names. Existing canonical secrets remain report-only for human remediation; do not copy them into Git history. Preserve the last verified clean view where available.
- **Untrusted evidence:** memories, graph strings and task text are data, never executable instructions or frontmatter. Serialize/escape them into bounded quoted evidence with origin IDs; do not allow closing delimiters or headings to escape that representation. Never derive `allowed-tools`, filesystem policy or shell commands from them. Reuse the pattern detector (`src/security/prompt_guard.rs:446`) as defense in depth, not proof of safety. Memory-supplied requests to ignore rules, reveal secrets or widen access are omitted and reported.
- **Limits of trust:** formatting an `UNTRUSTED` block cannot guarantee model behavior. Harness permissions and confirmation rules remain authoritative. The current session path wraps memory summaries as system-role documents (`src/server/http/context.rs:229`); this is not an acceptable trust model to copy. Same-user symlinks also do not prevent agents from editing source files; review policy and OS/harness write restrictions must protect canonical content. Private runtime artifacts use owner-only permissions; no content is exported automatically.

## 7. Out of scope

No skill marketplace, remote synchronization, automatic Git publication, new daemon, FUSE/VFS, custom DSL, plugin system, model training or LLM-driven canonical rewrites. No automatic credential repair, safety-rule modification, usage-based deletion or claim of complete invocation tracking. No arbitrary network crawling, cross-user catalog isolation, replacement of orchestration/harness permissions, or wholesale migration of existing CLI discovery settings. Compatibility validation and approved migration are prerequisites, not work performed by this design document.

## ADR-034 — Canonical skill sources and declarative tool views

Draft following `docs/adr/TEMPLATE-ADR.md`; Spanish section labels retain the template, with English content.

| Campo | Valor |
|---|---|
| **ID** | ADR-034 |
| **Estado** | Propuesto |
| **Fecha** | 2026-09-27 |
| **Autores** | Xavier architecture contributors |
| **Relacionados** | ADR-016; ADR-019 (narrow amendment proposed); REQ-060..064; Skill Controller brief |

### Contexto

Xavier needs selective CLI placement, task-scoped context and drift visibility while remaining file-based and local-first (`.gitcore/docs/SWAL_GOAL.md:8`, `docs/design/skill-controller/00-BRIEF.md:58`). Current Hermes canonical discovery is embedded in the registry (`src/context/skill_registry.rs:95`). Moving authority must preserve existing files and respect source ownership; adopting an in-process controller requires the explicit ADR-019 amendment described in §2.

### Opciones que compiten

| Opción | Descripción | Coste/complejidad esperada |
|---|---|---|
| A | Git repository directly at `$HOME/.agents/skills`; per-skill views elsewhere. | Moderate migration; lowest Codex integration cost; exposes full source catalog globally. |
| B | Retain `$HOME/.hermes/skills` as canonical, add versioning and projection. | Lowest immediate change; Hermes discovery remains coupled to the catalog. |
| C | Dedicated non-discovered Git source; per-skill projections into all harness roots. | Highest one-time migration of these three; explicit separation of authorship and discovery. |

These are qualitative engineering estimates, not simulation results. Whole-directory links and projected copies are competing publication strategies evaluated in §4, independently of store choice.

### Simulación (OBLIGATORIO)

**Simulación: PENDIENTE**

Checked on 2026-09-27: `$HOME/proyectosSWAL/periferia/swal-sim/adr_sim.py` exists and `python3 -B "$HOME/proyectosSWAL/periferia/swal-sim/adr_sim.py" --list` exits successfully. It lists `xavier_cloud_tier`, `maloca_analytics_monetization`, `ledger_architecture`, `supply_scale`, `karma_semantics`, `council_approvals`, and `reviewer_assignment`. None models skill-source ownership, migration or projection; using one would measure a different decision.

Prerequisite: implement and register model **`skill_controller_store`** with ADR ID `ADR-034`, options A/B/C, complete metric definitions, anchored inputs and bull/base/bear/worst scenarios. This model does **not** exist today. Measure migration effort, unwanted discovery, recovery time and drift incidents; use field measurements or explicitly label estimates `ASSUMPTION`. Scenario assumptions must cover healthy operation, catalog growth, conflicting edits and interrupted migration/store loss.

Exact command to run after that prerequisite:

```sh
python3 -B "$HOME/proyectosSWAL/periferia/swal-sim/adr_sim.py" \
  --model skill_controller_store --runs 5000 --seed 42 \
  --outdir "$HOME/proyectosSWAL/periferia/swal-sim/reports"
```

- **Motor:** the checked script above; no simulator file was edited.
- **Runs / seed:** 5000 / 42 planned, not executed.
- **Resultado / ¿Coinciden?:** composite winner, primary-only winner and agreement pending; no measured margin.
- **Sensitivity:** pending leave-one-scenario-out checks and p10–p90 intervals; robustness unknown.
- **Reporte:** expected `$HOME/proyectosSWAL/periferia/swal-sim/reports/ADR-034-skill_controller_store.md` and `.json`; neither generated by this task.

### Supuestos y su anclaje

| Parámetro | Valor usado | Fuente |
|---|---|---|
| Existing canonical scan root | Hermes plus workspace `skills/` and `.agents/skills` | `src/context/skill_registry.rs:105` |
| Native global discovery | Codex reads `.agents`; OpenCode also reads `.claude` | Brief evidence, `docs/design/skill-controller/00-BRIEF.md:34`; revalidate installed versions |
| Canonical catalog inside a discovery root defeats selective exposure | Architectural inference for A/B | Discovery assumption above; no numerical estimate |
| File operation without Xavier | Required | `docs/design/skill-controller/00-BRIEF.md:11` |
| Per-skill links supported by every target version | ASSUMPTION, adapter acceptance gate | No universal compatibility measurement |
| Ephemeral budget / stale lease / telemetry retention | 4000 tokens / 86400 seconds / 30 days | ASSUMPTION: initial configurable policy, not simulation output |
| Relative migration costs in options table | Qualitative estimates | ASSUMPTION; collect operator time and conflicts before simulation |

### Decisión

Provisionally recommend **C with per-skill symlinks**, because source placement must not implicitly publish the whole catalog; **no simulated winner or margin is claimed**. Accept only after simulation, adapter verification, and reconciliation of `AGENTS.md:92`, REQ-060 and ADR-019. Keep Hermes authoritative until an approved migration completes.

### Consecuencias

- **Positivas:** deliberate visible catalogs, Git-backed source provenance, offline file access, one reconciliation path for all tools, reversible placement.
- **Negativas / coste:** migration and backups, adapter-specific discovery tests, source availability dependency, incomplete telemetry without harness hooks, application-layer exception to ADR-019.
- **Qué invalidaría esta decisión:** reliable native per-tool filtering makes A/B equally selective; measured migration/recovery cost exceeds the benefit; a required harness cannot safely consume either symlinks or managed copies; or scenario sensitivity reverses the recommendation without a justified tradeoff.

### Verificación posterior

Before acceptance: run the relevant simulation and report both winners, uncertainty and sensitivity; test each enabled CLI at startup with Xavier stopped. In an isolated fixture, verify exact visible skill sets, byte-identical second apply, rejection of symlink escapes/swaps and unmanaged-file collisions, recovery after interruption at each mutation boundary, secret-safe reports, scoped evidence, bounded output and task cleanup without deleting user edits. These are future implementation gates, not tests run here.

Review on **2026-10-27**, or before rollout if earlier: record unexpected visible skills, unresolved drift, failed rollback restorations, composition failures, actual migration time and invocation-observation coverage. Acceptance targets are zero unauthorized writes, zero overwritten unmanaged entries, zero secret-bearing publications and no false claim that unobserved use is zero. If no pilot exists by that date, keep ADR-034 **Propuesto**. Feature status remains governed by the verification pipeline.
