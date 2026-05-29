# Xavier Diagnosis & Fix - 2026-05-27

## 🔧 Fix Applied

### Problem
`burn-candle 0.20.1` incompatible con `candle-core 0.9.2` — no cubre nuevos DType: `I16`, `I32`, `F8E4M3`, `F6E2M3`, `F6E3M2`. Bloqueaba compilación con default features.

### Solution
**Removed `local-gllm` from default features** in `Cargo.toml`:
```
before: default = ["cli-interactive", "local-gllm"]
after:  default = ["cli-interactive"]
```

### Current State ✅
| Component | Status | Notes |
|-----------|--------|-------|
| `cargo build` (default) | ✅ SUCCESS | 35 warnings (cosméticos) |
| `xavier.exe` | ✅ 40.5 MB | `target/debug/xavier.exe` |
| HTTP Server | ✅ RUNNING | PID 9624, puerto 8006 |
| Health Check | ✅ `status: "ok"` | `uptime: 24s`, `version: "1.0.0"` |
| QmdMemory | ✅ 106 memories loaded | Persistent store OK |
| Code Graph DB | ✅ Initialized | `data/code_graph.db` |
| Embedding | ⚠️ NO-OP fallback | `local-gllm` no compilado. Funciona sin embeddings |
| Docker | ❌ No disponible | Docker Desktop no corre |

### Trade-off
- **Perdemos:** Embeddings locales con GLLM (all-MiniLM-L6-v2). Xavier usa no-op embedder.
- **Ganamos:** Xavier compila y arranca. Todo el resto funciona: memoria, HTTP API, code graph, security, rate limiting.

### Para recuperar embeddings locales:
Opción 1: Parchear `burn-candle` localmente (`_ => unimplemented!()`)
Opción 2: Actualizar `burn-candle` a versión compatible con candle-core 0.9.2
Opción 3: Usar embedding externo (config `embedding_provider_mode` en xavier.config.json)
