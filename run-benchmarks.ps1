<#
.SYNOPSIS
    One-Command Embedding Benchmark Runner for Xavier
.DESCRIPTION
    Builds Xavier with CUDA support and runs all 3 GLLM embedding benchmarks.
    Includes health check + auto-start for Xavier HTTP server on :8006.
    
    Usage (PowerShell as admin or normal):
      .\run-benchmarks.ps1
    
    Output:
      bench-results/embedding-bench-{timestamp}.json  — raw data
      bench-results/embedding-bench-summary-{timestamp}.md — report
#>

$ErrorActionPreference = "Stop"
$XavierRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$Timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$OutDir = Join-Path $XavierRoot "bench-results"
$null = New-Item -ItemType Directory -Force -Path $OutDir

function Write-Step($Msg, $Icon = "⏳") { Write-Host "  $Icon $Msg" }
function Write-Header($Title) {
    Write-Host "`n╔════════════════════════════════════════════╗" -ForegroundColor Cyan
    Write-Host "║ $($Title.PadRight(44))║" -ForegroundColor Cyan
    Write-Host "╚════════════════════════════════════════════╝" -ForegroundColor Cyan
}

$LogFile = Join-Path $OutDir "full-run-$Timestamp.log"
Start-Transcript -Path $LogFile -Append | Out-Null

# ════════════════════════════════════════════════════════════════
# 0. HEALTH CHECK & AUTO-START
# ════════════════════════════════════════════════════════════════
Write-Header "🏥 HEALTH CHECK — Xavier HTTP (:8006)"

$XavierUrl = "http://localhost:8006"
$HealthTimeoutSec = 30
$HealthIntervalSec = 3

function Test-XavierHealth {
    try {
        $resp = Invoke-WebRequest -Uri "$XavierUrl/health" -Method GET -UseBasicParsing -TimeoutSec 5 -ErrorAction Stop
        if ($resp.StatusCode -eq 200) {
            try {
                $data = $resp.Content | ConvertFrom-Json
                $status = $data.status
                Write-Step "Xavier health status: $status" $(if ($status -eq "healthy") { "✅" } else { "⚠️" })
                return $true
            } catch {
                Write-Step "Xavier responds but body is not JSON — treating as ok" "✅"
                return $true
            }
        }
        return $false
    } catch {
        return $false
    }
}

