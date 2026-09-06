# AGENTS.md — Xavier

Xavier is a **high-performance vector memory runtime for AI agents**, written in
Rust. It provides persistent, searchable, interpretable memory with native HTTP,
CLI, and MCP entry points (SQLite + `sqlite-vec`, BM25, hybrid search).

This file is a contract for anyone — human or AI agent — working on this repo.

## 🌐 SWAL Ecosystem Integration Block

- **GOAL:** Defined in `.gitcore/docs/SWAL_GOAL.md`. Decoupled, local-first, privacy-preserving AI context & memory architecture.
- **PROJECT MAP:**
  - `src/` — Main Rust domain logic, HTTP/MCP servers, security, and storage adapters.
  - `xavier-core/` — Core vector store & embedding calculation primitives.
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

## 3. Development by waves

- Work is organized in waves: research → issues → execution → verification.
- Full protocol: `docs/protocol/` (README first).
- A new wave does not open until the previous one's features are `stable`.

## 4. Feature verification

- `.gitcore/features.json` is the source of truth (status, tests, files).
- Run `scripts/verify-pipeline.sh` to see the real state — it EXECUTES the
  declared tests. The pipeline is the judge; status is never hand-promoted.
- CI runs the same pipeline on every PR.

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
- CI runs: fmt, clippy, tests, feature verification, secret scan.
- Never commit: session state, output artifacts, `.env`, logs, databases.

## 10. Canonical Directory Protocol & Agent Hygiene

All autonomous agents (Jules, Hermes, Antigravity, Claude, etc.) MUST strictly respect directory boundaries. **DO NOT invent arbitrary root-level dot-directories or config folders.**
- **Jules memory / learnings**: MUST only be recorded in lowercase `.jules/` (e.g. `.jules/palette.md`). Never create uppercase `.Jules/`.
- **Git Hooks**: The repository strictly uses `.husky/` via Husky 9. Never create or restore `.githooks/`.
- **GitCore Ledger**: All wave management, issue definitions, chronicles, and specs live under `.gitcore/`. Do not create competing tracking directories.
- **Skills**: Global skills reside in `~/.hermes/skills/` and project skills in `.skills/` (never commit ephemeral skill files directly to repo root).
- **Runtime databases and caches**: `.xavier/`, `data/`, `*.db`, `*.sqlite*`, `metrics.db*`, `xavier_memory.db*` are strictly runtime caches. Never commit SQLite database files, WAL files, or SHM files. Only static configuration fixtures (such as `.xavier/maturity-anchors.json`) are tracked under `.xavier/`.
- **Cargo & Compiler Settings**: `.cargo/` is the canonical Rust compiler configuration directory and must remain clean.

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
