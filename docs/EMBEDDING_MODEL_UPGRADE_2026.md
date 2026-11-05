# 🚀 Embedding Model Upgrade - Qwen3-4B (November 2026)

> **Status:** Ready for implementation  
> **Date:** 2026-11-04  
> **Target GPU:** AMD Radeon RX 6600 (8GB VRAM)  
> **Migration:** Qwen3-0.6B → Qwen3-4B

---

## 📋 Executive Summary

Xavier está migrando del modelo de embedding por defecto de **Qwen3-Embedding-0.6B** a **Qwen3-Embedding-4B**, logrando:

- ✅ **+3.9 puntos MTEB** (70.7 → 74.6) - mejora del 5.5% en calidad
- ✅ **+5.12 puntos multilingual** (64.33 → 69.45) - mejora del 8% en español/otros idiomas
- ✅ **Contexto de 32K tokens** (mantiene la capacidad actual)
- ✅ **Matryoshka embeddings** (2560 dims, comprimible a 32-2560)
- ⚠️ **+3.5GB VRAM** (1.5GB → 5GB en INT8) - sigue cabiendo en 8GB GPU
- ⚠️ **~2x latencia** (8-12ms → 15-20ms) - imperceptible en UI

---

## 🎯 Justificación

### Estado Actual (Qwen3-0.6B)

**Pros:**
- ⚡ Ultra rápido (~8-12ms/embedding)
- 💾 Footprint mínimo (1.5GB VRAM)
- ✅ Apache 2.0 license

**Contras:**
- ⚠️ **MTEB 70.7** - competitivo pero no top-tier
- ⚠️ **MMTEB 64.33** - débil en multilingual vs competencia
- ⚠️ Solo 0.6B parámetros - capacidad de representación limitada

### Modelo Propuesto (Qwen3-4B)

**Ventajas:**
- 🏆 **MTEB 74.6** - top 5 open-source 2026
- 🌍 **MMTEB 69.45** - excelente multilingual
- 🎯 Balance óptimo para 8GB GPU
- 📦 Ollama native (`ollama pull qwen3-embedding:4b`)
- 🔧 Matryoshka (comprimir dims sin reentrenar)
- ⚡ Sigue siendo rápido (~15-20ms)

**Trade-offs:**
- 💾 +3.5GB VRAM (pero sigue siendo 5GB < 8GB disponibles)
- ⏱️ ~2x más lento (15ms vs 8ms) - imperceptible en práctica
- 📥 Descarga inicial mayor (9GB vs 1.5GB)

---

## 📊 Benchmark Comparison

| Métrica | Qwen3-0.6B | Qwen3-4B | Delta | % Mejora |
|---------|------------|----------|-------|----------|
| **MTEB-eng-v2** | 70.7 | **74.6** | +3.9 | +5.5% |
| **MMTEB Multilingual** | 64.33 | **69.45** | +5.12 | +8.0% |
| **Contexto** | 32,768 | 32,768 | 0 | — |
| **Dimensiones** | 1024 | 2560 | +1536 | +150% |
| **VRAM (INT8)** | 1.5GB | 5GB | +3.5GB | +233% |
| **Latencia GPU** | 8-12ms | 15-20ms | +8ms | +100% |
| **Parámetros** | 0.6B | 4B | +3.4B | +567% |

**Ranking Open-Source (Nov 2026):**
- Qwen3-8B: **#1** MMTEB (70.58), MTEB-eng-v2 (75.22)
- **Qwen3-4B: #2** MMTEB (69.45), MTEB-eng-v2 (74.6)
- Qwen3-0.6B: #5-7 (dependiendo del benchmark)

---

## 🔧 Implementación

### Cambios en Código

**`src/embedding/gllm.rs`:**
```rust
// ANTES:
pub const DEFAULT_GLLM_MODEL: &str = "Qwen/Qwen3-Embedding-0.6B";
pub const DEFAULT_GLLM_DIMENSION: usize = 1024;

// DESPUÉS:
pub const DEFAULT_GLLM_MODEL: &str = "Qwen/Qwen3-Embedding-4B";
pub const DEFAULT_GLLM_DIMENSION: usize = 2560;
```

