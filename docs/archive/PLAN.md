# PLAN: Xavier Dogfooding Fixes & Improvements

> Based on SWAL dogfooding session (2026-06-11) — bugs, quality flaws, and improvements identified during real-world testing of xavier 0.6.1-beta.

## ✅ Status

| # | Issue | Type | Status | PR |
|---|-------|------|--------|-----|
| 1 | Hardcoded version `0.4.1` in `stats_handler` | Bug | ✅ Fixed | This session |
| 2 | Hardcoded version `0.4.1` in Telegram `/health` | Bug | ✅ Fixed | This session |
| 3 | `Export` missing `--limit` flag | Feature | ✅ Fixed | This session |
| 4 | `Search` missing `--limit` flag (only positional `[LIMIT]`) | UX | ✅ Fixed | This session |
| 5 | `recall` offline shows `score: 0.000` or `score: 1.0` always | Quality | 🟡 Needs BM25 fallback scoring | Future |
| 6 | `search` offline returns results but score not computed | Quality | 🟡 Needs BM25 fallback scoring | Future |
| 7 | `stats` shows no memory count/detail (just version + workspace) | Feature | 🟡 Needs more data | Future |
| 8 | Compilation has 29 warnings (unused imports, unused variables) | Code Quality | 🟡 Clean up | Future |
| 9 | Embedding-based `add` is slow (~1.2s) due to API call on every add | Performance | 🟡 Embedding cache needed | Future |
| 10 | `setup` cmd to handle GLP-1 / platform setup | Feature | 🟡 Sugar feature | Future |

## Bug Details

### B1: Hardcoded version string `"0.4.1"`
**Files:** `src/cli/handlers/memory.rs:507`, `src/telegram/mod.rs:91`

**Root cause:** Human error — version string not updated when bumping `Cargo.toml` to 0.6.1-beta.

**Fix:** Replace `"0.4.1"` with `env!("CARGO_PKG_VERSION")` which reads from `Cargo.toml` at compile time.

### B2: Export command missing `--limit`
**File:** `src/cli/commands/enums.rs:154`, `src/cli/commands/mod.rs:147`

**Root cause:** The `Export` struct only had `--public` and `--output`. Users cannot limit how many memories to export.

**Fix:** Added `#[arg(short, long)] limit: Option<usize>` field, default 1000, clamped 1-10000.

### B3: Search positional `[LIMIT]` is unintuitive
**File:** `src/cli/commands/enums.rs:30`, `src/cli/commands/mod.rs:52`

**Root cause:** `Search` uses `limit: Option<usize>` as a positional argument (`[LIMIT]`). Users expect `--limit`.
The help text shows `xavier search <QUERY> [LIMIT]` but `--limit` is not recognized.

**Fix:** Added `#[arg(short, long)] limit_flag: Option<usize>` alongside positional limit. Handler prefers `limit_flag` over positional.

### B4: Offline search scores are always 1.0
**File:** `src/cli/handlers/memory.rs:640`

**Root cause:** The offline fallback in `search_memories_filtered` uses `doc.metadata.get("score").unwrap_or(1.0)`. When searching offline via `search_with_cache_filtered`, the backend doesn't compute BM25 scores — it just returns documents with metadata intact. Since no score was written at `add` time, it defaults to 1.0.

**Workaround:** Document all results with score 1.0 as "relevant" but indistinguishable.
**Proper fix needed:** Add BM25 or TF-IDF scoring in `search_with_cache_filtered` for offline mode.

### B5: Stats view is sparse
**Current output:**
```json
{
  "status": "ok",
  "version": "0.6.1-beta",
  "workspace_id": "default"
}
```

**Problem:** No memory count, no disk usage, no cluster info. Users get no useful information.

**Future improvement:** Add `document_count`, `total_size_bytes`, `cluster_counts`.

## Performance Findings

| Operation | Latency | Bottleneck |
|-----------|---------|------------|
| `add` via HTTP API | ~1,238ms | OpenRouter embedding API call |
| `add` offline (SQLite-Vec) | ~50ms | Local only, fast |
| `search` via HTTP API | ~733ms | OpenRouter embedding API call |
| `search` offline (cache) | ~100ms | No embedding needed |
| `stats` | ~34ms | Simple HTTP or local query |
| `version` | ~34ms | Simple HTTP |
| CLI startup | ~26ms | Binary load + cache init |

**Key insight:** The bottleneck for online operations is the embedding API call (OpenRouter). Adding an embedding cache would reduce `add` from ~1.2s to ~50ms for repeated content.

## Implementation Order

1. ✅ **P0 — Bugs that cause wrong behavior** → Fixed (version strings)
2. ✅ **P1 — Missing features that harm UX** → Fixed (`--limit` flags)
3. 🟡 **P2 — Quality issues** → Needs more work (BM25 scoring, stats expansion)
4. 🟡 **P3 — Performance** → Embedding cache (needs design)
5. 🟡 **P4 — Code cleanup** → 29 warnings (unused imports, dead code)

## Embedding Model Benchmarking Plan

### Problem
`xavier add` takes ~1,200ms via OpenRouter embedding API. Local embeddings remove network latency entirely but must match or exceed retrieval precision.

### Goal
Benchmark 5 embedding backends on retrieval precision (recall@3) + latency to select the optimal default.

### Contenders

