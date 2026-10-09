# ADR-042 — Native in-process embedding (EmbeddingGemma 2 via ONNX Runtime)

| Field | Value |
|-------|-------|
| **ID** | ADR-042 |
| **Status** | Accepted (2026-10-07) |
| **Date** | 2026-10-07 |
| **Authors** | ARCHITECT (🏛️) |
| **Related** | ADR-016 (canonical data dir), ADR-019 (plugin-first), ADR-033 (hexagonal crates + fallback), ADR-035 (trunk-based ship/show/ask), ADR-036 (tree retrieval precedent) |
| **Durable path** | `docs/adr/ADR-042-native-embeddinggemma2.md` + entry in `docs/adr/README.md` (this file under `.gitcore/sdd/` is ephemeral) |

## Context

Xavier produces embeddings only via an external HTTP service (Ollama /
OpenAI-compatible) or `gllm`+candle behind a feature (`src/embedding/`:
`ollama.rs`, `openai.rs`, `gllm.rs`, `FallbackEmbedder` +
`CircuitBreakerEmbedder` in `mod.rs`). No ONNX runtime exists in the
dependency tree (`ort`, `fastembed`, `llama-cpp-2` are absent from
`Cargo.toml`, verified on `origin/main`). The default local model is
`embeddinggemma` v1 via Ollama, which requires an external daemon.

A measurement spike (CPU bench, public material + repo, no builds in the
repo) shows the EmbeddingGemma 2 text encoder runs in-process via ONNX
Runtime against a **community-exported ONNX artifact** (`onnx-community`
build: `model.onnx` + `model.onnx_data` fp32). All spike numbers below are
**provisional and belong to that community artifact, not to the artifact
this ADR adopts**. Reference host: reference x86-64 desktop, 12 threads.
Results: mean onnx-vs-reference cosine 1.00000 on a small fixture,
recall ≥ current baseline (R@1 0.60 vs 0.56, R@5 1.00 vs 0.94, n=50 public
ADR pairs), 768d = current production dimension
(`DEFAULT_EMBEDDING_DIMENSIONS = 768`,
`src/memory/sqlite_vec_store/config.rs:11`, table `memory_embeddings_768`
in `src/storage/migrations.rs`). Active constraints: ADR-016 (canonical
data dir), ADR-019 (plugin-first: no new native code in core without
isolation), ADR-033 (crates as walls + explicit observable fallback chains).

## Competing options

| Option | Description | Expected cost/complexity |
|--------|-------------|--------------------------|
| A — `ort` ONNX fp32 768d (proposed) | Text encoder via ONNX Runtime in a separate crate, behind `EmbeddingPort`; project-exported ONNX from the official safetensors | Medium: new crate + export script + backfill; heavy native dep but isolated behind a default-OFF feature |
| B — Keep external Ollama | No repo change; EG-2 via `ollama pull` when available | Zero repo cost; operational cost: mandatory external daemon, no in-process embedder |
| C — Pure-Rust `candle` | Reuse optional `candle-core`; port the Gemma-4 architecture + pooling + Matryoshka | High: no candle checkpoint exists for EG-2; weeks, not days — rejected for v1 |
| D — GGUF via `llama.cpp` | Community Q8_0 (~310 MB); good CPU/RAM history, listed on the official blog | Lower inference cost, but NOT measured in the spike; needs its own mini-spike before displacing A |
| E — Dynamic int8 ONNX | Same runtime as A, peak RSS 847→555 MB | Rejected for the latency path: 4.2x slower at batch-1 (1475 vs 351 ms); only pays off at batch-32 |

## Simulation (REQUIRED)

> Multi-scenario comparison anchored in measured bench inputs. The formal
> run with the engine (`adr_sim.py --runs 5000 --seed 42`) is an acceptance
> gate before leaving Proposed status.

- **Engine:** `periferia/swal-sim/adr_sim.py` (pinned revision recorded in the report; `--model embeddinggemma2-native` must exist at that revision)
- **Command:** `python3 adr_sim.py --model embeddinggemma2-native --runs 5000 --seed 42 --outdir periferia/swal-sim/reports`
- **Runs / seed:** 5000 / 42
- **Result:** **pending** (formal run not yet executed — see acceptance P0)
- **Agreement:** **pending**
- **Sensitivity:** **pending** (unknown until run; cf. ADR-036:60-62 precedent)
- **Report:** `periferia/swal-sim/reports/ADR-042-embeddinggemma2-native.md` (+ `.json`) — produced in PR-1

