# AGENTS.md — Xavier

Xavier is a **high-performance vector memory runtime for AI agents**, written in
Rust. It provides persistent, searchable, interpretable memory with native HTTP,
CLI, and MCP entry points (SQLite + `sqlite-vec`, BM25, hybrid search).

This file is a contract for anyone — human or AI agent — working on this repo.

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

- `docs/features/features.json` is the source of truth (status, tests, files).
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
