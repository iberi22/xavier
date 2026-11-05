# 🚀 Xavier GLLM Quick Start Guide

> **Modelo:** Qwen3-Embedding-4B (MTEB 74.6, 2560d)  
> **GPU:** AMD Radeon RX 6600 (8GB VRAM)  
> **Status:** Compilación en progreso...

---

## ⚡ Quick Start (3 pasos)

### 1. Esperar Compilación

```powershell
# Monitorear progreso
Get-Content build-gllm.log -Tail 10 -Wait

# O verificar proceso
Get-Process | Where-Object { $_.ProcessName -like "*cargo*" }
```

### 2. Verificar Build

```powershell
# Ejecutar script de verificación
.\scripts\verify-gllm-build.ps1
```

### 3. Iniciar Xavier

```powershell
# Copiar configuración optimizada
Copy-Item .env.gllm .env

# Iniciar servidor
.\target\release\xavier.exe server

# En otro terminal, verificar salud
curl http://localhost:3030/v1/health | ConvertFrom-Json
```

---

## 📋 Verificación Manual

### Binario Compilado

```powershell
# Verificar tamaño (esperado >100MB con GLLM)
(Get-Item target/release/xavier.exe).Length / 1MB

# Verificar versión
.\target\release\xavier.exe --version
```

### GPU AMD Detectada

```powershell
# Ver GPU
Get-WmiObject Win32_VideoController | 
  Where-Object { $_.Name -like "*AMD*" } | 
  Select-Object Name, @{N="VRAM_GB";E={[math]::Round($_.AdapterRAM / 1GB, 2)}}
```

### Vulkan Disponible

```powershell
# Verificar Vulkan (necesario para wgpu)
vulkaninfo --summary | Select-String "AMD"
```

---

## 🎯 Primera Ejecución

Al iniciar Xavier por primera vez con GLLM:

1. **Descarga automática del modelo** (~9GB)
   - Ubicación: `%USERPROFILE%\.cache\gllm\models\Qwen3-Embedding-4B\`
   - Tiempo: 5-15 min (depende de conexión)
   - Progreso: se muestra en terminal

2. **Warmup del modelo** (~10-20 segundos)
   - Carga el modelo en VRAM
   - Primera inferencia sintética
   - Logs: `embedder warmup complete, elapsed_ms=...`

3. **Servidor listo**
   - Panel UI: http://localhost:3030
   - Health check: http://localhost:3030/v1/health
   - MCP server activo

---

## 🧪 Test Básico

```powershell
# Test 1: Añadir memoria
.\target\release\xavier.exe memory add "gpu-test" `
  --content "AMD Radeon RX 6600 supports Vulkan for ML inference with Qwen3-4B embeddings"

# Test 2: Buscar (debe retornar con score alto)
.\target\release\xavier.exe memory search "AMD GPU Vulkan embeddings" --show-scores

# Esperado: score > 0.85
```

---

## 🔧 Configuración Avanzada

### Cambiar Modelo

```powershell
# En .env:
XAVIER_GLLM_MODEL=Qwen/Qwen3-Embedding-4B    # Por defecto (MTEB 74.6)
# XAVIER_GLLM_MODEL=Qwen/Qwen3-Embedding-8B  # Máxima calidad (MTEB 75.22, 9GB VRAM)
# XAVIER_GLLM_MODEL=Qwen/Qwen3-Embedding-0.6B # Ultra rápido (MTEB 70.7, 1.5GB VRAM)

XAVIER_GLLM_DIMENSION=2560  # Ajustar según modelo (4096 para 8B, 1024 para 0.6B)
```

### Optimizar Performance

```powershell
# GPU preference
$env:WGPU_BACKEND = "vulkan"              # Vulkan (recomendado para AMD)
$env:WGPU_POWER_PREF = "high-performance" # Max performance

# Warmup
$env:XAVIER_EMBED_WARMUP = "true"         # Warmup al inicio
$env:XAVIER_EMBED_WARMUP_INTERVAL_SECS = "1200"  # Re-warm cada 20 min

# Cache
$env:XAVIER_EMBEDDING_CACHE_ENABLED = "true"
$env:XAVIER_EMBEDDING_CACHE_MAX_CAPACITY = "10000"
```