## Assumptions and anchors

| Parameter | Value used | Source |
|-----------|------------|--------|
| Recall EG-2 fp32 R@1/R@5 | 0.60 / 1.00 (n=50, ADR domain, with mandatory prefixes) | spike on **community** artifact, reference x86-64 desktop, 12 threads — PROVISIONAL until PR-0 re-measures the project export |
| Current baseline R@1/R@5 | 0.56 / 0.94 (effective fallback model on the host) | same eval, n=50 — frozen JSON baseline committed in PR-0 replaces this |
| Latency ORT fp32 | 351 ms b1, 122 ms/text b32; peak RSS 847 MB | spike on community artifact — PROVISIONAL (see PR-0) |
| Dynamic int8 b1 | 1475 ms (4.2x regression) → rejected for latency path | spike measured |
| 256d ≈ 768d recall | no loss in this eval (consistent with the model card down to 256d) | spike + public model card |
| Official weight license | Apache-2.0 | public model card (verified) |
| Existing ONNX/GGUF | community only; official weights are safetensors-only | HF listing verified 2026-10-07 |
| Parity onnx-vs-official | mean cosine 1.00000 (small fixture) | spike on community artifact — replaced by the PR-0 numerical gate below |
| +9.92 MTEB-code gain | NOT proven on our corpus (no code queries in n=50 eval) | **ASSUMPTION** — requires the parity gate with a larger fixture; withdrawn as a promotion criterion |
| Final binary size with `ort` | unmeasured (heavy native dep) | **ASSUMPTION** — measured in PR-2 and recorded |
| Project-exported ONNX latency/RSS | unmeasured | **ASSUMPTION** — measured in PR-0 on the adopted artifact |
| System onnxruntime availability (NixOS) | unmeasured | **ASSUMPTION** — anchored via `nix eval nixpkgs#onnxruntime.version` in PR-0 |

## Decision

Integrate in-process embedding with `ort` (ONNX Runtime), fp32, 768d only,
behind `EmbeddingPort` (`src/ports/outbound/embedding_port.rs`), in a
separate crate **`crates/xavier-embed-ort`** behind feature `embed-native`
(default OFF), per ADR-019 (crate-vs-feature: **crate**, because ADR-033
declares crates as the isolation walls; a feature on the root crate does
not isolate the native dependency from default builds or library
consumers) and ADR-033 (the backend enters **first** in the fallback order:
native → Ollama → OpenAI-compatible local; the current provider stays
intact as an observable fallback; BM25/FTS-only degradation under the
existing `XAVIER_EMBEDDING_FALLBACK_BUDGET_MS` budget and `/health
degraded`).

**B1 — ort acquisition/linking (normative).** The `ort` dependency is
declared as `ort = { version = "=2.0.0-rc.12", default-features =
false, features = ["load-dynamic", "api-18"] }` (exact pre-release pin — no stable
`2.x` exists on crates.io, so plain `version = "2"` does not resolve),
**never linked at build time**. The ONNX Runtime shared library is
provisioned at runtime by the operator via `ORT_DYLIB_PATH` (proven by the PR-2 build/link gate, not by this text: `load-dynamic` implies `ort-sys/disable-linking`,
whose build script performs no download and no link). A `pkg-config`
resolution against a system `onnxruntime` is an **alternative feature
set**, not a second step under `load-dynamic` (whose build script returns
before any probe; NixOS anchor: `nix eval nixpkgs#onnxruntime.version`,
mirroring ADR-036:70-71/86-89). **`download-binaries` and `copy-dylibs`
are explicitly forbidden.** Default and `ci-safe` builds require no native
library. Requirement row: *default/ci-safe build needs no onnxruntime
native lib | required | AGENTS.md §2, `.github/workflows/ci.yml`
check/clippy/test jobs*. PR-2 includes a build/link gate (`cargo check`
with and without `embed-native`, plus an offline link smoke test on NixOS).

