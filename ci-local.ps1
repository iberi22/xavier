#!/usr/bin/env pwsh
param(
    [switch]$fast,
    [string]$job = "",
    [switch]$verbose
)

$REPO_ROOT = (Get-Item $PSScriptRoot).FullName
$START = Get-Date

function Log($m) { Write-Host "[$(Get-Date -Format HH:mm:ss)] $m" -ForegroundColor Cyan }
function LogOk($m) { Write-Host "  [OK] $m" -ForegroundColor Green }
function LogFail($m) { Write-Host "  [FAIL] $m" -ForegroundColor Red }

# Run a native command (cargo/pnpm/act) and return its exit code.
# We use "Continue" so stderr progress output (cargo writes build progress to
# stderr) does NOT abort the script. Callers must check the return code.
function Invoke-Native {
    param([scriptblock]$Block)
    $ErrorActionPreference = "Continue"
    & $Block 2>&1 | Out-Host
    $code = $LASTEXITCODE
    Set-Variable -Name ErrorActionPreference -Value "Continue" -Scope Script
    return $code
}

Push-Location $REPO_ROOT

if ($job) {
    Log "Running single job: $job via act"
    $code = Invoke-Native { act --workflows .github/workflows/ci.yml --job $job }
    Pop-Location; exit $code
}

# Phase 1: Rust formatting
# cargo fmt --all fails because xavier-core has unresolved module files
# (incomplete crate). Format the live packages individually instead.
Log "cargo fmt --all -- --check"
$code = Invoke-Native { cargo fmt --all -- --check }
if ($code -ne 0) { $fmtFailed = $true }
if ($fmtFailed) { LogFail "fmt failed"; Pop-Location; exit 1 }
LogOk "fmt OK"

# Phase 2: Rust check
# Excluded incomplete crates (nothing depends on them):
#   xavier-core                  — missing 7 module files, cross-deps on main crate
#   codegraph-parse-typescript   — stale PluginResponse fields (symbols/edges removed)
# app is the Tauri panel (built separately via pnpm tauri build).
Log "cargo check"
$code = Invoke-Native { cargo check --workspace --all-targets --features ci-safe --exclude app }
if ($code -ne 0) { LogFail "check failed"; Pop-Location; exit 1 }
LogOk "check OK"

# Phase 3: Clippy + Tests (slow, skip with -fast)
if (-not $fast) {
    Log "cargo clippy"
    $code = Invoke-Native { cargo clippy --workspace --all-targets --features ci-safe --exclude app -- -D warnings }
    if ($code -ne 0) { LogFail "clippy failed"; Pop-Location; exit 1 }
    LogOk "clippy OK"

    Log "cargo test"
    $code = Invoke-Native { cargo test --workspace --no-default-features --features ci-safe --exclude app }
    if ($code -ne 0) { LogFail "tests failed"; Pop-Location; exit 1 }
    LogOk "tests OK"
}

# Phase 4: Panel UI
Log "pnpm install"
$code = Invoke-Native { pnpm install --frozen-lockfile --config.dangerouslyAllowAllBuilds=true }
if ($code -ne 0) { LogFail "pnpm install failed"; Pop-Location; exit 1 }

Log "pnpm panel-ui check"
$code = Invoke-Native { pnpm --filter xavier-panel-ui check }
if ($code -ne 0) { LogFail "panel-ui check failed"; Pop-Location; exit 1 }
LogOk "panel-ui check OK"

if (-not $fast) {
    Log "pnpm panel-ui test"
    $code = Invoke-Native { pnpm --filter xavier-panel-ui test }
    if ($code -ne 0) { LogFail "panel-ui tests failed"; Pop-Location; exit 1 }
    LogOk "panel-ui tests OK"
}

# Phase 5: Docker CI via act (matches GitHub Actions)
if (-not $fast) {
    Log "Running Docker CI via act"
    $code = Invoke-Native { act --workflows .github/workflows/ci.yml --job check }
    if ($code -ne 0) { LogFail "Docker CI failed"; Pop-Location; exit 1 }
    LogOk "Docker CI passed"
}

Pop-Location

$ELAPSED = [math]::Round(((Get-Date) - $START).TotalSeconds, 1)
Write-Host ""
Write-Host "<<<< ALL CHECKS PASSED (${ELAPSED}s) >>>>" -ForegroundColor Green
