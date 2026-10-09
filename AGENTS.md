# AGENTS.md — Xavier

Xavier is a **high-performance vector memory runtime for AI agents**, written in
Rust. It provides persistent, searchable, interpretable memory with native HTTP,
CLI, and MCP entry points (SQLite + `sqlite-vec`, BM25, hybrid search).

This file is a contract for anyone — human or AI agent — working on this repo.

## 🌐 SWAL Ecosystem Integration Block

- **GOAL:** Defined in `.gitcore/docs/SWAL_GOAL.md`. Decoupled, local-first, privacy-preserving AI context & memory architecture.
- **PROJECT MAP:**
  - `src/` — Main Rust domain logic, HTTP/MCP servers, security, and storage adapters.
  - `crates/xavier-core-logic/` — Core vector store & embedding calculation primitives.
  - `code-graph/` — Static AST code indexer & symbol graph engine.
  - `panel-ui/` — React frontend presentation layer & Maloca web portal.
  - `.gitcore/` — GitCore protocol ledger (`features.json`, `MANIFEST.json`, `AGENT_INDEX.md`).
  - `docs/` — SRS requirements (`docs/SRS/REQUIREMENTS.md`), design ADRs, and operational guides.
- **Xavier Namespace:** `swal/{app_id}/{instance_id}` for workspace isolation across multi-instance agent nodes.
- **SWAL Mesh & Node Identity:** Node identity authenticated via Ed25519 BIP39-24 keypair (`src/node_identity/`) with zero central user accounts. Pro features gated by SWAL active node state (`pro_gate.rs`), never Stripe or paywalls.
- **Protocol Reference:** GitCore 3.8.0 specification compliant (`.git-core-protocol-version`).

## 1. Purpose & vision

Xavier is the cognitive memory brain of the SWAL ecosystem. The repo is built
in **waves** (sprints) with a **verifiable feature ledger**: nothing is "done"
by declaration, only by a green verification run.

## 2. Setup commands

```bash
# Install deps (NixOS): nix-shell (see shell.nix) — needs openssl + pkg-config
cargo build --release --features local-gllm   # or: --features ci-safe for CI
cargo test --workspace                         # full suite
cargo clippy --all-targets -- -D warnings      # warnings are errors
```

### Remote agents (Jules, cloud sandboxes)