**B4 — crate contract and workspace membership (normative).** The new crate
is a **workspace MEMBER**, listed explicitly in root `[workspace] members`
(an in-tree path dependency is a member regardless — `cargo metadata`
lists it under `workspace_members` — so this ADR states membership instead
of claiming exclusion; R2-2 resolved as member, not excluded). All
`-p xavier-embed-ort` commands are therefore valid. Isolation comes from
the default-OFF `embed-native` feature plus `load-dynamic` (the member
compiles with no native link and needs no native lib), and CI is unaffected
(it uses `--package xavier --features ci-safe`, never `--workspace`). It exposes a flat
API (`encode_batch`, `dim`, `space_id`) and does **not** depend on
`xavier-ports` (which does not exist as a crate in this revision; only
`src/ports/`). The `Embedder` impl lives at the root in
`src/embedding/native.rs`; composition/glue stays at the root per ADR-033.
Naming: `xavier-embed-ort` is an engine crate; the `xavier-adapter-*`
naming of ADR-033 §1 is aspirational for provider adapters. The chain type
extended is the real `FallbackEmbedder` (`src/embedding/mod.rs`), not the
Wave-33 generic `FallbackChain<P>` which does not exist yet.

**B2 — model artifact (normative).** The canonical artifact is a
**project-exported ONNX graph from the official safetensors**
(`google/embeddinggemma-2`, Apache-2.0). Community ONNX/GGUF blobs are never
canonical. **PR-0 (before any backend code)** reproducibly exports the
official safetensors to ONNX, checks parity against the official reference,
and re-runs the recall/latency spike on THAT artifact; all provisional
numbers above are replaced by PR-0 measurements. The export manifest pins:
exporter toolchain versions (torch/transformers/optimum) + deterministic
flags + the text-only slicing procedure + graph input/output names, dtypes,
ranks/shapes (including required empty modality inputs) + host (release
asset) + sha256 of every file, verified from `SHA256SUMS` before use.
Reference for parity: sentence-transformers fp32 CPU. Numerical gate
(replaces "cosine = 1.0" / "bit-identical"): minimum cosine ≥ 0.9999 per
vector, maximum absolute element error ≤ 1e-5, norm error ≤ 1e-5, all
components finite. Sequencing (R5): PR-0 proves the gate via the Python
reference (sentence-transformers + onnxruntime) since no Rust exists yet;
PR-2 re-runs the identical gate through the **Rust** backend before
adoption — the Rust result, never only the Python one, promotes.

**B3 — vector migration (normative, shadow storage).** Same dimension does
not mean the same vector space: re-embedding in place under `INSERT OR
REPLACE` would destroy the legacy space row by row with no rollback and no
A/B comparison (`backend_impl.rs`, `migrations.rs`). Therefore v1 uses
**shadow storage per model generation**: a new table keyed by
`(workspace_id, memory_id, generation_id)` carrying `space_id` (the
canonical manifest fingerprint), `dim`, and the embedding BLOB; legacy
rows in `memory_embeddings` / `memory_embeddings_768` are never
overwritten. Dual reads encode one query per space, rank each space
separately, fuse ranks (never raw cross-space cosine values), and
deduplicate by memory identity. Cutover is atomic per workspace (single
transaction flipping the workspace's active `generation_id`, recorded with
`embedding_status`); **rollback is flipping the pointer back in one
command**, with old vectors retained until the cleanup gate passes.
Backfill checkpoints are keyed by `(workspace_id, target_generation,
source_revision, memory_id)` with transactional row-write + progress
updates; failures stay retryable. During cutover, every write/delete targets
the generation that is active at commit time (the active generation is
re-read inside the write transaction, so a write racing the flip cannot land
in the deactivated generation uncovered). Each shadow row records the source
content revision/hash it was computed from and commits only if that revision
is still current, otherwise it is discarded and re-queued; reads and the
cutover completeness check reject stale rows. The rollback generation stays
eligible only while reconciled with current content (post-cutover
inserts/edits/deletes are tracked for catch-up); an unreconciled rollback is
refused, never silently stale. A crash between the cutover transaction and
cleanup leaves the old generation intact, so rollback remains one command.
Proven by PR-3 tests `backfill_race_never_mixes_generations`,
`cutover_crash_recovery_keeps_one_generation`, and
`rollback_after_post_cutover_writes_reconciles`. Write-time dimension check per ADR-019:
any vector whose length mismatches the active generation is rejected at
write time, never repaired at query time.