**`.env.gllm`:**
```bash
# ANTES:
XAVIER_GLLM_MODEL=Qwen/Qwen3-Embedding-0.6B
XAVIER_GLLM_DIMENSION=1024

# DESPUÉS:
XAVIER_GLLM_MODEL=Qwen/Qwen3-Embedding-4B
XAVIER_GLLM_DIMENSION=2560
```

### Pasos de Migración

#### 1. Recompilar Xavier con local-gllm

```powershell
# Limpiar build anterior
cargo clean

# Compilar con local-gllm feature
cargo build --release --features local-gllm

# O usar el script de build:
.\scripts\build-gllm-amd.ps1
```

#### 2. Configurar Entorno

```powershell
# Copiar configuración optimizada para 4B
Copy-Item .env.gllm .env

# O manualmente:
$env:XAVIER_GLLM_MODEL = "Qwen/Qwen3-Embedding-4B"
$env:XAVIER_GLLM_DIMENSION = 2560
$env:WGPU_BACKEND = "vulkan"
```

#### 3. Descargar Modelo (Automático)

Al iniciar Xavier por primera vez:
```powershell
.\target\release\xavier.exe server
```

Xavier detectará que el modelo no está en cache y lo descargará automáticamente desde HuggingFace Hub (~9GB, progreso en terminal).

**Ubicación del cache:**
- Windows: `%USERPROFILE%\.cache\gllm\models\Qwen3-Embedding-4B\`
- Linux: `~/.cache/gllm/models/Qwen3-Embedding-4B/`

#### 4. Verificar Estado

```powershell
# CLI
.\target\release\xavier.exe embedder status

# HTTP
curl http://localhost:3030/v1/health | jq .gllm

# Esperado:
# {
#   "gllm": {
#     "configured": true,
#     "ready": true,
#     "model": "Qwen/Qwen3-Embedding-4B",
#     "dimension": 2560,
#     "device": "AMD Radeon RX 6600"
#   }
# }
```

#### 5. Re-indexar Memoria Existente (Opcional pero Recomendado)

Si tienes memoria pre-existente con embeddings de 0.6B:

```powershell
# Opción A: Re-indexar todo (recomendado)
.\target\release\xavier.exe reindex --all

# Opción B: Re-indexar solo sin embeddings
.\target\release\xavier.exe reindex --only-pending

# Opción C: Mantener embeddings antiguos (coexistencia)
# No hacer nada - ambos modelos pueden coexistir
```

**⚠️ Nota:** Los embeddings de diferentes modelos **no son directamente comparables**. Una búsqueda híbrida con embeddings mixtos (0.6B + 4B) degradará ligeramente la calidad. Recomendamos re-indexar.

---

## 🧪 Testing y Validación

### Test de Integración

```bash
# Test de embeddings básico
cargo test --features local-gllm embedding::gllm::tests

# Test E2E con 4B
cargo test --features local-gllm tests/embedding_local_integration.rs

# Benchmark comparativo
cargo bench --features local-gllm embedding_benchmark
```

### Test Manual

```bash
# 1. Añadir memoria de prueba
xavier memory add "gpu-test" \
  --content "AMD Radeon RX 6600 supports Vulkan for ML inference"

# 2. Buscar (debe retornar con score alto)
xavier memory search "AMD GPU Vulkan" --show-scores

# Esperado: score > 0.85
```

### Métricas de Performance

**Antes (0.6B en RX 6600):**
- Latencia: 8-12ms/embedding
- Throughput: ~80-120 embeddings/seg
- VRAM: 1.5GB

**Después (4B en RX 6600):**
- Latencia: 15-20ms/embedding
- Throughput: ~50-60 embeddings/seg
- VRAM: 5GB (INT8) / 9GB (FP16)

---

## 📈 Impacto Esperado

### En Búsquedas (Search)

- ✅ **+5-7% Recall@10** - más documentos relevantes recuperados
- ✅ **+8% Multilingual** - mejor en español, portugués, etc.
- ✅ **Menos falsos positivos** - embeddings más discriminativos

### En UI

- ⚠️ **Latencia +8ms** - imperceptible (15ms vs 8ms)
- ✅ **Resultados más relevantes** - mejor ranking
- ✅ **Sin cambios en UX** - misma API/interfaz

### En Servidor

- ⚠️ **+3.5GB VRAM** - sigue siendo 62% GPU free (5GB/8GB)
- ⚠️ **Throughput reducido** - pero sigue siendo rápido
- ✅ **Mayor capacidad de representación** - embeddings más ricos

---

## 🔄 Rollback Plan

Si surgen problemas con 4B:

### Rollback a 0.6B

```powershell
# 1. Revertir código
cd src/embedding
git checkout HEAD~1 -- gllm.rs

