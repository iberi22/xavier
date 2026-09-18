# Extracción de lógicas Ripwire + Graphify → Xavier

> **Fases F1–F6 completas. Olas O1–O7 IMPLEMENTADAS 2026-09-18 con TDD** (cada una 80% `planned`, pendiente `verify-pipeline.sh` para promoción a `stable`). Fuentes aisladas, inventariadas, mapeadas contra Xavier y convertidas en 7 features GitCore + 7 REQs + 2 ADRs.

## 1. Fuentes (F1)

| Repo | Commit pineado | Licencia | Temporal | Alcance extraído |
|---|---|---|---|---|
| `redhat-et/ripwire` (CLI C++23 + MCP, call-graph determinista) | `f45087a779b1988da19e967c3ca4f9c79a7e6d77` | Apache-2.0 | `/tmp/opencode/ripwire` (7.7M) | `docs/{ARCHITECTURE,COMMANDS,EVALS,LINEAGE,LIMITS,METHODOLOGY,TUNING}.md`, 17 skills, `queries/rust+python/tags.scm` |
| `Graphify-Labs/graphify` rama `v8` (knowledge graph tree-sitter, sin vectores) | `26b02b5e3430e4ab85dd7e72c7b98836d8e65c48` | Apache-2.0 | `/tmp/opencode/graphify` (1.9M) | `extract.py`, `extractors/{engine,base,models,rust}.py`, `cluster.py`, `analyze.py`, `paths.py`, `build.py`, `symbol_resolution.py`, `resolver_registry.py`, refs `query` + `extraction-spec` (opencode/claude), `how-it-works.md` |

Técnica: `git clone --filter=blob:none --no-checkout --depth 1` + checkout selectivo (el clon completo no terminó ni en 10 min por red lenta; `third_party/`, `bench/`, `present/`, assets binarios excluidos — no se necesitan para extraer lógicas). **Port por reescritura Rust nativa**, nunca copy-paste (C++/Python → Rust, AGPL-3.0 compatible con atribución aquí y en `THIRD_PARTY.md` al implementar).

## 2. Herramientas Xavier usadas en este diseño (dogfooding)

- `xavier_codegraph_explore("blast_radius")` → confirmó `QueryEngine::blast_radius` en `code-graph/src/query/mod.rs:220-283` (complejidad 17), handler en `src/cli/handlers/code.rs:1337`, test existente `:391`.
- `xavier_trace_path("blast_radius", reverse)` → callers reales con estrategia y confidence (`SameModule` 0.90/0.72, `UniqueName` 0.75/0.60): el propio code-graph ya etiqueta incertidumbre, pero sin norma única — justifica O1.
- `xavier_mem_search` → recuperó memorias F2/F3/F4 del proyecto (`01M2S41F8…`, `01M2S5MK1…`): continuidad entre sesiones verificada.

## 3. Tabla de extracción: qué, de dónde, por qué

### Ripwire → Xavier