**B6/B7 — configuration (normative).** v1 accepts **only 768d**. Explicit
startup state table — one behavior per state, never a silent semantic change
per ADR-033 §4: (a) `XAVIER_EMBEDDER=native` (alias `embeddinggemma2`)
without the `embed-native` feature compiled in → hard startup error
(fail-fast, following the `gllm.rs:38-51` precedent and the `Invalid` variant
already in `EmbedderConfig`); (b) invalid dimension
(`XAVIER_NATIVE_EMBEDDING_DIM != XAVIER_EMBEDDING_DIMENSIONS`, conflicting
settings, stored-identity mismatch) → hard startup/write/query error;
(c) feature compiled + explicit native + artifact absent at startup → hard
startup error, unless `XAVIER_EMBEDDING_ALLOW_DOWNLOAD=1`, in which case
acquisition runs **before** the check and its failure is still a hard error;
(d) loss or failure of an already-verified artifact at **runtime** → typed
`ModelUnavailable`, fallback to Ollama + `/health degraded` (observable).
This resolves the R6 contradiction: startup-missing is an error, runtime-loss
is fallback — never both. PR-2 fixes `.env.example:112`
from 384 to 768 (not a comment note). Model cache
lives under the resolved data dir
(`XavierSettings::resolve_data_dir().join("models")`, ADR-016), layout
`models/embeddinggemma-2/<variant>/` (snapshot style + `SHA256SUMS`);
download happens only via an explicit `xavier models fetch` command or
first start with `XAVIER_EMBEDDING_ALLOW_DOWNLOAD=1` (one-shot, explicit
log); offline with no artifact is an explicit error, never a silent
download. `xavier models fetch` reuses or explicitly supersedes the
existing `POST /v1/offline/download` path (`src/cli/server.rs`) and the
`data/models/` convention (`src/cli/handlers/offline_models.rs:78`).
`--check` means presence + sha256 of every `SHA256SUMS` entry; downloads
are atomic (temp + rename) with locking, disk preflight, and partial
cleanup. Attribution: `LICENSE` + `NOTICE`/`THIRD-PARTY-NOTICES` under the
model dir (weights Apache-2.0, ONNX Runtime MIT, export toolchain
licenses); the artifact is never versioned in git.

**Preprocessing / output contract (normative).** `encode(&str)` cannot
distinguish roles, so the backend exposes a typed contract (minimum):

```rust
enum EmbeddingInput {
    Query { task: RetrievalTask, text: String },
    Document { title: Option<String>, content: String },
}
struct EmbeddingSpaceId(String); // canonical manifest fingerprint
struct EmbeddedVector { space: EmbeddingSpaceId, values: Vec<f32> }
```

Tokenizer: the pinned official `tokenizer.json` via the Rust `tokenizers`
crate (hash-pinned with the checkpoint; token IDs/masks tested against the
reference — no Gemma-v1 or ad-hoc SentencePiece substitution without an
independent reproduction proof). Prompts follow the official model card,
applied exactly once before tokenization: queries `task: search result |
query: …` (`task: code retrieval | query: …` for code), documents `title:
{title} | text: {content}`, absent title rendered literally as
`title: none | text: {content}`; task selection is caller-provided, never
guessed from content. Tokenizer config: pinned `tokenizer.json` plus pinned
postprocessor/special-token insertion policy recorded in the manifest; token
IDs/masks tested against the reference. Limits: **8192 total tokens including
prefixes/special tokens**; right-side truncation with observable token counts
and `InvalidInput` only for empty/whitespace-only canonical input — never
byte/character slicing; right-padding with the pinned pad ID and zero
attention mask. Tensors: `input_ids`/`attention_mask` int64,
`sentence_embedding` float32 `[batch,768]`. The export bakes pooling into the
graph, which MUST expose a final L2-normalized `sentence_embedding` output
(checkpoint projection + mask-aware mean pooling with `include_prompt=true`,
L2 epsilon 1e-12; zero-norm → typed `InvalidOutput`, never a silent vector)
— no CLS path, no external-pooling variant. Canonical input is `trim()`med
(matching `cache.rs`) with no interior collapse in v1, covered by the space
fingerprint policy version. Batches return ordered results, all-or-nothing
with typed per-batch errors. Finite values, length, and nonzero norm are
checked before accepting a result; padding and batch invariance are tested
(PR-2 test `tokenizer_contract_covers_unicode_whitespace_empty_boundary`). Dimension policy: v1
rejects any `DIM != 768`; phase-2 Matryoshka allows only leading slices
512/256/128 followed by L2 renormalization (never averaging, projection,
or padding), each requiring explicit schema/routing + evaluation changes.