# 2. Recompilar
cargo build --release --features local-gllm

# 3. Configurar
$env:XAVIER_GLLM_MODEL = "Qwen/Qwen3-Embedding-0.6B"
$env:XAVIER_GLLM_DIMENSION = 1024

# 4. Reiniciar Xavier
.\target\release\xavier.exe server
```

### Mantener Ambos Modelos

Xavier puede cambiar de modelo en runtime:

```bash
# Configurar en .env o variables de entorno
XAVIER_GLLM_MODEL=Qwen/Qwen3-Embedding-0.6B  # o 4B
XAVIER_GLLM_DIMENSION=1024                    # o 2560
```

---

## 🚧 Consideraciones

### VRAM Disponible

**RX 6600 (8GB total):**
- Qwen3-4B (INT8): ~5GB
- Overhead del sistema: ~0.5GB
- Panel UI + REST API: ~0.5GB
- **Libre para LLM:** ~2GB

**⚠️ Implicación:** Si también ejecutas un LLM local grande, podrías necesitar:
- Usar Qwen3-0.6B (embedding) + LLM pequeño
- O usar Qwen3-4B (embedding) + offload LLM a CPU
- O usar quantización más agresiva (Q4 en vez de INT8)

### Descarga Inicial

Primera vez que ejecutes Xavier con 4B:
- Descarga: ~9GB desde HuggingFace Hub
- Tiempo: 5-15 min (depende de conexión)
- Requiere: conexión a internet activa
- Cache: `~/.cache/gllm/models/`

**Tip:** Pre-descargar manualmente:
```bash
# Usando huggingface-cli
huggingface-cli download Qwen/Qwen3-Embedding-4B \
  --local-dir ~/.cache/gllm/models/Qwen3-Embedding-4B
```

### Compatibilidad con Ollama

Si prefieres usar Ollama en vez de GLLM integrado:

```bash
# Pull desde Ollama
ollama pull qwen3-embedding:4b

# Configurar Xavier para usar Ollama
export XAVIER_EMBEDDING_PROVIDER_MODE=local
export XAVIER_EMBEDDING_URL=http://localhost:11434/v1/embeddings
export XAVIER_EMBEDDING_MODEL=qwen3-embedding:4b
```

---

## 📚 Referencias

- **Qwen3-Embedding-4B Card:** https://huggingface.co/Qwen/Qwen3-Embedding-4B
- **MTEB Leaderboard:** https://huggingface.co/spaces/mteb/leaderboard
- **Xavier GLLM Docs:** `docs/guides/embedding-setup.md`
- **Nomic Embed v2 Paper:** arXiv:2502.07972v3
- **Local AI Benchmarks (2026):** https://d-central.tech/local-embedding-models/

---

## ✅ Checklist de Migración

- [ ] Leer este documento completo
- [ ] Verificar GPU disponible: `Get-WmiObject Win32_VideoController`
- [ ] Verificar Vulkan instalado: `vulkaninfo --summary`
- [ ] Backup de `.env` actual
- [ ] Actualizar `src/embedding/gllm.rs` (ya hecho en commit)
- [ ] Actualizar `.env.gllm` (ya hecho en commit)
- [ ] Recompilar: `cargo build --release --features local-gllm`
- [ ] Copiar config: `Copy-Item .env.gllm .env`
- [ ] Iniciar Xavier: `.\target\release\xavier.exe server`
- [ ] Verificar descarga del modelo (observar logs)
- [ ] Test: `xavier memory search "test"`
- [ ] Re-indexar memoria existente (opcional): `xavier reindex --all`
- [ ] Verificar `/v1/health` endpoint
- [ ] Test en panel UI
- [ ] Benchmark de performance (opcional)

---

**Fecha:** 2026-11-04  
**Autor:** Xavier Team  
**Revisión:** Pre-release  
**Estado:** ✅ Ready for implementation
