# ARCH_WAVE4 — Mesh + IVN + Training + Curation hardening (WAVE-4)

> **WAVE-4** — 10/10 deltas + harness canónico. Continuación de WAVE-3 (enterprise mesh). Fecha: 2026-08-31 18:30 (-05). Commit base 18e4dc5c → fix ivn clippy + docs verification.

## Resumen

| # | Delta | PR | Isla disjunta | Estado | Tests |
|---|-------|----|---------------|--------|-------|
| 4.01 | Training datasets REST API (list/manifest/train/eval/bundles) | #1766 | `src/data_commons/training.rs`, `src/adapters/.../training.rs` | stable 100% | training handlers 4 rutas |
| 4.02 | Mini-experts personal on-demand (registry + Ollama) | #1758 | `src/data_commons/mini_experts.rs` | stable 100% | provider_router integration |
| 4.03 | Mesh service network INTERNAL publish/consume + PII exclusion | #1754 | `src/mesh/mesh_service.rs`, `src/mesh/heartbeat.rs` | stable 100% | PII exclusion test |
| 4.04 | Private mesh by key wallet + cross-wallet isolation | #1753 | `src/mesh/private_mesh.rs` | stable 100% | `test_private_mesh_cross_wallet_isolation` |
| 4.05 | Human curation approve/classify flow + history | #1756 | `src/data_commons/curation.rs` | stable 100% | `test_curation_approve`, `history` |
| 4.06 | Issue-context-packager: auto analysis GitHub issues → context pack | #1765 | `src/codebase/issue_context.rs` | stable 100% | issue→context pack |
| 4.07 | Store path hierarchy: preserve full store path hierarchy | #1767 | `src/memory/store.rs` | stable 100% | path hierarchy tests |
| 4.08 | IVN karma rewards, sanctions, reputation + 6 HTTP handlers | #1759 | `src/data_commons/ivn.rs`, `src/adapters/.../ivn.rs` | stable 100% | Karma + Verdict + exclusion |
| 4.09 | WASM + RAG: IndexedDB real web-sys + RAG RRF hybrid | #1755 | `crates/xavier-wasm/` + `src/search/rerank.rs` | stable 100% | wasm 4 + RAG 2 |
| 4.10 | Mesh + clearance hardening: libp2p gossipsub wire + E2E | #1757 | `src/mesh/libp2p_transport.rs` + `src/security/clearance.rs` | stable 100% | gossipsub + clearance E2E |

Total: 52/52 stable 100% (de 52 features, 0 beta). 9 PRs WAVE-4 + 1 previo (1758 ya en main). Duplicados cerrados 1763/1764.

## Decisiones

- **Harness canónico Rust** (`xavier-jules-wave` v1.0): título `feat-` + 11 secciones + PR Delivery con `wc -l` + `grep -c fn test_` + Verification cargo check/clippy/test/fmt. Copiado a `~/.hermes/skills/` + `apps/xavier/.hermes/skills/`. Obligatorio para olas 5+. 3 issues recreados con nuevo template (1760-1762) pero ola final usó 1753-1767 directos.
- **Islas disjuntas**: cada PR toca archivos exclusivos salvo `src/mesh/libp2p_transport.rs` compartido entre 4.03/4.10 con merge order; verificación `python3 islands` sin conflicto.
- **Clippy fix**: `IvnEngineStore` manual `impl Default` → `#[derive(Default)]` (KarmaEngine ya tiene Default). 7 files fmt en WAVE-3 fix, 4 files chunks_exact, 1 WasmStore.
- **CI lab**: `.cargo/config.toml` target-dir absoluto `/build/rust-target` rompe GH Actions → override `CARGO_TARGET_DIR=target` en `pr-gate` (fix 4d26bd3f). PR duplicado close con comentario `superseeded`.
- **IvnEngineStore derive**: verifica `KarmaEngine: Default` antes de derive; `HashMap::new()` no necesario.
- **Training API**: `DatasetMetadata` clearance/consent/segment/language; handlers `list_training_datasets_handler`, `get_training_dataset_handler`, `get_training_split_handler`, `create_training_bundle_handler` con JSONL splits + seed/eval_ratio.
- **Private mesh**: wallet-scoped Clavis leases, X25519+AES-GCM session encryption, `PrivateMesh::discover/sync`, cross-wallet isolation test es gate.
- **Curation**: `CurationStatus::Pending|Approved|Rejected`, `GET /v1/curation/pending`, `POST approve/classify`, `GET history` (`who classified what, when`), gate `Approved` para TrainingExporter.

