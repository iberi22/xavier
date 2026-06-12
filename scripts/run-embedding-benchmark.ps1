<#
.SYNOPSIS
    Embedding Model Benchmark Suite for Xavier
.DESCRIPTION
    Tests 5 embedding backends across 10 retrieval questions.
    Measures: retrieval precision (accuracy%) + latency (ms).

    Models tested:
      1. GLLM local: all-MiniLM-L6-v2  (384d) — baseline
      2. GLLM local: all-mpnet-base-v2  (768d) — new default
      3. GLLM local: Qwen3-Embedding-0.6B (1024d) — SOTA
      4. Docker: Infinity/TEI/Ollama
      5. OpenRouter: text-embedding-3-small (1536d)

.PARAMETER Model
    Skip to specific model: "minilm", "mpnet", "qwen3", "docker", "openrouter", or "all"
.PARAMETER Cuda
    Use CUDA features when building gllm (default: true on NVIDIA GPUs)
.PARAMETER OutputDir
    Directory for benchmark results (default: E:\scripts-python\xavier\bench-results)
.PARAMETER DockerEndpoint
    Infinity/TEI endpoint (default: http://localhost:7997/v1/embeddings)
.PARAMETER DockerModel
    Model for Docker backend (default: Alibaba-NLP/gte-Qwen2-1.5B-instruct)

.EXAMPLE
    .\scripts\run-embedding-benchmark.ps1 -Cuda
.EXAMPLE
    .\scripts\run-embedding-benchmark.ps1 -Model qwen3
.EXAMPLE
    .\scripts\run-embedding-benchmark.ps1 -Model docker -DockerEndpoint "http://10.0.0.5:8080/v1/embeddings"
#>

param(
    [ValidateSet("all", "minilm", "mpnet", "qwen3", "docker", "openrouter")]
    [string]$Model = "all",

    [switch]$Cuda = $true,

    [string]$OutputDir = "bench-results",

    [string]$DockerEndpoint = "http://localhost:7997/v1/embeddings",

    [string]$DockerModel = "Alibaba-NLP/gte-Qwen2-1.5B-instruct"
)

$ErrorActionPreference = "Stop"
$XavierRoot = "E:\scripts-python\xavier"
$Timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$ResultsFile = Join-Path $XavierRoot $OutputDir "embedding-bench-$Timestamp.json"
$SummaryFile = Join-Path $XavierRoot $OutputDir "embedding-bench-summary-$Timestamp.md"

# Ensure output directory
New-Item -ItemType Directory -Force -Path (Join-Path $XavierRoot $OutputDir) | Out-Null

function Write-Banner {
    param([string]$Title)
    Write-Host ""
    Write-Host "╔══════════════════════════════════════════════════════╗" -ForegroundColor Cyan
    Write-Host "║ $($Title.PadRight(50))║" -ForegroundColor Cyan
    Write-Host "╚══════════════════════════════════════════════════════╝" -ForegroundColor Cyan
    Write-Host ""
}

function Write-Step {
    param([string]$Message, [string]$Status = "⋯")
    $icons = @{
        "⋯" = "⏳"
        "✅" = "✅"
        "❌" = "❌"
        "⚠️" = "⚠️"
        "🚀" = "🚀"
    }
    $icon = $icons[$Status]
    if (-not $icon) { $icon = $Status }
    Write-Host "  $icon $Message"
}

function Check-Command {
    param([string]$Command)
    return (Get-Command $Command -ErrorAction SilentlyContinue) -ne $null
}

function Measure-Embedder {
    param(
        [string]$Name,
        [string]$EnvVars
    )

    Write-Host ""
    Write-Host "━━━ Testing: $Name ━━━" -ForegroundColor Yellow
    Write-Host "  Env: $EnvVars"
    Write-Host ""

    $logFile = Join-Path $XavierRoot $OutputDir "bench-$Name-$Timestamp.log"
    $envBlock = ""
    if ($EnvVars) {
        $envBlock = "$EnvVars; "
    }

    # Run the bench as a test (not requiring criterion)
    $cmd = "cd $XavierRoot; $envBlock cargo test --test embedding_benchmark -- --nocapture 2>&1"
    $result = Invoke-Expression $cmd

    # Save log
    $result | Out-File -FilePath $logFile -Encoding utf8
    Write-Host "  📄 Log saved to: $logFile"

    # Extract accuracy from output
    $accuracyLine = $result | Select-String "Best model in test:"
    if ($accuracyLine) {
        Write-Host "  $($accuracyLine.Line)" -ForegroundColor Green
    }

    $failLine = $result | Select-String "FAILED"
    if ($failLine) {
        Write-Host "  ❌ Test FAILED" -ForegroundColor Red
        return $false
    }

    # Extract individual results
    $hits = $result | Select-String "HIT" | Measure-Object | Select-Object -ExpandProperty Count
    $misses = $result | Select-String "MISS" | Measure-Object | Select-Object -ExpandProperty Count
    $total = $hits + $misses
    if ($total -gt 0) {
        $accuracy = [math]::Round(($hits / $total) * 100, 1)
        Write-Host "  📊 Accuracy: ${hits}/${total} = ${accuracy}%" -ForegroundColor Green
    }

    Write-Host "  ✅ Completed: $Name" -ForegroundColor Green
    return $true
}

# ════════════════════════════════════════════════════════════════
# PREFLIGHT CHECKS
# ════════════════════════════════════════════════════════════════

Write-Banner "🔍 PREFLIGHT CHECKS"

# Check Rust toolchain
$rustc = (rustc --version 2>$null)
if (-not $rustc) {
    Write-Step "Rust toolchain not found" "❌"
    exit 1
}
Write-Step "Rust: $rustc" "✅"

# Check CUDA
$nvidia = "NO"
$cudaSmi = Get-Command "nvidia-smi" -ErrorAction SilentlyContinue
if ($cudaSmi) {
    $gpuInfo = & nvidia-smi --query-gpu=name,memory.total --format=csv,noheader 2>$null
    if ($gpuInfo) {
        $nvidia = "YES"
        Write-Step "GPU: $gpuInfo" "✅"
    }
}
if ($nvidia -eq "NO") {
    Write-Step "No NVIDIA GPU detected — CUDA tests will be skipped" "⚠️"
    $Cuda = $false
}

# Check Docker
$docker = "NO"
$dockerCmd = Get-Command "docker" -ErrorAction SilentlyContinue
if ($dockerCmd) {
    $dockerVer = docker --version 2>$null
    if ($dockerVer) {
        $docker = "YES"
        Write-Step "Docker: $dockerVer" "✅"
    }
}
if ($docker -eq "NO") {
    Write-Step "Docker not found — Docker model tests will be skipped" "⚠️"
}

# Check OpenRouter key
$orKey = $env:OPENAI_API_KEY
if (-not $orKey) { $orKey = $env:XAVIER_OPENROUTER_API_KEY }
if ($orKey) {
    Write-Step "OpenRouter API key found" "✅"
} else {
    Write-Step "No OpenRouter API key — cloud model tests will be skipped" "⚠️"
}

Write-Host ""

# ════════════════════════════════════════════════════════════════
# BUILD XAVIER (clean features for benchmark)
# ════════════════════════════════════════════════════════════════

if ($Model -eq "all" -or $Model -in @("minilm", "mpnet", "qwen3")) {
    Write-Banner "🔨 BUILDING XAVIER (GLLM)"

    if ($Cuda -and $nvidia -eq "YES") {
        Write-Step "Building with CUDA features (local-gllm-cuda)" "🚀"
        Push-Location $XavierRoot
        cargo build --features local-gllm-cuda --release 2>&1 | Tee-Object -FilePath (Join-Path $XavierRoot $OutputDir "build-cuda-$Timestamp.log")
        if ($LASTEXITCODE -ne 0) {
            Write-Step "CUDA build failed — falling back to wgpu" "⚠️"
            Push-Location $XavierRoot
            cargo build --features local-gllm --release 2>&1 | Tee-Object -FilePath (Join-Path $XavierRoot $OutputDir "build-wgpu-$Timestamp.log")
        }
        Pop-Location
    } else {
        Write-Step "Building with default features (local-gllm wgpu)" "🚀"
        Push-Location $XavierRoot
        cargo build --features local-gllm --release 2>&1 | Tee-Object -FilePath (Join-Path $XavierRoot $OutputDir "build-wgpu-$Timestamp.log")
        Pop-Location
    }

    Write-Step "Build complete" "✅"
}

# ════════════════════════════════════════════════════════════════
# RUN BENCHMARKS
# ════════════════════════════════════════════════════════════════

$results = @()
$startTime = Get-Date

# ─── Model 1: all-MiniLM-L6-v2 (baseline) ───
if ($Model -eq "all" -or $Model -eq "minilm") {
    Write-Banner "📊 [1/5] GLLM: all-MiniLM-L6-v2 (384d, MTEB 58.8)"
    $ok = Measure-Embedder -Name "minilm-l6-v2" -EnvVars ""
    if ($ok) { $results += "minilm-l6-v2: OK" } else { $results += "minilm-l6-v2: FAIL" }
}

# ─── Model 2: all-mpnet-base-v2 (new default) ───
if ($Model -eq "all" -or $Model -eq "mpnet") {
    Write-Banner "📊 [2/5] GLLM: all-mpnet-base-v2 (768d, MTEB 63.0)"
    $ok = Measure-Embedder -Name "mpnet-base-v2" -EnvVars "`$env:XAVIER_GLLM_MODEL='all-mpnet-base-v2'"
    if ($ok) { $results += "mpnet-base-v2: OK" } else { $results += "mpnet-base-v2: FAIL" }
}

# ─── Model 3: Qwen3-Embedding-0.6B ───
if ($Model -eq "all" -or $Model -eq "qwen3") {
    Write-Banner "📊 [3/5] GLLM: Qwen3-Embedding-0.6B (1024d, MTEB ~67.5)"
    $ok = Measure-Embedder -Name "qwen3-embedding-0.6b" -EnvVars "`$env:XAVIER_GLLM_MODEL='Qwen/Qwen3-Embedding-0.6B'"
    if ($ok) { $results += "qwen3-embedding-0.6b: OK" } else { $results += "qwen3-embedding-0.6b: FAIL" }
}

# ─── Model 4: Docker backend ───
if (($Model -eq "all" -or $Model -eq "docker") -and $docker -eq "YES") {
    Write-Banner "📊 [4/5] Docker: gte-Qwen2-1.5B / Ollama"

    # Check if Infinity/TEI is running
    $dockerRunning = $false
    try {
        $response = Invoke-WebRequest -Uri "$DockerEndpoint/models" -UseBasicParsing -TimeoutSec 3
        if ($response.StatusCode -eq 200) {
            $dockerRunning = $true
            Write-Step "Docker endpoint $DockerEndpoint is live" "✅"
        }
    } catch {
        Write-Step "Docker endpoint $DockerEndpoint not reachable" "⚠️"
    }

    if ($dockerRunning) {
        $ok = Measure-Embedder -Name "docker-infinity" -EnvVars "`$env:XAVIER_BENCH_DOCKER_URL='$DockerEndpoint'; `$env:XAVIER_BENCH_DOCKER_MODEL='$DockerModel'"
        if ($ok) { $results += "docker-gte-qwen2-1.5b: OK" } else { $results += "docker-gte-qwen2-1.5b: FAIL" }
    } else {
        # Check Ollama
        try {
            $ollamaResp = Invoke-WebRequest -Uri "http://localhost:11434/api/tags" -UseBasicParsing -TimeoutSec 3
            if ($ollamaResp.StatusCode -eq 200) {
                Write-Step "Ollama is running" "✅"
                $ok = Measure-Embedder -Name "ollama" -EnvVars "`$env:XAVIER_BENCH_DOCKER_URL='http://localhost:11434/v1/embeddings'; `$env:XAVIER_BENCH_DOCKER_MODEL='nomic-embed-text'"
                if ($ok) { $results += "ollama-nomic-embed-text: OK" } else { $results += "ollama-nomic-embed-text: FAIL" }
            }
        } catch {
            Write-Step "No Docker embedding server found" "⚠️"
        }
    }
} else {
    if ($docker -eq "NO") {
        Write-Step "Docker not available — skipping Docker benchmarks" "⚠️"
    }
}

# ─── Model 5: OpenRouter cloud ───
if ($Model -eq "all" -or $Model -eq "openrouter") {
    if ($orKey) {
        Write-Banner "📊 [5/5] OpenRouter: text-embedding-3-small (1536d, MTEB 62.3)"
        $ok = Measure-Embedder -Name "openrouter-text-embedding-3-small" -EnvVars ""
        if ($ok) { $results += "openrouter-text-embedding-3-small: OK" } else { $results += "openrouter-text-embedding-3-small: FAIL" }
    } else {
        Write-Step "OpenRouter not available — skipping cloud benchmarks" "⚠️"
    }
}

# ════════════════════════════════════════════════════════════════
# SUMMARY
# ════════════════════════════════════════════════════════════════

$endTime = Get-Date
$duration = ($endTime - $startTime).TotalMinutes

Write-Banner "📋 BENCHMARK SUMMARY"

Write-Host ""
Write-Host "  Duration: $([math]::Round($duration, 1)) minutes"
Write-Host "  Date:     $($startTime.ToString('yyyy-MM-dd HH:mm'))"
Write-Host "  Host:     $env:COMPUTERNAME"
Write-Host "  CUDA:     $nvidia"
Write-Host "  Docker:   $docker"
Write-Host ""

foreach ($r in $results) {
    $status = if ($r -match ": OK") { "✅" } else { "❌" }
    Write-Host "  $status $r"
}

Write-Host ""
Write-Host "  📄 Logs saved to: $OutputDir/"
Write-Host ""

# Generate markdown summary
$summary = @"
# Embedding Benchmark Results — Xavier v0.6.1-beta

**Date:** $($startTime.ToString('yyyy-MM-dd HH:mm'))
**Host:** $env:COMPUTERNAME
**Duration:** $([math]::Round($duration, 1)) min
**GPU/CUDA:** $nvidia

## Models Tested

$($results | ForEach-Object { "- $_" }) | Out-String)

## Notes

- GLLM models run natively in-process (no network call)
- Docker models require running Infinity/TEI/Ollama container
- OpenRouter requires API key and internet access
- CUDA models require NVIDIA GPU with 8GB+ VRAM

## Next Steps

1. Compare retrieval precision across all working models
2. Pick best model as new default in Cargo.toml
3. Configure XAVIER_GLLM_MODEL env var for production
"@

$summary | Out-File -FilePath $SummaryFile -Encoding utf8
Write-Host "  📄 Summary saved to: $SummaryFile" -ForegroundColor Green
Write-Host ""

Write-Host "  🏁 Benchmark complete!" -ForegroundColor Green
Write-Host ""