| # | Model | Backend | Dims | MTEB | Size | VRAM |
|---|-------|---------|------|------|------|------|
| 1 | **all-MiniLM-L6-v2** | GLLM (wgpu/cpu) | 384 | 58.8 | 80 MB | 0 MB |
| 2 | **all-mpnet-base-v2** ⭐ | GLLM (wgpu/cpu) | 768 | **63.0** | 420 MB | 0 MB |
| 3 | **Qwen3-Embedding-0.6B** | GLLM (cuda) | 1024 | **~67.5** | ~1.5 GB | ~1.5 GB |
| 4 | **gte-Qwen2-1.5B-instruct** | Docker (Infinity/TEI) | 1536 | ~64.5 | ~3 GB | ~3 GB |
| 5 | **text-embedding-3-small** | OpenRouter (API) | 1536 | 62.3 | N/A | N/A |

### Files Created

| File | Purpose |
|------|---------|
| `benches/embedding_benchmark.rs` | Rust benchmark (10 questions, recall@3, latency) |
| `scripts/run-embedding-benchmark.ps1` | PowerShell orchestrator (builds, runs, reports) |
| `docker/docker-compose.embeddings.yml` | Docker services for Infinity/TEI/Ollama |

### How to Run

**Quick test (one model):**
```bash
cargo test --test embedding_benchmark -- --nocapture
```

**Full benchmark suite:**
```powershell
.\scripts\run-embedding-benchmark.ps1 -Cuda
```

**Docker models (pre-requisite):**
```bash
docker compose -f docker/docker-compose.embeddings.yml up -d infinity
```

### What We Measure

- **Retrieval precision:** recall@3 across 10 multi-domain queries (contracts, incidents, releases, performance, infrastructure)
- **Latency:** average/min/max encode time per query (ms)
- **Hit rate:** correct top-3 retrievals / total queries

### GPU Usage (8GB VRAM)

- `gllm` CUDA feature — Qwen3-Embedding-0.6B fits in ~1.5 GB (easily within 8 GB)
- Docker Infinity with gte-Qwen2-1.5B — ~3 GB (well within 8 GB)
- Both can run simultaneously if needed

### Actual Results (CPU wgpu, EditorOne, 2026-06-11)

| Model | Accuracy | Latency (CPU) | Dims | MTEB | Notes |
|-------|----------|---------------|------|------|-------|
| **MiniLM-L6-v2** | 60% ✅ | **1,928ms** | 384 | 58.8 | All HIGH correct. LOW fails due to shared terms in old pairs. |
| **mpnet-base-v2** | 60% ✅ | **6,410ms** | 768 | 63.0 | Same, HIGH all correct. |
| **Qwen3-Embed-0.6B** | 60% ✅ | **17,368ms** | 1024 | 67.5 | Same pattern. CPU too slow for practical use. |
| Docker Infinity | 40% ❌ | 4,690ms | 1536 | 64.5 | Failed — no Docker server running during test. |
| OpenRouter API | 40% ❌ | 357ms | 1536 | 62.3 | Failed — no API key configured. |

**Key finding:** Modern dense embeddings (all models) produce cosine similarities in 0.97-1.0 range. Old thresholds (0.4/0.5) are invalid.

### Decision (Applied 2026-06-11)

**Default: `Qwen/Qwen3-Embedding-0.6B` (1024d, MTEB 67.5)**
- **GPU available (AMD RX 6600 8GB):** wgpu via Vulkan → estimated latency ~2-5ms
- **CPU fallback:** all-MiniLM-L6-v2 (384d, 1.9s — usable but slow)
- **Docker Infinity:** Qwen3-Embedding-0.6B via Infinity on port 7997
- **Feature usado:** `local-gllm` (wgpu) — NO CUDA porque AMD no tiene CUDA

### Applied Changes
- `src/embedding/gllm.rs` → DEFAULT_GLLM_MODEL = Qwen/Qwen3-Embedding-0.6B, dim = 1024
- `benches/embedding_benchmark.rs` → thresholds calibrados (0.95/0.94), LOW pairs rediseñados
- `docker/docker-compose.embeddings.yml` → Infinity con Qwen3-Embedding-0.6B
- `pplx-embed` eliminado de Docker, reemplazado por Infinity en puerto 7997

### Env Var Configuration

```bash
# Model selection (GLLM)
XAVIER_GLLM_MODEL=Qwen/Qwen3-Embedding-0.6B    # SOTA, needs CUDA
XAVIER_GLLM_MODEL=all-mpnet-base-v2             # Balanced, CPU/GPU

# Feature selection (compile-time)
cargo build --features local-gllm                # wgpu (CPU + any GPU)
cargo build --features local-gllm-cuda           # CUDA (NVIDIA only)

# Docker backend
XAVIER_EMBEDDING_LOCAL_URL=http://localhost:7997/v1/embeddings
XAVIER_EMBEDDING_MODEL=Alibaba-NLP/gte-Qwen2-1.5B-instruct
```

## How to Verify

### After PR merge:
```bash
# Test version consistency
xavier --version          # Should show 0.6.1-beta
xavier stats              # Should show 0.6.1-beta

# Test --limit in search
xavier search "test" --limit 5
xavier search "test" 5    # Old positional still works

# Test --limit in export
xavier export --limit 10 --public
```

## Future: BM25 offline scoring

To fix score=1.0 in offline mode, implement a simple BM25 scorer in:

```
src/memory/qmd/search.rs  →  search_with_cache_filtered()
src/memory/
```

A `MemoryDocument` should have a computed `bm25_score(query)` method that runs BM25 on the document's `content` field when serving results from cache.