**Concurrency / resources (normative).** All synchronous model work
(loading, tokenization, tensor prep, ORT runs, CPU postprocessing) runs on
`spawn_blocking`/dedicated blocking workers — never Rayon `par_iter()`
directly on a Tokio worker. One shared loaded session initially; a bounded
admission semaphore is acquired **before** spawning/allocating and held
inside the blocking job until real completion (async timeouts do not
release it; no overlapping retries). v1 limits (hash-pinned runtime-policy
manifest; missing/unbounded values rejected at startup): max in-flight runs
2, max waiting requests 32, max input bytes 256 KB per request (bounded before
retaining/allocating), max batch 32, max padded-token budget 32×8192; ORT
intra-op threads = physical cores/2, inter-op 1; overload → typed
`Overloaded`, expired waiters/cancel → `DeadlineExceeded` (no work retained);
backfill shares the same scheduler with interactive priority. Latency gate
statistic: p95 batch-1 ≤ 500 ms on the fixture length profile. The ≤ 1 GB
budget is incremental embedder RSS in decimal GB at the maximum admitted
workload including pending queue + cache + backfill (p50/p95 + mean
reported) — measured in a dedicated bench job on provisioned hardware, never
asserted on shared CI runners (PR-2 test
`admission_queue_rejects_overload_with_typed_error`).

**Cache identity (normative).** Both memory and SQLite caches are namespaced
by the immutable space fingerprint (model revision/weight hash, exported
graph hash/version, tokenizer hash, preprocessing/pooling policy version,
precision/variant, dimension, normalization policy) plus role/task, title,
and exact canonical input. Fallback output is cached under the successful
provider's space, never the failed primary's; legacy entries are misses.
Dimension/finite/norm/identity are validated on load.

**Cross-model fallback isolation (normative).** Native EG-2 → Ollama EG-1 /
nomic is not a transparent vector fallback: vectors from different spaces
must never mix in one table, rank list, or cache namespace. Transparent
fallback is restricted to an equivalent artifact/preprocessing contract
proven by parity; otherwise the system degrades to BM25 or queries a
separately retained legacy corpus with its own legacy query encoder. A
fallback vector is never written into or searched against the native
generation. End-to-end (R1, PR-2/PR-3): `EmbeddingPort` gains typed
`embed_typed(EmbeddingInput) -> EmbeddedVector`; the fallback decorator returns
the **successful** provider's tagged space (never the primary's config);
query and ingest callers thread the tag to the storage boundary, which rejects
space/generation mismatch before scoring or writing — the native path never
passes through the bare-vector interface untagged. Proven by PR-3 test
`fallback_vector_never_enters_wrong_generation`.

PARITY GATE (promotion to default, blocking pre-promotion — not shared-CI
pass/fail): all current embedding tests green (enumerated from the research
report §2.4) + a recall eval on a fixed, committed gold fixture (**n≥200
pairs, 100 general-memory ES/EN + 100 code, pinned corpus + relevance
labels + tie-breaking**, correct prefixes per side) against a **frozen
committed baseline JSON** (artifact/fixture/reference/baseline hashes
pinned) with thresholds **R@1 ≥ baseline and R@5 ≥ baseline, plus strict
improvement on a pre-selected code metric (R@1 or R@5, declared in the
manifest; strict = strictly larger integer hit count)** + budgets
**batch-1 latency ≤ 500 ms and peak RSS ≤ 1 GB** on the reference x86-64
desktop in the dedicated bench job. Binary size and Apache-2.0 license
recorded. ARM/mobile (LiteRT/MLX are separate runtimes, separate ADR) is
out of v1. If the gate fails, the default is not promoted and the +9.92
claim stays withdrawn.

## Consequences

- **Positive:** no external daemon for embeddings; recall ≥ baseline on the
  adopted artifact with a declared numerical parity proof; migration without
  destroying the legacy space; current provider kept as an observable fallback.
- **Negative / cost:** `ort` grows the binary and CI surface (mitigated:
  separate member crate, default-OFF feature, no native link, excluded from
  `ci-safe`); ~0.85 GB peak RSS in fp32 (provisional, re-measured in PR-0);
  full background re-embedding; export-script maintenance on checkpoint revisions.
- **What would invalidate this decision:** the PR-0 parity gate on the
  project export fails; the PR-2 binary measurement exceeds the budget set
  there; a GGUF/`llama.cpp` mini-spike shows ≥ recall with strictly better
  latency/RAM under equal artifact reproducibility.

## Post-verification