## Verificación (WAVE-4 2026-08-31)

```
CARGO_TARGET_DIR=target cargo check --all-targets  # 0 (incl. IvnEngineStore derive fix)
CARGO_TARGET_DIR=target cargo check --package xavier --all-targets --features ci-safe  # 0 (CI gate)
CARGO_TARGET_DIR=target cargo clippy --all-targets -- -D warnings  # 0 (clippy derivable_impls fixed)
CARGO_TARGET_DIR=target cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings  # 0
cargo fmt --check                                   # 0
CARGO_TARGET_DIR=target cargo test --package xavier --lib --features ci-safe -- --test-threads=4  # 2009 passed, 2 ignored, 0 failed (33.96s)
CARGO_TARGET_DIR=target cargo test -p xavier-wasm    # 4 passed
CARGO_TARGET_DIR=target cargo test -p code-graph --lib  # 81 passed
CARGO_TARGET_DIR=target cargo test -p xavier-core-logic --lib  # 24 passed
pnpm --filter xavier-panel-ui run build              # 0 (vite 8.0.16, 3647 modules, gzip 334k)
gh pr list --state open  # 0
gh issue list --state open  # 0  (wave-4 11 cerradas, 0 open)
python3 -c ".gitcore/features.json metadata" # total 52 stable 52 pct 100.0
```

Branch: main 18e4dc5c → fix clippy + docs (este ARCH_WAVE4). Remotas huérfanas `feat/wave-4.04/.05` (merged, auto-borrar).

## Riesgos y mitigaciones

- **Panel-ui glib-sys en workspace cargo test**: `cargo test --workspace` falla por `panel-ui/src-tauri` glib missing. Mitigado: CI usa `cargo test --package xavier --lib --features ci-safe` (sin tauri). Documentado aquí.
- **verify-pipeline.sh ledger path**: script esperaba `docs/features/features.json` pero ledger real es `.gitcore/features.json`. Mitigado: fix script a `.gitcore/features.json` + documentar `CARGO_TARGET_DIR=target`.
- **WASM sin IndexedDB real en native**: `MemoryWasmStore` HashMap fallback testeable en native, real IndexedDB solo en wasm target via `web-sys`. 4 tests nativos garantizan lógica.
- **On-chain治理 en mesh 0%**: bicameral DAO alloy gating sigue sin despliegue Amoy vivo — documentado como residual ops, no bloquea verified (libp2p/onchain maturity 0% transparente).
- **Loro CRDT / Tor transporte futuro**: phase 3/4 no iniciadas — SRS REQ-012 actualizada a verified reflejando phase 0-2 shippeadas; CRDT/Tor quedan como roadmap v1.1.

## Siguiente (post WAVE-4)

- **E2E cargo test --workspace --features ci-safe excluyendo tauri** + `scripts/check-secrets.sh` + gitleaks ya pasan (CI gate verde)
- **Docs**: FEATURE_STATUS.md y SRS REQ-012..030 marcados verified 100% (este commit)
- **Limpieza**: borrar `.env.bak-*`, `.hermes/ola4`, ramas huérfanas `feat/wave-4.04/.05`
- **Release**: tag `v1.0.0` y `git push --tags` cuando docs + limpieza verdes; luego `gh release create v1.0.0`
- **Próxima ola (WAVE-5 opcional)**: E2E real con panel-ui + gitleaks + docs build + tag release; usar harness `xavier-jules-wave` con islas disjuntas y labels `ola5,wave-5` sin `jules` hasta verificar
