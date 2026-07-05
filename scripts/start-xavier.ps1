# start-xavier.ps1 — Stable server startup script for Xavier 0.12.0
#
# Resolves the binary path, generates an API token if missing,
# and starts the HTTP server on port 8006.

$XavierDir = Resolve-Path "$PSScriptRoot\.."
Set-Location $XavierDir

Write-Host "🚀 Initializing Xavier Cognitive Memory Runtime..." -ForegroundColor Cyan

# 1. Ensure binary exists
$BinPath = "target\release\xavier.exe"
if (-not (Test-Path $BinPath)) {
    $BinPath = "xavier.exe" # Try PATH
}

if (-not (Test-Path $BinPath)) {
    Write-Host "⚠️ Xavier binary not found in target\release. Attempting to build..." -ForegroundColor Yellow
    cargo build --release --features ci-safe
    if (-not $?) {
        Write-Host "❌ Build failed. Please ensure Rust is installed." -ForegroundColor Red
        exit 1
    }
}

# 2. Setup environment
if (-not $env:XAVIER_TOKEN) {
    Write-Host "🔑 Generating new access token..." -ForegroundColor Gray
    $env:XAVIER_TOKEN = & $BinPath token new | Select-Object -Last 1
}

if (-not $env:XAVIER_AGENTS_DIR) {
    $env:XAVIER_AGENTS_DIR = "$HOME\clawd\agents"
}

# 3. Start server
Write-Host "🌐 Starting HTTP server on http://localhost:8006" -ForegroundColor Green
& $BinPath http 8006