Observable metric: recall on the fixed eval + R@5 in production after
cutover, and the fallback-to-Ollama rate (should tend to 0 except for
model-offline). Review date: one cycle after promotion to default;
re-evaluate GGUF as primary and Matryoshka-256d as phase 2 then.

## Deterministic acceptance (executable checks)

- P0: formal `adr_sim.py --runs 5000 --seed 42` run published in
  `periferia/swal-sim/reports/` (status stays Proposed without it).
- P1: `cargo test -p xavier embedding` green (default features, shared CI) +
  `cargo test -p xavier --features embed-native --test embed_native_contract` green (native-only target incl. mandatory test `startup_missing_artifact_is_hard_error`, pre-promotion-only — needs dylib + artifact; never a bare `-- --ignored` that also pulls the keyring test — every gate script asserts expected test names plus nonzero counts and fails on 0 tests run).
- P2: `cargo run -p xavier --features embed-native --bin xavier -- models fetch --check` green + offline-guard test green (no network in CI); semantics per the Decision section.
- P3: `cargo test -p xavier --features embed-native --test embed_recall -- --ignored --nocapture` green with printed thresholds (n≥200 fixture, R@1/R@5 ≥ frozen baseline, strict code-metric improvement); `cargo test -p xavier-embed-ort --test tokenizer_contract` green; pre-promotion only, with the provisioned artifact present — missing artifact fails the gate, never skips. This is the Rust-side numerical gate that supersedes the PR-0 Python reference parity.
- P4: `cargo test -p xavier --features embed-native --test embed_native_migration` green (mandatory tests `backfill_race_never_mixes_generations`, `cutover_crash_recovery_keeps_one_generation`, `rollback_after_post_cutover_writes_reconciles`): idempotent/resumable backfill, shadow-table isolation, per-workspace atomic cutover + rollback, `:memory:`/tempdir only, nonzero test count asserted.
- P5: `cargo test -p xavier --features embed-native --test embed_native_fallback` green (mandatory tests `fallback_vector_never_enters_wrong_generation`, `runtime_artifact_loss_falls_back_and_reports_degraded`): native failure degrades to BM25 or the legacy space with its own encoder; asserts a fallback vector can never be written to / searched in the wrong generation (typed in-process errors, not bare HTTP 500).
- P6: `cargo clippy -p xavier -p xavier-embed-ort --features embed-native --all-targets -- -D warnings`, `cargo test -p xavier --no-default-features --features ci-safe embedding`, and `cargo fmt --all --check` green; `ci-safe` excludes `embed-native`.
- Bench (provisioned hardware, not shared CI): `bash scripts/embed-native-bench.sh --manifest tests/fixtures/embed-native/manifest.json --offline` and `bash scripts/verify-embed-native-gates.sh --manifest tests/fixtures/embed-native/manifest.json --offline` green (numerical parity on all fixture rows, aggregate R@1/R@5 ≥ baseline, strict code improvement, latency/RSS/concurrency limits; nonzero on any violation). The gate wrapper asserts expected executed test names and fails on unexpected skips/ignored gate tests or a missing provisioned artifact (missing artifact fails, never skips).

## Files-to-Modify (PR-0 → PR-3)

