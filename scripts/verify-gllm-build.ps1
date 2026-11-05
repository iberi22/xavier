#!/usr/bin/env pwsh
# ══════════════════════════════════════════════════════════════════════════
# Xavier GLLM Build Verification Script
# ══════════════════════════════════════════════════════════════════════════
# Purpose: Verify that Xavier was compiled correctly with local-gllm feature
# ══════════════════════════════════════════════════════════════════════════

$ErrorActionPreference = "Stop"

Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  Xavier GLLM Build Verification" -ForegroundColor Cyan
Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""

$allPassed = $true

# ── Test 1: Binary exists ─────────────────────────────────────────────
Write-Host "[1/6] Verificando binario compilado..." -ForegroundColor Yellow
if (Test-Path "target/release/xavier.exe") {
    $size = (Get-Item "target/release/xavier.exe").Length / 1MB
    Write-Host "  ✓ Binario existe: $([math]::Round($size, 2)) MB" -ForegroundColor Green
    
    if ($size -lt 50) {
        Write-Host "  ⚠ Tamaño sospechosamente pequeño (esperado >100MB con GLLM)" -ForegroundColor Yellow
        $allPassed = $false
    }
} else {
    Write-Host "  ✗ Binario NO encontrado en target/release/xavier.exe" -ForegroundColor Red
    $allPassed = $false
    exit 1
}

# ── Test 2: Version output ────────────────────────────────────────────
Write-Host ""
Write-Host "[2/6] Verificando versión..." -ForegroundColor Yellow
try {
    $version = & "target/release/xavier.exe" --version 2>&1
    Write-Host "  ✓ Versión: $version" -ForegroundColor Green
} catch {
    Write-Host "  ✗ No se pudo ejecutar el binario" -ForegroundColor Red
    $allPassed = $false
}

# ── Test 3: Check for GLLM in binary ──────────────────────────────────
Write-Host ""
Write-Host "[3/6] Verificando símbolos GLLM en binario..." -ForegroundColor Yellow
$binaryContent = Get-Content "target/release/xavier.exe" -Raw -Encoding Byte
$gllmFound = $binaryContent -match [byte[]][char[]]"gllm"

if ($gllmFound) {
    Write-Host "  ✓ Símbolos GLLM encontrados en binario" -ForegroundColor Green
} else {
    Write-Host "  ⚠ Símbolos GLLM NO encontrados (puede ser normal si están optimizados)" -ForegroundColor Yellow
}

# ── Test 4: Environment configuration ─────────────────────────────────
Write-Host ""
Write-Host "[4/6] Verificando configuración .env..." -ForegroundColor Yellow
if (Test-Path ".env.gllm") {
    Write-Host "  ✓ .env.gllm existe" -ForegroundColor Green
    
    if (-not (Test-Path ".env")) {
        Write-Host "  ⚠ .env NO existe - necesitas copiar .env.gllm a .env" -ForegroundColor Yellow
        Write-Host "    Ejecuta: Copy-Item .env.gllm .env" -ForegroundColor DarkGray
    } else {
        $envContent = Get-Content ".env" -Raw
        if ($envContent -match "Qwen3-Embedding-4B") {
            Write-Host "  ✓ .env configurado para Qwen3-4B" -ForegroundColor Green
        } elseif ($envContent -match "Qwen3-Embedding-0.6B") {
            Write-Host "  ℹ .env configurado para Qwen3-0.6B (modelo anterior)" -ForegroundColor Cyan
            Write-Host "    Para usar 4B: Copy-Item .env.gllm .env" -ForegroundColor DarkGray
        } else {
            Write-Host "  ⚠ .env no tiene configuración GLLM" -ForegroundColor Yellow
        }
    }
} else {
    Write-Host "  ✗ .env.gllm NO encontrado" -ForegroundColor Red
    $allPassed = $false
}

# ── Test 5: Dependency check ──────────────────────────────────────────
Write-Host ""
Write-Host "[5/6] Verificando dependencias de wgpu..." -ForegroundColor Yellow
try {
    $vulkanInfo = vulkaninfo --summary 2>&1 | Select-String "AMD" | Select-Object -First 1
    if ($vulkanInfo) {
        Write-Host "  ✓ Vulkan disponible para AMD: $vulkanInfo" -ForegroundColor Green
    } else {
        Write-Host "  ⚠ Vulkan no detectado - wgpu usará fallback" -ForegroundColor Yellow
    }
} catch {
    Write-Host "  ⚠ vulkaninfo no encontrado - instalar Vulkan SDK" -ForegroundColor Yellow
}

# ── Test 6: Build log check ───────────────────────────────────────────
Write-Host ""
Write-Host "[6/6] Verificando log de compilación..." -ForegroundColor Yellow
if (Test-Path "build-gllm.log") {
    $logContent = Get-Content "build-gllm.log" -Raw
    
    if ($logContent -match "error") {
        Write-Host "  ⚠ Log contiene errores - revisar build-gllm.log" -ForegroundColor Yellow
        $allPassed = $false
    } elseif ($logContent -match "warning") {
        Write-Host "  ℹ Log contiene warnings (puede ser normal)" -ForegroundColor Cyan
    }
    
    if ($logContent -match "Finished.*release.*target") {
        Write-Host "  ✓ Compilación completada exitosamente" -ForegroundColor Green
    } else {
        Write-Host "  ⚠ Log no muestra completado - compilación en progreso?" -ForegroundColor Yellow
    }
} else {
    Write-Host "  ℹ build-gllm.log no encontrado" -ForegroundColor Cyan
}

# ── Summary ───────────────────────────────────────────────────────────
Write-Host ""
Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Cyan

if ($allPassed) {
    Write-Host "  ✓ Verificación PASADA" -ForegroundColor Green
    Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Siguientes pasos:" -ForegroundColor Yellow
    Write-Host "  1. Copiar configuración: Copy-Item .env.gllm .env" -ForegroundColor White
    Write-Host "  2. Iniciar Xavier: .\target\release\xavier.exe server" -ForegroundColor White
    Write-Host "  3. El modelo Qwen3-4B se descargará automáticamente (~9GB)" -ForegroundColor White
    Write-Host ""
    Write-Host "Verificar estado:" -ForegroundColor Yellow
    Write-Host "  curl http://localhost:3030/v1/health | ConvertFrom-Json" -ForegroundColor White
} else {
    Write-Host "  ⚠ Verificación FALLÓ - revisar warnings arriba" -ForegroundColor Yellow
    Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Troubleshooting:" -ForegroundColor Yellow
    Write-Host "  - Revisar: build-gllm.log" -ForegroundColor White
    Write-Host "  - Recompilar: cargo clean; cargo build --release --features local-gllm" -ForegroundColor White
    Write-Host "  - Verificar GPU: Get-WmiObject Win32_VideoController" -ForegroundColor White
}

Write-Host ""
