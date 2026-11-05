#!/usr/bin/env pwsh
# ══════════════════════════════════════════════════════════════════════════
# Xavier GLLM Build Script - AMD GPU Optimized
# ══════════════════════════════════════════════════════════════════════════
# Purpose: Compile Xavier with local-gllm feature for AMD Radeon GPUs
# GPU: AMD Radeon RX 6600 (wgpu + Vulkan backend)
# ══════════════════════════════════════════════════════════════════════════

$ErrorActionPreference = "Stop"

Write-Host "══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  Xavier GLLM Build - AMD Radeon RX 6600" -ForegroundColor Cyan
Write-Host "  Model: Qwen3-Embedding-4B (MTEB 74.6, 2560d)" -ForegroundColor Cyan
Write-Host "══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""

# ── Step 1: Verify GPU ────────────────────────────────────────────────
Write-Host "[1/6] Verificando GPU AMD..." -ForegroundColor Yellow
$gpu = Get-WmiObject Win32_VideoController | Where-Object { $_.Name -like "*AMD*" -or $_.Name -like "*Radeon*" }
if ($gpu) {
    Write-Host "  ✓ GPU detectada: $($gpu.Name)" -ForegroundColor Green
    Write-Host "  ✓ VRAM: $([math]::Round($gpu.AdapterRAM / 1GB, 2)) GB" -ForegroundColor Green
} else {
    Write-Host "  ✗ No se detectó GPU AMD" -ForegroundColor Red
    exit 1
}

# ── Step 2: Verify Vulkan ─────────────────────────────────────────────
Write-Host ""
Write-Host "[2/6] Verificando Vulkan..." -ForegroundColor Yellow
$vulkanCheck = vulkaninfo --summary 2>&1 | Select-String "AMD"
if ($vulkanCheck) {
    Write-Host "  ✓ Vulkan disponible para AMD" -ForegroundColor Green
} else {
    Write-Host "  ⚠ Vulkan no detectado - wgpu podría usar fallback" -ForegroundColor DarkYellow
}

# ── Step 3: Clean Previous Build ──────────────────────────────────────
Write-Host ""
Write-Host "[3/6] Limpiando build anterior..." -ForegroundColor Yellow
if (Test-Path "target/release/xavier.exe") {
    Remove-Item "target/release/xavier.exe" -Force
    Write-Host "  ✓ Build anterior eliminado" -ForegroundColor Green
} else {
    Write-Host "  ✓ No hay build previo" -ForegroundColor Green
}

# ── Step 4: Compile with local-gllm ───────────────────────────────────
Write-Host ""
Write-Host "[4/6] Compilando Xavier con local-gllm..." -ForegroundColor Yellow
Write-Host "  Esto puede tardar 10-15 minutos en la primera compilación..." -ForegroundColor DarkGray
Write-Host ""

$buildStart = Get-Date

# Set Vulkan preference for wgpu
$env:WGPU_BACKEND = "vulkan"

# Build command
cargo build --release --features local-gllm

if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "  ✗ Compilación falló" -ForegroundColor Red
    exit 1
}

$buildEnd = Get-Date
$buildDuration = ($buildEnd - $buildStart).TotalMinutes

Write-Host ""
Write-Host "  ✓ Compilación exitosa en $([math]::Round($buildDuration, 2)) minutos" -ForegroundColor Green

# ── Step 5: Verify Binary ─────────────────────────────────────────────
Write-Host ""
Write-Host "[5/6] Verificando binario..." -ForegroundColor Yellow
$binarySize = (Get-Item "target/release/xavier.exe").Length / 1MB
Write-Host "  ✓ Tamaño del binario: $([math]::Round($binarySize, 2)) MB" -ForegroundColor Green

# Check if GLLM is available
$versionOutput = & "target/release/xavier.exe" --version 2>&1
Write-Host "  ✓ Versión: $versionOutput" -ForegroundColor Green

# ── Step 6: Setup Configuration ───────────────────────────────────────
Write-Host ""
Write-Host "[6/6] Configurando entorno..." -ForegroundColor Yellow
if (Test-Path ".env.gllm") {
    Write-Host "  ✓ Configuración .env.gllm lista" -ForegroundColor Green
    Write-Host "    Para usar: Copy-Item .env.gllm .env" -ForegroundColor DarkGray
} else {
    Write-Host "  ⚠ .env.gllm no encontrado" -ForegroundColor DarkYellow
}

# ── Summary ───────────────────────────────────────────────────────────
Write-Host ""
Write-Host "══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  ✓ Xavier compilado con GLLM + AMD GPU" -ForegroundColor Green
Write-Host "══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""
Write-Host "Siguientes pasos:" -ForegroundColor Yellow
Write-Host "  1. Copiar configuración: Copy-Item .env.gllm .env" -ForegroundColor White
Write-Host "  2. Iniciar Xavier: .\target\release\xavier.exe server" -ForegroundColor White
Write-Host "  3. El modelo Qwen3-4B se descargará automáticamente (~9GB)" -ForegroundColor White
Write-Host "     - MTEB Score: 74.6 (English v2)" -ForegroundColor DarkGray
Write-Host "     - MMTEB Score: 69.45 (Multilingual)" -ForegroundColor DarkGray
Write-Host "     - VRAM Usage: ~5GB (INT8) / ~9GB (FP16)" -ForegroundColor DarkGray
Write-Host ""
Write-Host "Verificar estado:" -ForegroundColor Yellow
Write-Host "  .\target\release\xavier.exe embedder status" -ForegroundColor White
Write-Host ""