| # | Lógica | Fuente exacta | Por qué se extrae | Estado Xavier (F4) | Destino |
|---|---|---|---|---|---|
| R1 | Honestidad etiquetada: `counts_floor`, `amb=K` split `1/k`, `shown/total/capped/truncated`, rehusar+did-you-mean antes que falso 0; `0` = "none found" jamás "none exists" | `docs/COMMANDS.md` (`--callers/callees/impact/uses`), `ARCHITECTURE.md §4`, `METHODOLOGY.md §6,§9` | Los agentes toman un 0 silencioso como verdad y rompen código. Xavier alimenta agentes: no puede mentir por omisión | NO (solo `relevance_threshold 0.5` en `src/retrieval/gating.rs:99`) | O1 `feat-cg-honest-confidence` |
| R2 | Blast-radius transitivo (`--impact`, set `reaches` floor) + use-sites por rol (`--uses`: call/read/write/import/extends/type) + tests-to-run (`--affected/--situ/--test-gate`, exit 4 si hay radio sin test, `run=` solo si derivable) | `COMMANDS.md` (`--impact/affected/situ/test-gate`), `EVALS.md` (Shotgun Surgery precision@8 .35/.43) | Pasar de "quién llama" (1-hop) a "qué rompo y qué test lo cubre" cierra regresiones de agentes (−70% TDAD publicado) | PARCIAL (`blast_radius` BFS Calls depth≤8 `code-graph/src/query/mod.rs:220-283`, `traverse/reverse`; `impact` comentado en `lib.rs:11`) | O3 `feat-cg-blast-testgate` |
| R3 | Token-budget: `bundle=compact` (firmas sin cuerpos), `max-tokens` como shape, `token-budget` como gate (exit 3), cuotas fijas por sección en `pack-task` (40/30/15/5/10 + roll-forward) | `ARCHITECTURE.md §rank/serialize`, `COMMANDS.md` (`--for --pack-task --top-k/max-tokens/token-budget`), `LINEAGE.md §1-2` (LongCodeBench/LostInMiddle) | Terminalidad bajo techo: 1 pregunta = 1 respuesta completa sin relecturas que cuestan 2.3×–324× tokens | PARCIAL (XCP `src/memory/pack.rs`, `ContextBudgetConfig` en `src/context/orchestrator.rs:418-492`; sin gate en code) | O5 `feat-cg-budget-query` |
| R4 | Rankeo: peso arista `(media conf)·√nref cap8` sin self-loops + PageRank (α=.85, teleport β=.7 a ficheros cambiados) + router `name-exact BM25 (k1=1.5 b=.75)` vs `subtoken+body` con `confidence/margin_pct` y corte `--adaptive` | `ARCHITECTURE.md §1-3`, `COMMANDS.md` (`--rank-by`, `--adaptive`, `--map-diff`) | Robustez a hot-loops/sinks + retrieval sin embeddings calibrado; el teleport a cambiados da `--map-diff` gratis | PARCIAL (RRF k=60 sí; sin PageRank/router/margin en code) | O6 `feat-cg-pagerank-router` |
| R5 | Contratos pre-merge: `--edit-check` (unchanged/new-symbol/contract-change vs HEAD + aridad incompatible), `--safe-delete` (risk=none-found/untested-radius/uses-exist, nunca veredicto), `--verify` (confirmed/refuted/not-established) | `COMMANDS.md` (`--edit-check/safe-delete/verify`) | El gate que impide merges que cambian contratos sin tests; `verify` de 3 valores es la honestidad R1 aplicada a claims | NO (solo `debug.rs:69` genérico + prune) | O7 `feat-cg-contracts-arch` |
| R6 | Gramática Rust tree-sitter (`tags.scm`): fns, métodos con fix de span, `trait`, `Self::`, turbofish `f::<T>`, `Scope::name`, sin `@reference.implementation` | `queries/rust/tags.scm` (vendored tree-sitter-rust v0.23.2) | Plantilla directa para endurecer `code-graph/src/parser/rust.rs` | SÍ (reusar, no portar) | — (referencia) |

### Graphify → Xavier

| # | Lógica | Fuente exacta | Por qué se extrae | Estado Xavier (F4) | Destino |
|---|---|---|---|---|---|
| G1 | Rúbrica confidence discreta: `EXTRACTED=1.0` / `INFERRED ∈ {0.95,0.85,0.75,0.65,0.55}` / `AMBIGUOUS ∈ 0.1-0.3`, nunca `0.5` | `docs/how-it-works.md#Confidence tagging`, `extraction-spec.md#confidence_score`, `symbol_resolution.py` (0.85 cross-file) | Unifica las 3 escalas actuales de Xavier (edges 0.3-1.0, belief 0.3/0.6/0.9, score 10/5/1) y calibra la fusión RRF | PARCIAL | O1 `feat-cg-honest-confidence` |
| G2 | `LanguageConfig` (gramática+tipos+accessors+handlers) + `resolver_registry` (registro ordenado por sufijo, warn-and-continue) | `extractors/models.py`, `extractors/engine.py`, `resolver_registry.py`, `ARCHITECTURE.md#Adding` | Añadir lenguajes sin tocar el núcleo; hoy el dispatch está disperso (`Language::from_extension`, `chain_for`) | PARCIAL | O2 `feat-cg-language-registry` |
| G3 | IDs `{fullpath}_{símbolo}` anti-colisión + incremental (`manifest.json` + cache por hash de contenido + `build_merge` con prune de borrados) + rewire de fantasmas | `extractors/base.py#_file_stem`, `build.py#build_merge`, `cache.py`, `dedup.py` | Xavier usa `mtime` (falsos positivos) y `project_id="default"` fijo (bloquea multi-proyecto); hash-contenido + manifest = reindex solo-diff | PARCIAL | O4 `feat-cg-incremental-ids` |
| G4 | `raw_calls` en 2 fases (intra-fichero EXTRACTED por `label_to_nid`, cross INFERRED conservador + god-guard que salta hubs ambiguos) | `extractors/rust.py`, `symbol_resolution.py#resolve_cross_file_raw_calls` | Precisión cross-file sin explosión de falsos positivos; reemplaza el regex de 1 fase (`call_resolution.rs:229-248`) | NO | O4 `feat-cg-incremental-ids` |
| G5 | Leiden (seed 42, determinista) + `exclude_hubs_percentile` con re-anexo por voto + label-por-hub sin LLM + cohesión | `cluster.py` | Clustering de subsistemas sin embeddings ni LLM; solo `hub_nodes` por grado existe hoy | NO | O8 opcional (dentro de O7 como extensión) |
| G6 | Query barata previa al vector: expansión obligada contra `.vocab.txt` (≤12 tokens solo-vocab, 0→parar) + BFS prof.3 (qué conectado) vs DFS prof.6 (cómo llega) + `--budget` | `skills/opencode+claude/references/query.md`, `paths.py` | Retrieval auditable y barato antes de gastar vector; `traverse d≤8` existe pero sin vocab-constrain ni budget | PARCIAL | O5 `feat-cg-budget-query` |
| G7 | Señales arquitectura: god-nodes (top grado con blocklist builtins), surprising connections (cross-file rankeadas), import-cycles (`simple_cycles ≤5`), rationale nodes (`NOTE/WHY/HACK`, ADRs como nodos) | `analyze.py`, `how-it-works.md`, `node-summaries-rfc.md` | Superficie lista para UI/MCP (`graph-explorer`, `trace_path`): lo sorprendente es lo que el agente debe ver primero | PARCIAL (solo hubs/hotspots riesgo) | O7 `feat-cg-contracts-arch` |