- **PR-0 — Reproducible export + measured artifact (new, blocking first):** `scripts/export-eg2-onnx.sh` (pinned toolchain + deterministic flags + text-only slicing + sha256 + host as release asset), `tests/fixtures/embed-native/manifest.json` + `SHA256SUMS` (artifact/fixture/reference/baseline hashes), frozen baseline JSON, committed bench harness + fixtures (the spike scripts live in `/tmp` and validated Python onnxruntime, not the Rust integration — the harness makes them reproducible), spec `docs/features/specs/FEATURE-embed-native-parity.md` + REQ IDs, three ledger entries in `.gitcore/features.json` (`feat-embed-native-backend`, `feat-embed-native-data`, `feat-embed-native-parity`, `status: planned`, tests + `implemented_in`), re-run recall/latency/RSS spike on the project export.
- **PR-1 — Recorded spike + parity gate definition:** `docs/spikes/embed-native-ort.md` (ort/candle/gguf/fastembed table with bench measures + GO fp32 / NO-GO int8-b1 verdict), `scripts/embed-native-bench.sh` + `scripts/verify-embed-native-gates.sh` (nonzero-count asserts, offline), `periferia/swal-sim/adr_sim.py` (add the `embeddinggemma2-native` model, F7) + `periferia/swal-sim/reports/ADR-042-embeddinggemma2-native.{md,json}` (formal run), `tests/embed_recall.rs` + gold JSONL fixture (n≥200, root crate — never `crates/xavier-pageindex/src/eval/`, which is standalone with no embedding deps).
- **PR-2 — Isolated `NativeEmbedder` backend:** `crates/xavier-embed-ort/{Cargo.toml,src/lib.rs,AGENTS.md}` (new workspace member listed in root `[workspace] members`; `ort = "=2.0.0-rc.12"` with `default-features = false, features = ["load-dynamic", "api-18"]` only here), `src/embedding/native.rs` (facade: typed `EmbeddingInput` batch encode via `spawn_blocking` + bounded semaphore, mandatory prefixes, `dimension()` from validated artifact metadata = 768, fail-fast config errors), `src/embedding/mod.rs` (wiring + `embeddinggemma-2*` routing + native-first order in `FallbackEmbedder`), root `Cargo.toml` (optional path-dep + `embed-native` feature, default OFF, outside `ci-safe`), `src/ports/outbound/embedding_port.rs` (typed `embed_typed(EmbeddingInput) -> EmbeddedVector` + chain order), `.env.example` (`XAVIER_EMBEDDER=native`, `XAVIER_NATIVE_EMBEDDING_{MODEL_PATH,VARIANT,DIM}`, `XAVIER_EMBEDDING_ALLOW_DOWNLOAD`, and the 384→768 fix), build/link gate for NixOS offline.
- **PR-3 — Data, CLI, and shadow-migration:** `xavier models fetch` (+ `--check`), shadow generation table + per-workspace active-generation registry + atomic cutover/rollback command, `src/bin/backfill_native_embeddings.rs` (resumable checkpoint keyed by workspace+generation, dual-read rank fusion), migration + fallback-isolation tests, `/health degraded` + `doctor` extended to `native`, measured binary/RAM doc and ARM/mobile follow-up note, startup invalidation updated to cover both tables and all vector read/write/delete/reconciliation paths listed.

### Final panel amendments (round 3)
- **ort API level:** `api-18` is mandatory with `default-features = false`; without it `ort =2.0.0-rc.12` does not compile (`ort/src/ep/vitis.rs:47` needs `ort-sys` `api-18`). Minimum runtime ONNX Runtime **≥ 1.18** is recorded in the runtime/artifact manifest and checked at startup (`GetApi` must not return null); the reference runtime (nixpkgs onnxruntime 1.22) satisfies it. PR-2 gate: `cargo check -p xavier-embed-ort --features embed-native --offline` and `cargo clippy --workspace --all-targets` both succeed.
- **Legacy vectors of unknown provenance:** vectors whose model/version cannot be established are never mixed with any generation. Until provenance is proven, vector retrieval over them is disabled (lexical/BM25 only) and they are queued for rebuild into a known generation. Named mandatory test (PR-3): `legacy_vectors_without_provenance_are_excluded_from_vector_search`.

## Revision log