### Logs de Debugging

```powershell
$env:RUST_LOG = "xavier=debug,gllm=info,wgpu=warn"
$env:XAVIER_LOG_LEVEL = "debug"

.\target\release\xavier.exe server
```

---

## 📊 Performance Esperado

### Qwen3-Embedding-4B en AMD RX 6600

| Métrica | Valor |
|---------|-------|
| **VRAM usado** | ~5GB (INT8) / ~9GB (FP16) |
| **Latencia** | 15-20ms/embedding (GPU) |
| **Throughput** | 50-60 embeddings/seg |
| **Primera inferencia** | ~200-500ms (cold start) |
| **Warmup completo** | ~10-20 segundos |

### Comparativa vs 0.6B (anterior)

| Aspecto | 0.6B | 4B | Mejora |
|---------|------|----|----|
| **MTEB Score** | 70.7 | 74.6 | +5.5% |
| **MMTEB (Multilingual)** | 64.33 | 69.45 | +8.0% |
| **Latencia** | 8-12ms | 15-20ms | 2x más lento |
| **VRAM** | 1.5GB | 5GB | 3.3x más |
| **Calidad de búsqueda** | Buena | Excelente | +5-7% recall@10 |

---

## 🐛 Troubleshooting

### Problema: "Model not found" al iniciar

**Solución:**
```powershell
# Descargar manualmente desde HuggingFace
# (requiere huggingface-cli: pip install huggingface_hub)
huggingface-cli download Qwen/Qwen3-Embedding-4B `
  --local-dir $env:USERPROFILE\.cache\gllm\models\Qwen3-Embedding-4B
```

### Problema: "Out of VRAM" o crashes

**Solución:**
```powershell
# Cambiar a modelo más pequeño
$env:XAVIER_GLLM_MODEL = "Qwen/Qwen3-Embedding-0.6B"
$env:XAVIER_GLLM_DIMENSION = "1024"

# O cerrar otros procesos que usen GPU
Get-Process | Where-Object { $_.GPU -gt 0 } | Stop-Process
```

### Problema: Compilación falló

**Solución:**
```powershell
# Limpiar y recompilar
cargo clean
cargo build --release --features local-gllm

# Si falla de nuevo, revisar:
# - Rust version: rustc --version (requiere 1.70+)
# - Disk space: al menos 30GB libres
# - RAM: al menos 8GB disponibles durante compilación
```

### Problema: Vulkan no detectado

**Solución:**
```powershell
# Instalar/actualizar drivers AMD Adrenalin
# Download: https://www.amd.com/en/support

# Verificar instalación
vulkaninfo --summary

# Si falla, wgpu usará fallback (DirectX o CPU)
```

---

## 📚 Documentación Adicional

- **Guía completa de migración:** `docs/EMBEDDING_MODEL_UPGRADE_2026.md`
- **Análisis comparativo:** Ver artifact en chat
- **Embedding setup guide:** `docs/guides/embedding-setup.md`
- **Troubleshooting GLLM+GPU:** `docs/troubleshooting/gllm-gpu.md`

---

## 🔗 Referencias

- **Qwen3-Embedding-4B:** https://huggingface.co/Qwen/Qwen3-Embedding-4B
- **MTEB Leaderboard:** https://huggingface.co/spaces/mteb/leaderboard
- **wgpu + AMD Guide:** https://github.com/gfx-rs/wgpu/wiki/AMD-GPUs

---

**Última actualización:** 2026-11-04  
**Status de compilación:** En progreso (~10-15 min)  
**Próximo paso:** Ejecutar `.\scripts\verify-gllm-build.ps1` cuando termine