## 4. Decisiones (ver ADRs 032 y 033)

- **Reescritura Rust nativa**, atribución obligatoria (licencias Apache-2.0 → AGPL-3.0 lo permiten con aviso).
- **TDD red-first siempre; smoke solo como gate post-deploy** (ver §6) — responde la duda original: smoke no sustituye tests.
- **1 feature por ola**, orden por dependencia técnica (O1→O7, O8 opcional).
- Deps nuevas: ninguna obligatoria en O1–O5; `petgraph` (hoy solo transitiva) para O6; Leiden O8 se vendorigiza o se implementa Louvain propio (sin crate estándar).

## 5. Olas → features → REQs

| Ola | Feature (`planned`) | REQ | Spec |
|---|---|---|---|
| O1 | `feat-cg-honest-confidence` | REQ-053 | `FEATURE-feat-cg-honest-confidence.md` |
| O2 | `feat-cg-language-registry` | REQ-054 | `FEATURE-feat-cg-language-registry.md` |
| O3 | `feat-cg-blast-testgate` | REQ-055 | `FEATURE-feat-cg-blast-testgate.md` |
| O4 | `feat-cg-incremental-ids` | REQ-056 | `FEATURE-feat-cg-incremental-ids.md` |
| O5 | `feat-cg-budget-query` | REQ-057 | `FEATURE-feat-cg-budget-query.md` |
| O6 | `feat-cg-pagerank-router` | REQ-058 | `FEATURE-feat-cg-pagerank-router.md` |
| O7 | `feat-cg-contracts-arch` | REQ-059 | `FEATURE-feat-cg-contracts-arch.md` |

Historias US-101..US-114 y tests TDD nominales en cada spec.

## 6. Estrategia de tests (cierra F6 y la duda smoke)

Pirámide Rust: unit+doc (muchos, `#[cfg(test)]`, doctests como contrato público) → property (`proptest`: invariantes `reaches` monotónico, `shown+withheld=total`, IDs estables) → integración `tests/*.rs` (API pública) → **smoke como gate** (`/health`, 1 ciclo write/read, `verify-pipeline.sh`), nunca como sustituto. TDD red-green-refactor por ola: test nominal primero (falla), implementación mínima, refactor con `cargo test` en verde. Cada spec lista sus tests `test_*` antes que el código exista.

## 7. Verificación por ola (F8)

`cargo fmt` + `cargo clippy --all-targets -- -D warnings` + `cargo test --workspace` (o `scripts/verify-pipeline.sh` como juez) + bench tokens/latencia antes/después + `scripts/check-secrets.sh`. Promoción `planned→stable` solo con run verde; memoria POST a Xavier `projects/xavier/`.