Do **not** run the setup commands above in a remote sandbox: a full release build of this
workspace exceeds typical sandbox limits (Jules fails while "preparing the virtual machine
environment"). Remote agents only need `rustfmt`; they write code and tests, and CI verifies
every PR (fmt, clippy `-D warnings`, tests). Use a light setup: `rustup component add rustfmt`.

**Jules PRs (owner rule, 2026-10-03):** all Xavier implementation goes through Jules; the owner's
machine never builds. PRs must be mergeable as-is:
- Base branch = the branch named in the task (today `sprint/xavier-2026-10`), never `main`.
- Touch ONLY the files the task lists; no lockfile, formatting-only or drive-by changes elsewhere.
- Run ONLY `cargo fmt -- <files you touched>` (rustfmt does not compile). **NEVER run `cargo build`,
  `cargo check`, `cargo test`, `cargo clippy` or `cargo run` in a remote VM**: a cold build of this workspace
  outlasts and exhausts the sandbox, and the session dies ~10 min later with "Jules encountered an error when
  working on the task" (measured on 102 failed sessions, 2026-09). Write the code and the tests, open the PR,
  and let CI compile and test it.
- CI runs on PRs to `main` and `sprint/**`. If it fails, a bot comment `@jules` brings the failing log
  to the PR: fix only what the log shows, on the same branch.

## 3. Development by waves

- Work is organized in waves: research → issues → execution → verification.
- Full protocol: `docs/protocol/` (README first).
- A new wave does not open until the previous one's features are `stable`.

## 4. Feature verification

- `.gitcore/features.json` is the source of truth (status, tests, files).
- Run `scripts/verify-pipeline.sh` to see the real state — it EXECUTES the
  declared tests. The pipeline is the judge; status is never hand-promoted.
- **Post-merge lane** on `main`: full feature-ledger verification (`scripts/verify-pipeline.sh`), and it is the only thing allowed to promote a `features.json` status.

## 5. Modifying features.json

- Never change `status` to a higher value by hand — a green run promotes it.
- New feature: PR that adds the spec (`docs/features/specs/FEATURE-*.md`) +
  the ledger entry (`status: planned`) + implementation + tests.

## 6. Architecture decisions

- Non-obvious decisions become ADRs in `docs/adr/`
  (numbered, context → decision → consequences). Debates happen in public.
- A change that contradicts an ADR must update it or open a new one.

## 7. Configuration & secrets (12-factor)

- All configuration lives in environment variables. No hardcoded credentials,
  environment endpoints, or personal paths in the code.
- `.env` is never committed. `.env.example` documents every variable with
  placeholder values — if you add a `std::env::var`, add the key to
  `.env.example` in the same PR.
- Secrets are scanned by `scripts/check-secrets.sh` (gitleaks) before merge.

## 8. Code style

- `cargo fmt` required; clippy must be warning-free (`-D warnings`).
- Errors: `thiserror` in libs, `anyhow` in binaries (follow existing patterns).
- Golden rule (Tokio + Rayon): never call Rayon `.par_iter()` directly inside
  a Tokio worker — wrap in `tokio::task::spawn_blocking`.
- Comments in English, minimal density.

## 9. Pull requests

- 1 PR = 1 feature (or a bounded part of it), referencing its feature id.
- **PR lane**: `cargo fmt --all -- --check`, `cargo check --package xavier --all-targets --features ci-safe`, `cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings`, `cargo test --package xavier --lib --features ci-safe -- --test-threads=1`, plus the secret scan.
- Never commit: session state, output artifacts, `.env`, logs, databases.

## 10. Canonical Directory Protocol & Agent Hygiene

All autonomous agents (Jules, Hermes, Antigravity, Claude, etc.) MUST strictly respect directory boundaries. **DO NOT invent arbitrary root-level dot-directories or config folders.**
- **Jules memory / learnings**: MUST only be recorded in lowercase `.jules/` (e.g. `.jules/palette.md`). Never create uppercase `.Jules/`.
- **Git Hooks**: The repository strictly uses `.husky/` via Husky 9. Never create or restore `.githooks/`.
- **GitCore Ledger**: All wave management, issue definitions, chronicles, and specs live under `.gitcore/`. Do not create competing tracking directories.
- **Skills**: Global skills reside in `~/.hermes/skills/` (authoritative canonical store until migration completes) and project skills in `skills/` (never commit ephemeral skill files directly to repo root). The canonical store is configurable via `XAVIER_SKILL_STORE` (e.g. `$HOME/.local/share/xavier/skill-store`); Hermes remains authoritative until an approved migration completes (docs/design/skill-controller/05-DECISIONS-AND-SCOPE.md D1, D5).
- **Runtime databases and caches**: `.xavier/`, `data/`, `*.db`, `*.sqlite*`, `metrics.db*`, `xavier_memory.db*` are strictly runtime caches. Never commit SQLite database files, WAL files, or SHM files. Only static configuration fixtures (such as `.xavier/maturity-anchors.json`) are tracked under `.xavier/`.
- **Cargo & Compiler Settings**: `.cargo/` is the canonical Rust compiler configuration directory and must remain clean.

## Build and verify — read this before running cargo

This repo is large; a `cargo test` on it routinely outruns any pipe buffer.
Audited failure: one agent ran the same `cargo test … | tail -N` **22 times**,
changing `-N` each time (`-35 → -60 → -25`), and 15 of 22 runs returned empty
output. Every one of those turns was wasted.

```bash
# WRONG — discards the error when the build outlasts the buffer
cargo test --features ci-safe 2>&1 | tail -30

# RIGHT — full output on disk, then read the file
cargo test --features ci-safe > /tmp/check.log 2>&1
```

- `PKG_CONFIG_PATH=$HOME/.nix-profile/lib/pkgconfig` is needed on NixOS.
- Fast inner loop: `cargo test --package xavier --lib --features ci-safe`.
  Use `--test <name>` for a single integration test. Never `--release`.
- Runs over ~30s: launch in background with completion notification, keep
  working, read the log when it lands. Do not block on repeated `wait` polls.
- **A retry is only justified when the error message changes.** Identical error
  twice means the same bug — change strategy or ask, do not run it a third time.
- A `count: 0` or empty result is a hypothesis, not a finding. Confirm on disk.

## Symbol map — verified 2026-10-02 (generated from source, not prose)

An audit of 10 agent sessions found **0 of 15** searched symbols documented
anywhere in this repo's entry docs. This table is the fix. Grep here first.

| Symbol | Defined at |
|---|---|
| `record_accesses` | `src/memory/sqlite_vec_store/store_impl.rs:869` |
| `flush_pending` | `src/memory/access.rs:389` |
| `maybe_flush` | `src/memory/access.rs:370` |
| `RECORDER` (`static`) | `src/memory/access.rs:122` |
| `MemoryStore` | `src/memory/sqlite_vec_store/store_impl.rs:52` |
| `MemoryDaemon` | `src/scheduler/daemon.rs:25` |
| `qmd_memory` | `src/memory/qmd_memory.rs:9` |
| `resolve_data_dir` | `src/settings/serialization.rs:59` |
| `XavierSettings` | `src/settings/mod.rs:143` |
| `code_graph_db_path_for` | `src/codebase/codegraph_paths.rs:17` |
| `open_code_graph_db` | `src/server/headless/routes.rs:192` |
| `execute_code_tool` | `src/server/headless/routes.rs:205` |
| `register_project` | `code-graph/src/db/mod.rs:2096` |
| `QueryEngine` | `code-graph/src/query/mod.rs:434` |
| `CodeGraphDB` | `code-graph/src/db/mod.rs:92` |

`src/memory/access_tests.rs` is a **file**, not a symbol. Largest files by
module: `memory` (51 files / 25.2k lines), `server` (12k), `cli` (6.1k),
`codebase` (5.8k).

### Do not trust the code graph yet (known defects, verified 2026-10-02)

- `code_stats` **ignores its `path` argument** — two different repos return
  identical numbers. The values describe the server's single shared DB.
- `project_root` is **NULL for all 24,708 indexed symbols** and appears in no
  `WHERE` clause, so there is no per-project isolation. `Symbol` has no
  `project_root` field and neither INSERT writes it.
- Consequence: `code_scan` of any repo overwrites the previous repo's index in
  the shared `data/code_graph.db`. **Confirm the target DB path before a write
  scan.**
- Only `apps/xavier/data/code_graph.db` holds symbols; 18 of 19 `code_graph.db`
  files on disk are 4 KB stubs with 0 rows.
- A `count: 0` from `code_find` is not proof a symbol is absent — the index is
  import-saturated (13.6k Variable / 3.2k Import vs 2.4k Function).

## Decision rights (agent panel)

- An independent agent panel decides reversible designs, ADR acceptance, Ask-class PR approval, and task plans after deterministic gates pass. Two independent models, neither the author, must approve; store their verdicts and an auditable report tied to the reviewed revision.
- The owner alone gives final product acceptance through the Product Acceptance Package. Agents prepare credentials, paid capacity, branch rules, public releases, and production data migrations for that acceptance; they do not activate them silently.

<!-- SWAL-ROUTING-START -->
## SWAL Routing Minimalista (SDD Hibrido F1)
> Antes de crear `.gitcore/sdd/` aplica routing organico (gentle-ai v2.3.0).
> - **Direct inline**: 1-3 files trivial -> inline sin delegar, sin SDD
> - **Delegated direct**: 4+ files o 2+ non-trivial -> delegate_task con Xavier skill search, sin SDD
> - **Optional SDD**: ambiguedad alta -> proponer SDD opcional, si SI crear `.gitcore/sdd/specs/###-feat/onepage.md` (1 pagina spec P1 + plan HOW minimo + tasks [P])
> Ver skill `sdd-hibrido` (`~/.hermes/skills/sdd-hibrido/references/routing.md`). `rm -rf .gitcore/sdd` limpia sin tocar features.json.
<!-- SWAL-ROUTING-END -->

<!-- SWAL-REGISTRY-START -->
## Skill Registry + Xavier Indexer (F1b)
> Skills viven en `skills/` dentro del proyecto y global en `~/.hermes/skills`. GitCore referencia via `.atl/skill-registry.md` + cache `.skill-registry.cache.json`.
>
> ### 📁 Canonical Project Skills (`skills/`)
> - [`xavier-cognitive-memory`](skills/xavier-cognitive-memory/SKILL.md): Memory, CodeGraph, and MCP protocol integration.
> - [`xavier-rtk-execution`](skills/xavier-rtk-execution/SKILL.md): High-performance CLI execution & token compression via `rtk-kernel` proxy.
> - [`xavier-code-graph-analysis`](skills/xavier-code-graph-analysis/SKILL.md): AST navigation, symbol lookup, and blast-radius impact analysis.
> - [`xavier-wave-verification`](skills/xavier-wave-verification/SKILL.md): Feature verification ledger & test pipeline execution.
> - [`xavier-maintenance-hygiene`](skills/xavier-maintenance-hygiene/SKILL.md): Anti-sprawl repo hygiene, database cache quarantine, and deprecation policy.
>
> - Refresh: `~/.hermes/scripts/skill-registry-refresh.sh --cwd <proyecto>`
> - Index: `~/.hermes/scripts/xavier-index-skills.sh --cwd <proyecto>` (Xavier tags [skill])
> - Antes de delegar: `xavier_search(tags=[skill]) -> skill_view(paths)`
> Ver skills `skill-registry` y `xavier-skill-indexer`.
<!-- SWAL-REGISTRY-END -->

<!-- SWAL-SDD-START -->
## SDD One-Page + SRS Mapping
> Spec efimero `.gitcore/sdd/specs/###-feat/onepage.md` referencia `REQ-xxx` durable de `docs/SRS/REQUIREMENTS.md` (IEEE 830 reduced). Drift detector `srs-src-drift-detector` mantiene traceabilidad. Docs humanos estables en `docs/`, specs AI en `.gitcore/sdd/` aislado.
<!-- SWAL-SDD-END -->

<!-- SWAL-ZERO-HANDOFF-START -->
## Protocolo Zero-Handoff Continuation (T3 / Xavier SSoT)
> Cuando una sesión larga de T3 acumula alta densidad de contexto (>250k–300k tokens o >750 pasos):
> 1. **Detección proactiva**: El agente verifica la salud del contexto ejecutando:
>    `python3 ~/.hermes/scripts/t3-context-monitor.py --check`
> 2. **Cápsula inmutable en Xavier**: Si el estado es `CRITICAL` o `needs_handoff: true`:
>    `python3 ~/.hermes/scripts/t3-context-monitor.py --capsule`
> 3. **Alerta y consulta obligatoria al usuario**:
>    El agente debe avisar al usuario de la saturación crítica y preguntarle explícitamente (usando `ask_question`) si desea abrir una ventana limpia con el Seed Prompt generado o continuar en la actual.
> 4. **Traspaso sin pérdida**:
>    Al migrar, la nueva ventana consulta la cápsula en Xavier vía `xavier search "<capsule_id>"` y retoma el trabajo con contexto limpio (<1k tokens) sin arrastrar histórico degradado.
<!-- SWAL-ZERO-HANDOFF-END -->