| Blocker | Fix location in this revision |
|---------|-------------------------------|
| Deepseek B1 (ort acquisition/link violates ADR-019, no NixOS build) | Decision §B1: `default-features=false + load-dynamic`, never linked at build time, `ORT_DYLIB_PATH`/pkg-config, `download-binaries`/`copy-dylibs` forbidden, nixpkgs anchor, build/link gate in PR-2 |
| Deepseek B2 (measured ≠ adopted artifact) | New PR-0: reproducible export of official safetensors to ONNX + parity vs official reference (cosine ≥ 0.9999) + spike re-run on THAT artifact; pinned toolchain + sha256 + host; spike numbers labeled provisional |
| Deepseek B3 (migration inconsistent, no rollback, no dimension check, .env drift) | Decision §B3: shadow table keyed by model generation, per-workspace atomic cutover, one-command rollback, write-time dimension rejection; PR-2 fixes `.env.example` 384→768 |
| Deepseek B4 (workspace/CI membership undecided) | Decision §B4 (R1; SUPERSEDED by R2-2 below: member, not excluded); flat API, no `xavier-ports` dep, `Embedder` impl at root; `FallbackEmbedder` named |
| Deepseek B5 (parity gate not executable) | Parity Gate + Acceptance: pre-promotion vs CI split, frozen baseline JSON, native-only test targets (no bare `--ignored`), fixture in root crate, latency/RSS in dedicated bench job, nonzero-count asserts, code-metric failure rule, enumerated embedding tests |
| Deepseek B6 (silent misconfig, dual dimension source) | Decision §B6/B7: explicit startup error for `native` without feature/artifact, fail-fast DIM equality, offline→fallback+degraded declared, `ALLOW_DOWNLOAD` in `.env.example` |
| Deepseek B7 (ledger/spec/process) | Durable path header + PR-0: spec + REQ IDs, three `planned` ledger entries, crate `AGENTS.md`, `docs/adr/README.md` entry, verify-pipeline gate |
| Deepseek B8 (simulation claims result without a run) | Simulation §: Result/Agreement/Sensitivity marked pending until P0 |
| Codex B1/B2 (cross-model fallback corrupts; no-new-table vs dual-read) | Cross-model isolation § + §B3: generation-shadow storage, per-space rank fusion, fallback vectors never enter native generation, rewritten P5 |
| Codex B3 (tokenizer/task-role API absent) | Preprocessing §: `tokenizers` crate + pinned `tokenizer.json`, per-model-card prompts, typed roles, 8192-token rule, truncation/overflow policy |
| Codex B4 (pooling/export contract missing; cosine 1 ≠ bit identity) | Decision §B2 + Preprocessing §: export manifest, `[batch,768]` output, mean/include-prompt + L2 rule, numerical gate (cosine ≥ 0.9999, elem/norm err ≤ 1e-5) through Rust |
| Codex B5 (no admission policy; 1 GB undefined) | Concurrency §: `spawn_blocking` + bounded semaphore held to completion, session/thread/batch/token budgets, backfill in same scheduler, GB/p50-p95 definition, bench-job measurement |
| Codex B6 (cache keyed by bare name) | Cache §: space-fingerprint + role/task/title keying, per-space caching, load-time validation |
| Codex B7 (DIM routing silent) | Config §: v1 rejects non-768, metadata-derived dimension, Matryoshka leading-slice + L2 rule for phase 2 |
| Codex B8 (gate commands prove nothing) | Acceptance: exact per-target commands, pinned manifest, zero-test failure, provisioned-vs-offline split |
| Generic wording + English | Full translation; "reference x86-64 desktop, 12 threads"; no machine/CPU names |
| R2-1 (Deepseek: `version = "2"` unresolvable, no stable 2.x) | §B1: exact pin `=2.0.0-rc.12`; pkg-config reworded as alternative feature set, not a step under `load-dynamic` |
| R2-2 (Deepseek: member-vs-exclude contradiction) | §B4 resolves as workspace MEMBER (explicit; `cargo metadata` lists in-tree path deps anyway); all `-p` commands stand; isolation via default-OFF feature + `load-dynamic`; CI uses `--package` |
| CB2 residual (cutover races, revision binding, crash recovery) | §B3: commit-time generation targeting, source-revision-gated commits, reconciled-rollback eligibility, crash-intact old generation; PR-3 named tests |
| Codex R1 (space/task identity stops at backend) | Isolation § + PR-2/PR-3: typed `embed_typed` through port/fallback/callers/storage boundary; PR-3 test `fallback_vector_never_enters_wrong_generation` |
| Codex R3 (preprocessing/graph placeholders) | Preprocessing §: `title: none`, right-truncate + observable counts, int64/f32 ABI, graph-baked normalized output, eps 1e-12 → `InvalidOutput`, `trim()` canonical, ordered all-or-nothing batches; PR-2 test |
| Codex R4 (semaphore without capacities) | Concurrency §: v1 limits (2 in-flight, 32 waiting, 256 KB/req, batch 32, 32×8192 tokens, intra-op cores/2); `Overloaded`/`DeadlineExceeded`; p95 b1 ≤ 500 ms; RSS incl. queue/cache/backfill; PR-2 test |
| Codex R5 (cargo isolation, gate sequencing, P5 target) | §B2/PR-0: Python parity first, identical Rust gate in PR-2 before adoption; P5 exact target; gate wrapper asserts executed test names, rejects skips, missing artifact fails |
| Codex R6 (startup-error vs fallback contradiction) | §B6/B7 state table (a–d): startup-missing is a hard error, runtime-loss is fallback + degraded; download precedes the check |
| F7 (sim model missing) | PR-1 lists `periferia/swal-sim/adr_sim.py` (`embeddinggemma2-native`) before the P0 report |
