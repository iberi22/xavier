#!/usr/bin/env pwsh
param(
    [switch]$fast,
    [string]$job = "",
    [switch]$verbose
)

$ErrorActionPreference = "Stop"
$REPO_ROOT = (Get-Item $PSScriptRoot).FullName
$START = Get-Date

function Log($m) { Write-Host "[$(Get-Date -Format HH:mm:ss)] $m" -ForegroundColor Cyan }
function LogOk($m) { Write-Host "  [OK] $m" -ForegroundColor Green }
function LogFail($m) { Write-Host "  [FAIL] $m" -ForegroundColor Red }

Push-Location $REPO_ROOT

if ($job) {
    Log "Running single job: $job via act"
    act --workflows .github/workflows/ci.yml --job $job 2>&1
    Pop-Location; exit $LASTEXITCODE
}

# Phase 1: Rust formatting
Log "cargo fmt --check"
cargo fmt --all -- --check 2>&1
if ($LASTEXITCODE -ne 0) { LogFail "fmt failed"; Pop-Location; exit 1 }
LogOk "fmt OK"

# Phase 2: Rust check
Log "cargo check"
cargo check --workspace --all-targets --features ci-safe --exclude xavier-web --exclude app 2>&1
if ($LASTEXITCODE -ne 0) { LogFail "check failed"; Pop-Location; exit 1 }
LogOk "check OK"

# Phase 3: Clippy + Tests (slow, skip with -fast)
if (-not $fast) {
    Log "cargo clippy"
    cargo clippy --workspace --all-targets --features ci-safe --exclude xavier-web --exclude app -- -D warnings 2>&1
    if ($LASTEXITCODE -ne 0) { LogFail "clippy failed"; Pop-Location; exit 1 }
    LogOk "clippy OK"

    Log "cargo test"
    cargo test --workspace --no-default-features --features ci-safe --exclude xavier-web --exclude app 2>&1
    if ($LASTEXITCODE -ne 0) { LogFail "tests failed"; Pop-Location; exit 1 }
    LogOk "tests OK"
}

# Phase 4: Panel UI
Log "pnpm install"
pnpm install --frozen-lockfile --config.dangerouslyAllowAllBuilds=true 2>&1
if ($LASTEXITCODE -ne 0) { LogFail "pnpm install failed"; Pop-Location; exit 1 }

Log "pnpm panel-ui check"
pnpm --filter xavier-panel-ui check 2>&1
if ($LASTEXITCODE -ne 0) { LogFail "panel-ui check failed"; Pop-Location; exit 1 }
LogOk "panel-ui check OK"

if (-not $fast) {
    Log "pnpm panel-ui test"
    pnpm --filter xavier-panel-ui test 2>&1
    if ($LASTEXITCODE -ne 0) { LogFail "panel-ui tests failed"; Pop-Location; exit 1 }
    LogOk "panel-ui tests OK"
}

# Phase 5: Docker CI via act (matches GitHub Actions)
if (-not $fast) {
    Log "Running Docker CI via act"
    act --workflows .github/workflows/ci.yml --job check 2>&1
    if ($LASTEXITCODE -ne 0) { LogFail "Docker CI failed"; Pop-Location; exit 1 }
    LogOk "Docker CI passed"
}

Pop-Location

$ELAPSED = [math]::Round(((Get-Date) - $START).TotalSeconds, 1)
Write-Host ""
Write-Host "<<<< ALL CHECKS PASSED (${ELAPSED}s) >>>>" -ForegroundColor Green