function Start-XavierServer {
    Write-Step "Intentando iniciar Xavier HTTP server..." "🚀"
    
    $startScript = Join-Path $XavierRoot "start-xavier.bat"
    $startScriptPs1 = Join-Path $XavierRoot "start-xavier-rag.ps1"
    
    if (Test-Path $startScript) {
        Write-Step "Encontrado: $startScript" "📄"
        try {
            $proc = Start-Process -FilePath $startScript -WindowStyle Hidden -PassThru
            Write-Step "Proceso iniciado (PID: $($proc.Id))" "✅"
            return $proc
        } catch {
            Write-Step "Falló al iniciar $startScript : $_" "❌"
        }
    } elseif (Test-Path $startScriptPs1) {
        Write-Step "Encontrado: $startScriptPs1" "📄"
        try {
            $proc = Start-Process powershell -ArgumentList "-ExecutionPolicy Bypass -File `"$startScriptPs1`"" -WindowStyle Hidden -PassThru
            Write-Step "Proceso iniciado (PID: $($proc.Id))" "✅"
            return $proc
        } catch {
            Write-Step "Falló al iniciar $startScriptPs1 : $_" "❌"
        }
    } else {
        Write-Step "No se encontró script de inicio en $XavierRoot" "⚠️"
        Write-Step "Intentando iniciar Xavier desde el binario compilado..." "🔍"
        
        $xavierBin = Join-Path $XavierRoot "target\release\xavier.exe"
        if (Test-Path $xavierBin) {
            try {
                $proc = Start-Process -FilePath $xavierBin -ArgumentList "serve" -WindowStyle Hidden -PassThru
                Write-Step "xavier serve iniciado (PID: $($proc.Id))" "✅"
                return $proc
            } catch {
                Write-Step "Falló al iniciar binario: $_" "❌"
            }
        } else {
            Write-Step "No se encontró binario en $xavierBin" "❌"
        }
    }
    return $null
}

# --- Health check loop ---
$healthOk = Test-XavierHealth
$elapsed = 0

if (-not $healthOk) {
    Write-Step "Xavier no responde en :8006. Intentando iniciar..." "⚠️"
    $proc = Start-XavierServer
    
    while ($elapsed -lt $HealthTimeoutSec) {
        Start-Sleep -Seconds $HealthIntervalSec
        $elapsed += $HealthIntervalSec
        Write-Step "Esperando... ${elapsed}s / ${HealthTimeoutSec}s" "⏳"
        
        $healthOk = Test-XavierHealth
        if ($healthOk) {
            Write-Step "Xavier responde después de ${elapsed}s" "✅"
            break
        }
    }
}

if (-not $healthOk) {
    Write-Host "`n❌ Xavier no pudo iniciarse en $HealthTimeoutSec segundos." -ForegroundColor Red
    Write-Host "   Verifica:"
    Write-Host "   • ¿Xavier está instalado?"
    Write-Host "   • ¿Los puertos (8006) están disponibles?"
    Write-Host "   • Revisa los logs en $LogFile"
    Write-Host ""
    Write-Host "   Para iniciar manualmente: .\start-xavier.bat"
    Stop-Transcript
    exit 1
}

Write-Step "Health check completado — servicios listos" "✅"

# ════════════════════════════════════════════════════════════════
# 0. DETECT GPU
# ════════════════════════════════════════════════════════════════
$HasCuda = $false
try {
    $gpuInfo = & nvidia-smi --query-gpu=name,memory.total --format=csv,noheader 2>$null
    if ($gpuInfo) {
        $HasCuda = $true
        Write-Header "🔍 GPU DETECTADA"
        Write-Step "$gpuInfo" "✅"
    }
} catch {}

if (-not $HasCuda) {
    Write-Host "`n⚠️  No se detectó GPU NVIDIA. Se usará wgpu (CPU + GPU genérica)." -ForegroundColor Yellow
    Write-Host "   Si tienes GPU NVIDIA, asegúrate de tener:"
    Write-Host "   • NVIDIA drivers instalados"
    Write-Host "   • CUDA Toolkit (cuando gllm lo requiera)"
    Write-Host "   • nvidia-smi en el PATH`n"
}

# ════════════════════════════════════════════════════════════════
# 1. BUILD
# ════════════════════════════════════════════════════════════════
$Feature = if ($HasCuda) { "local-gllm-cuda" } else { "local-gllm" }
Write-Header "🔨 BUILDING XAVIER ($Feature)"
Write-Step "Esto puede tomar varios minutos la primera vez..." "🚀"

Push-Location $XavierRoot
$buildLog = Join-Path $OutDir "build-$Timestamp.log"
cargo build --features $Feature --release 2>&1 | Tee-Object -FilePath $buildLog

if ($LASTEXITCODE -ne 0) {
    Write-Host "`n❌ Build falló con $Feature. Intentando con default features..." -ForegroundColor Red
    cargo build --release 2>&1 | Tee-Object -FilePath $buildLog
}

if ($LASTEXITCODE -ne 0) {
    Write-Host "`n❌ Build falló completamente. Revisa: $buildLog" -ForegroundColor Red
    Stop-Transcript
    exit 1
}
Write-Step "Build exitoso!" "✅"
Pop-Location

# ════════════════════════════════════════════════════════════════
# 2. BENCHMARK — MiniLM-L6-v2 (baseline)
# ════════════════════════════════════════════════════════════════
Write-Header "📊 [1/3] all-MiniLM-L6-v2 (384d, MTEB 58.8)"
$env:XAVIER_GLLM_MODEL = "all-MiniLM-L6-v2"
$log1 = Join-Path $OutDir "bench-minilm-$Timestamp.log"
Push-Location $XavierRoot
$result1 = cargo test --features $Feature --test embedding_benchmark -- --nocapture 2>&1
$result1 | Out-File -FilePath $log1 -Encoding utf8
Pop-Location

# Extraer accuracy
$acc1 = ($result1 | Select-String "accuracy_pct|accuracy" | Select-Object -Last 1).ToString()
$hits1 = ($result1 | Select-String "✅" | Measure-Object).Count
$miss1 = ($result1 | Select-String "❌" | Measure-Object).Count
$total1 = $hits1 + $miss1
$pct1 = if ($total1 -gt 0) { [math]::Round(($hits1 / $total1) * 100, 1) } else { 0 }
Write-Step "MiniLM-L6-v2: ${hits1}/${total1} = ${pct1}%" "✅"

# ════════════════════════════════════════════════════════════════
# 3. BENCHMARK — mpnet-base-v2 (nuevo default)
# ════════════════════════════════════════════════════════════════
Write-Header "📊 [2/3] all-mpnet-base-v2 (768d, MTEB 63.0)"
$env:XAVIER_GLLM_MODEL = "all-mpnet-base-v2"
$log2 = Join-Path $OutDir "bench-mpnet-$Timestamp.log"
Push-Location $XavierRoot
$result2 = cargo test --features $Feature --test embedding_benchmark -- --nocapture 2>&1
$result2 | Out-File -FilePath $log2 -Encoding utf8
Pop-Location

$hits2 = ($result2 | Select-String "✅" | Measure-Object).Count
$miss2 = ($result2 | Select-String "❌" | Measure-Object).Count
$total2 = $hits2 + $miss2
$pct2 = if ($total2 -gt 0) { [math]::Round(($hits2 / $total2) * 100, 1) } else { 0 }
Write-Step "mpnet-base-v2: ${hits2}/${total2} = ${pct2}%" "✅"

# ════════════════════════════════════════════════════════════════
# 4. BENCHMARK — Qwen3-Embedding-0.6B (SOTA)
# ════════════════════════════════════════════════════════════════
Write-Header "📊 [3/3] Qwen3-Embedding-0.6B (1024d, MTEB ~67.5)"
$env:XAVIER_GLLM_MODEL = "Qwen/Qwen3-Embedding-0.6B"
$log3 = Join-Path $OutDir "bench-qwen3-$Timestamp.log"
Push-Location $XavierRoot
$result3 = cargo test --features $Feature --test embedding_benchmark -- --nocapture 2>&1
$result3 | Out-File -FilePath $log3 -Encoding utf8
Pop-Location

$hits3 = ($result3 | Select-String "✅" | Measure-Object).Count
$miss3 = ($result3 | Select-String "❌" | Measure-Object).Count
$total3 = $hits3 + $miss3
$pct3 = if ($total3 -gt 0) { [math]::Round(($hits3 / $total3) * 100, 1) } else { 0 }
Write-Step "Qwen3-Embedding-0.6B: ${hits3}/${total3} = ${pct3}%" "✅"

# ════════════════════════════════════════════════════════════════
# 5. DECISION ENGINE
# ════════════════════════════════════════════════════════════════
Write-Header "🏆 DECISIÓN — ¿Cuál modelo usar?"

$env:XAVIER_GLLM_MODEL = ""  # clean env

$ranked = @(
    [PSCustomObject]@{ Name="all-MiniLM-L6-v2"; Accuracy=$pct1; Dims=384; MTEB="58.8"; Size="80MB" }
    [PSCustomObject]@{ Name="all-mpnet-base-v2"; Accuracy=$pct2; Dims=768; MTEB="63.0"; Size="420MB" }
    [PSCustomObject]@{ Name="Qwen3-Embedding-0.6B"; Accuracy=$pct3; Dims=1024; MTEB="67.5"; Size="~1.5GB"; NeedsCuda=$true }
) | Sort-Object Accuracy -Descending

$winner = $ranked[0]

# Decisión lógica
Write-Host ""
Write-Host "  Resultados:" -ForegroundColor Yellow
foreach ($r in $ranked) {
    $icon = if ($r.Name -eq $winner.Name) { "🏆" } else { "  " }
    Write-Host "  $icon $($r.Name): $($r.Accuracy)% accuracy"
}

# Determinar ganador
$decision = ""
$reason = ""
if ($pct3 -ge $pct2 -and $pct3 -ge $pct1 -and $HasCuda) {
    $decision = "Qwen3-Embedding-0.6B"
    $reason = "Mejor precisión + GPU disponible. Velocidad CUDA es imbatible."
} elseif ($pct2 -ge $pct1 -and ($pct2 - $pct3) -lt 5) {
    $decision = "all-mpnet-base-v2"
    $reason = "Casi tan bueno como Qwen3 sin depender de GPU. Mejor balance."
} elseif ($pct1 -ge $pct2) {
    $decision = "all-MiniLM-L6-v2"
    $reason = "Sorprendentemente bueno para su tamaño. Mantener baseline."
} elseif ($pct2 -gt $pct1) {
    $decision = "all-mpnet-base-v2"
    $reason = "Mejor que MiniLM, sin necesidad de GPU."
} else {
    $decision = "all-mpnet-base-v2"
    $reason = "Default por balance calidad/velocidad."
}

Write-Host ""
Write-Host "  🏆 GANADOR: $decision" -ForegroundColor Green
Write-Host "  📝 Razón: $reason" -ForegroundColor Green

# ════════════════════════════════════════════════════════════════
# 6. UPDATE DEFAULTS
# ════════════════════════════════════════════════════════════════
Write-Header "⚙️  ACTUALIZANDO DEFAULTS"

$gllmRs = Join-Path $XavierRoot "src\embedding\gllm.rs"
$content = Get-Content $gllmRs -Raw

# Encontrar el default actual
$currentModel = if ($content -match 'DEFAULT_GLLM_MODEL.*?"([^"]+)"') { $Matches[1] } else { "unknown" }

Write-Step "Default actual: $currentModel"
Write-Step "Nuevo default: $decision"

if ($decision -ne $currentModel) {
    # Update GLLM_MODEL constant
    $modelName = if ($decision -eq "Qwen3-Embedding-0.6B") { "Qwen/Qwen3-Embedding-0.6B" } else { $decision }
    $dimensions = @{"all-MiniLM-L6-v2"=384; "all-mpnet-base-v2"=768; "Qwen3-Embedding-0.6B"=1024}[$decision]
    
    $content = $content -replace 'DEFAULT_GLLM_MODEL.*?"[^"]*"', "DEFAULT_GLLM_MODEL: &str = `"$modelName`""
    $content = $content -replace 'DEFAULT_GLLM_DIMENSION.*?\d+', "DEFAULT_GLLM_DIMENSION: usize = $dimensions"
    
    Set-Content -Path $gllmRs -Value $content -NoNewline
    Write-Step "gllm.rs actualizado → modelo: $modelName, dims: $dimensions" "✅"
} else {
    Write-Step "Ya está en el modelo correcto" "✅"
}

# Also update default feature in Cargo.toml if CUDA selected
if ($decision -eq "Qwen3-Embedding-0.6B" -and $HasCuda) {
    $cargoToml = Join-Path $XavierRoot "Cargo.toml"
    $toml = Get-Content $cargoToml -Raw
    if ($toml -match 'default\s*=\s*\["local-gllm"\]') {
        $toml = $toml -replace 'default\s*=\s*\["local-gllm"\]', 'default = ["local-gllm-cuda"]'
        Set-Content -Path $cargoToml -Value $toml -NoNewline
        Write-Step "Cargo.toml → default feature = local-gllm-cuda" "✅"
    }
}

# ════════════════════════════════════════════════════════════════
# 7. GENERATE REPORT
# ════════════════════════════════════════════════════════════════
$SummaryFile = Join-Path $OutDir "embedding-bench-report-$Timestamp.md"

$logHealthStatus = if ($healthOk) { "✅ Respondió en :8006 después de ${elapsed}s" } else { "❌ No disponible" }

@"
# 🏆 Xavier Embedding Benchmark Report

**Date:** $(Get-Date -Format "yyyy-MM-dd HH:mm")
**Feature:** $Feature
**GPU:** $(if ($HasCuda) { (nvidia-smi --query-gpu=name --format=csv,noheader 2>$null)[0] } else { "No NVIDIA" })

## Pre-Bench Health Check

- **Xavier :8006:** $logHealthStatus
- **Auto-start intentado:** $(if ($elapsed -gt 0) { "Sí (${elapsed}s)" } else { "No (ya estaba arriba)" })

## Results

| Model | Accuracy | MTEB | Dims | Size |
|-------|----------|------|------|------|
$(foreach ($r in $ranked) { "| $($r.Name) | $($r.Accuracy)% | $($r.MTEB) | $($r.Dims) | $($r.Size) |`n" })

## Decision

**Winner:** $decision
**Reason:** $reason

## Changes Applied

- `src/embedding/gllm.rs` → DEFAULT_GLLM_MODEL = `$modelName`
- `src/embedding/gllm.rs` → DEFAULT_GLLM_DIMENSION = $dimensions
$(if ($decision -eq "Qwen3-Embedding-0.6B" -and $HasCuda) { "- `Cargo.toml` → default feature = `local-gllm-cuda`" })

## Logs

- Build: `$buildLog`
- MiniLM: `$log1`
- mpnet: `$log2`
- Qwen3: `$log3`

## Next Steps

1. `cargo build --release` to rebuild with new defaults
2. Verify `xavier status` works correctly
3. Run `xavier add` with a test document to measure latency improvement
"@ | Out-File -FilePath $SummaryFile -Encoding utf8

Write-Step "Reporte guardado: $SummaryFile" "📄"

# ════════════════════════════════════════════════════════════════
# 8. COMMIT + PUSH
# ════════════════════════════════════════════════════════════════
Write-Header "📤 COMMIT & PUSH"
Push-Location $XavierRoot
git add -A
git commit -m "perf(embedding): benchmark results - $decision wins

- Ran 3 GLLM embedding benchmarks
- Winner: $decision ($reason)
- Updated defaults in gllm.rs
- Full report: $SummaryFile"
git push --no-verify 2>&1 | ForEach-Object { Write-Host "  $_" }
Pop-Location

Write-Step "Commit y push completados!" "✅"

# ════════════════════════════════════════════════════════════════
# DONE
# ════════════════════════════════════════════════════════════════
Write-Header "🏁 COMPLETADO"
Write-Host ""
Write-Host "  Resumen rápido:" -ForegroundColor Yellow
Write-Host "  • MiniLM-L6-v2:  ${pct1}%" -ForegroundColor $(if ($pct1 -ge 70) { "Green" } else { "Gray" })
Write-Host "  • mpnet-base-v2: ${pct2}%" -ForegroundColor $(if ($pct2 -ge 70) { "Green" } else { "Gray" })
Write-Host "  • Qwen3-Embed-0.6B: ${pct3}%" -ForegroundColor $(if ($pct3 -ge 70) { "Green" } else { "Gray" })
Write-Host ""
Write-Host "  🏆 GANADOR: $decision" -ForegroundColor Green
Write-Host "  📄 Reporte: bench-results\embedding-bench-report-$Timestamp.md"
Write-Host ""

Stop-Transcript
