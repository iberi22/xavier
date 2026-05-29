#!/usr/bin/env pwsh
#requires -Version 5.1
# Xavier v0.6.1-beta - Windows Installer
# One-liner: irm https://raw.githubusercontent.com/iberi22/xavier/main/install.ps1 | iex

param(
    [string]$InstallDir = "$env:USERPROFILE\.xavier",
    [string]$Version = "0.6.1-beta",
    [string]$Token = "",
    [int]$Port = 8006,
    [switch]$SkipRustCheck,
    [switch]$SkipBuild,
    [switch]$AsService,
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$script:StartTime = Get-Date

# ─── Colors ───────────────────────────────────────────────────────────────
function Write-Info($msg) { Write-Host "  [INFO] $msg" -ForegroundColor Cyan }
function Write-Ok($msg) { Write-Host "  [OK]   $msg" -ForegroundColor Green }
function Write-Warn($msg) { Write-Host "  [WARN] $msg" -ForegroundColor Yellow }
function Write-Err($msg) { Write-Host "  [ERR]  $msg" -ForegroundColor Red }

# ─── Banner ───────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "╔══════════════════════════════════════════════════════════════╗" -ForegroundColor Cyan
Write-Host "║  Xavier v0.6.1-beta - Fast Vector Memory for AI Agents       ║" -ForegroundColor Cyan
Write-Host "║  Windows Installer                                           ║" -ForegroundColor Cyan
Write-Host "╚══════════════════════════════════════════════════════════════╝" -ForegroundColor Cyan
Write-Host ""

# ─── Pre-flight checks ────────────────────────────────────────────────────
Write-Host "=== Pre-flight Checks ===" -ForegroundColor Yellow

if (-not $SkipRustCheck) {
    $rustc = (Get-Command rustc -ErrorAction SilentlyContinue)
    $cargo = (Get-Command cargo -ErrorAction SilentlyContinue)
    if (-not $rustc -or -not $cargo) {
        Write-Warn "Rust not found. Installing via rustup..."
        $rustupInit = Join-Path $env:TEMP "rustup-init.exe"
        try {
            Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $rustupInit -UseBasicParsing
            Start-Process -FilePath $rustupInit -ArgumentList "-y","--default-toolchain","stable","--profile","default" -Wait -NoNewWindow
            $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
            [Environment]::SetEnvironmentVariable("PATH", $env:PATH, "User")
            Write-Ok "Rust installed successfully"
        } finally {
            if (Test-Path $rustupInit) { Remove-Item $rustupInit -Force }
        }
    } else {
        Write-Ok "Rust found: $(rustc --version)"
        Write-Ok "Cargo found: $(cargo --version)"
    }
} else {
    Write-Info "Skipping Rust check (--SkipRustCheck)"
}

# Check git
$git = (Get-Command git -ErrorAction SilentlyContinue)
if (-not $git) {
    Write-Err "Git is required. Install Git for Windows first: https://git-scm.com/download/win"
    exit 1
}
Write-Ok "Git found: $(git --version)"

# Check Node.js (optional, for panel-ui)
$node = (Get-Command node -ErrorAction SilentlyContinue)
if (-not $node) {
    Write-Warn "Node.js not found. Panel UI will not be available."
    Write-Info "Install from: https://nodejs.org/ (v22.12.0 recommended)"
} else {
    Write-Ok "Node found: $(node --version)"
}

# Check cmake (required for libsql-ffi)
$cmake = (Get-Command cmake -ErrorAction SilentlyContinue)
if (-not $cmake) {
    Write-Warn "CMake not found. Installing via winget..."
    winget install --id Kitware.CMake --accept-source-agreements --accept-package-agreements
    $env:PATH = "$env:ProgramFiles\CMake\bin;$env:PATH"
}
Write-Ok "CMake found: $(cmake --version | Select-Object -First 1)"

# ─── Generate Token if not provided ───────────────────────────────────────
if (-not $Token) {
    Write-Info "Generating secure auth token..."
    $bytes = New-Object byte[] 32
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    $rng.GetBytes($bytes)
    $Token = "xavier_" + [Convert]::ToBase64String($bytes).Replace("/","_").Replace("+","-").Substring(0,32)
    Write-Ok "Generated token: $($Token.Substring(0,8))... (saved to config)"
} else {
    Write-Info "Using provided token"
}

# ─── Create directories ───────────────────────────────────────────────────
Write-Host ""
Write-Host "=== Installation ===" -ForegroundColor Yellow

if ((Test-Path $InstallDir) -and -not $Force) {
    Write-Warn "Directory $InstallDir already exists. Use -Force to overwrite."
    $continue = Read-Host "Continue anyway? [Y/n]"
    if ($continue -eq "n") { exit 0 }
} elseif (Test-Path $InstallDir) {
    Remove-Item $InstallDir -Recurse -Force
}

New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
New-Item -ItemType Directory -Path "$InstallDir\data" -Force | Out-Null
New-Item -ItemType Directory -Path "$InstallDir\logs" -Force | Out-Null
New-Item -ItemType Directory -Path "$InstallDir\config" -Force | Out-Null
Write-Ok "Created directories in $InstallDir"

# ─── Clone repository ────────────────────────────────────────────────────
Write-Info "Cloning Xavier repository..."
Push-Location $InstallDir
try {
    git clone --depth 1 --branch main https://github.com/iberi22/xavier.git repo
    Write-Ok "Repository cloned"
} finally {
    Pop-Location
}

# ─── Build ───────────────────────────────────────────────────────────────
if (-not $SkipBuild) {
    Write-Info "Building Xavier (this may take 10-30 minutes on first run)..."
    Push-Location "$InstallDir\repo"
    try {
        # Build with minimal features for Windows compatibility
        cargo build --release --bin xavier --features "local-gllm,cli-interactive" --no-default-features
        if ($LASTEXITCODE -ne 0) {
            Write-Err "Build failed. See output above."
            exit 1
        }
        Write-Ok "Build complete"
        
        # Copy binary
        Copy-Item "target\release\xavier.exe" "$InstallDir\xavier.exe" -Force
        Write-Ok "Binary copied to $InstallDir\xavier.exe"
    } finally {
        Pop-Location
    }
} else {
    Write-Info "Skipping build (--SkipBuild)"
    # Download prebuilt binary (if available)
    $prebuiltUrl = "https://github.com/iberi22/xavier/releases/download/v$Version/xavier-windows-x64.exe"
    Write-Info "Attempting to download prebuilt binary..."
    try {
        Invoke-WebRequest -Uri $prebuiltUrl -OutFile "$InstallDir\xavier.exe" -UseBasicParsing
        Write-Ok "Prebuilt binary downloaded"
    } catch {
        Write-Warn "No prebuilt binary available for v$Version"
        Write-Err "Cannot continue without binary. Run without --SkipBuild to compile from source."
        exit 1
    }
}

# ─── Configuration ─────────────────────────────────────────────────────────
Write-Info "Writing configuration..."

$config = @{
    server = @{
        host = "127.0.0.1"
        port = $Port
        log_level = "info"
        code_graph_db_path = "data/code_graph.db"
        url = "http://127.0.0.1:$Port"
    }
    workspace = @{
        default_workspace_id = "default"
        workspace_dir = "$InstallDir\data"
    }
    memory = @{
        backend = "vec"
        store_path = "$InstallDir\data\xavier.db"
        embedding_dim = 384
    }
    security = @{
        token_secret = $Token
        enabled = $true
        min_confidence_threshold = 0.5
        auto_sanitize = $true
    }
    router = @{
        default_provider = "minimax"
    }
    advanced = @{
        enable_metrics = $true
        enable_audit_log = $true
    }
} | ConvertTo-Json -Depth 4

$configPath = "$InstallDir\config\xavier.config.json"
$config | Out-File -FilePath $configPath -Encoding UTF8
Write-Ok "Configuration written to $configPath"

# ─── Environment setup ─────────────────────────────────────────────────────
Write-Info "Setting environment variables..."

[Environment]::SetEnvironmentVariable("XAVIER_HOME", $InstallDir, "User")
[Environment]::SetEnvironmentVariable("XAVIER_TOKEN", $Token, "User")
[Environment]::SetEnvironmentVariable("XAVIER_PORT", $Port, "User")
[Environment]::SetEnvironmentVariable("XAVIER_CONFIG_PATH", $configPath, "User")

# Update PATH
$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($userPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("PATH", "$userPath;$InstallDir", "User")
    Write-Ok "Added $InstallDir to PATH"
}

Write-Ok "Environment variables configured"

# ─── Create startup scripts ───────────────────────────────────────────────
Write-Info "Creating startup scripts..."

# Start script
$startScript = @"
@echo off
setlocal
set XAVIER_HOME=$InstallDir
set XAVIER_TOKEN=$Token
set XAVIER_PORT=$Port
set XAVIER_CONFIG_PATH=$configPath
set RUST_LOG=info

if not exist "$InstallDir\data" mkdir "$InstallDir\data"
if not exist "$InstallDir\logs" mkdir "$InstallDir\logs"

"$InstallDir\xavier.exe" http --config "$configPath"
"@
$startScript | Out-File -FilePath "$InstallDir\start.bat" -Encoding ASCII

# Service wrapper script (using nssm if available, else scheduled task)
$serviceScript = @"
@echo off
:: Xavier Windows Service Setup
:: Run as Administrator

set XAVIER_HOME=$InstallDir
set XAVIER_TOKEN=$Token
set XAVIER_PORT=$Port

if exist "$InstallDir\xavier.exe" (
    sc create Xavier binPath= "$InstallDir\xavier.exe http --config $configPath" start= auto
    sc description Xavier "Xavier v0.6.1-beta - AI Agent Memory Runtime"
    sc start Xavier
    echo Xavier service installed and started
) else (
    echo Xavier binary not found. Run install.ps1 first.
)
"@
$serviceScript | Out-File -FilePath "$InstallDir\install-service.bat" -Encoding ASCII

# Uninstall script
$uninstallScript = @"
@echo off
sc stop Xavier 2>nul
sc delete Xavier 2>nul
echo Xavier service removed (if existed)
echo To remove files, delete: $InstallDir
"@
$uninstallScript | Out-File -FilePath "$InstallDir\uninstall.bat" -Encoding ASCII

Write-Ok "Startup scripts created"

# ─── Verification script ──────────────────────────────────────────────────
Write-Info "Creating verification script..."

$verifyScript = @'
param([string]$BaseUrl = "http://127.0.0.1:8006", [string]$Token = $env:XAVIER_TOKEN)

if (-not $Token) { Write-Error "XAVIER_TOKEN not set"; exit 1 }

$headers = @{ "X-Xavier-Token" = $Token; "Content-Type" = "application/json" }

Write-Host "=== Xavier v0.6.1-beta Verification ===" -ForegroundColor Cyan
Write-Host "URL: $BaseUrl" -ForegroundColor Gray

# Health check
Write-Host "`n[1/5] Health check..." -ForegroundColor Yellow
try {
    $r = Invoke-RestMethod -Uri "$BaseUrl/health" -Method GET -TimeoutSec 5
    Write-Host "  Health: $($r -replace '[^\w]','')" -ForegroundColor Green
} catch { Write-Host "  FAIL: $($_.Exception.Message)" -ForegroundColor Red }

# Auth check
Write-Host "`n[2/5] Auth check..." -ForegroundColor Yellow
try {
    $r = Invoke-RestMethod -Uri "$BaseUrl/workspace/default" -Headers $headers -Method GET
    Write-Host "  Auth OK, workspace: $($r.workspace_id)" -ForegroundColor Green
} catch { Write-Host "  FAIL: $($_.Exception.Message)" -ForegroundColor Red }

# Memory add
Write-Host "`n[3/5] Memory add..." -ForegroundColor Yellow
try {
    $body = @{ content = "Test memory from Windows installer"; path = "test/install"; metadata = @{} } | ConvertTo-Json
    $r = Invoke-RestMethod -Uri "$BaseUrl/memory/add" -Headers $headers -Method POST -Body $body
    Write-Host "  Added: $($r.id)" -ForegroundColor Green
} catch { Write-Host "  FAIL: $($_.Exception.Message)" -ForegroundColor Red }

# Memory search
Write-Host "`n[4/5] Memory search..." -ForegroundColor Yellow
try {
    $body = @{ query = "Test memory"; limit = 5 } | ConvertTo-Json
    $r = Invoke-RestMethod -Uri "$BaseUrl/memory/search" -Headers $headers -Method POST -Body $body
    Write-Host "  Found: $($r.count) results" -ForegroundColor Green
} catch { Write-Host "  FAIL: $($_.Exception.Message)" -ForegroundColor Red }

# Sync check
Write-Host "`n[5/5] Sync check..." -ForegroundColor Yellow
try {
    $r = Invoke-RestMethod -Uri "$BaseUrl/xavier/sync/check" -Headers $headers -Method GET
    Write-Host "  Status: $($r.status), Agents: $($r.active_agents)" -ForegroundColor Green
} catch { Write-Host "  FAIL: $($_.Exception.Message)" -ForegroundColor Red }

Write-Host "`n=== Verification Complete ===" -ForegroundColor Cyan
'@
$verifyScript | Out-File -FilePath "$InstallDir\verify.ps1" -Encoding UTF8
Write-Ok "Verification script created"

# ─── Test startup ─────────────────────────────────────────────────────────
Write-Host ""
Write-Host "=== Testing Startup ===" -ForegroundColor Yellow

$proc = Start-Process -FilePath "$InstallDir\xavier.exe" -ArgumentList "http", "--config", $configPath -WindowStyle Hidden -PassThru
Start-Sleep -Seconds 5

Write-Info "Waiting for server to start (5s)..."
$healthOk = $false
try {
    $r = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/health" -Method GET -TimeoutSec 3
    if ($r -eq "ok") { $healthOk = $true }
} catch { }

if ($healthOk) {
    Write-Ok "Server responding on port $Port"
    
    # Run verification
    & "$InstallDir\verify.ps1" -BaseUrl "http://127.0.0.1:$Port" -Token $Token
} else {
    Write-Warn "Server not responding yet. It may need more time to initialize."
    Write-Info "Run '$InstallDir\verify.ps1' later to check."
}

# Stop test process
Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue

# ─── Done ─────────────────────────────────────────────────────────────────
$elapsed = (Get-Date) - $script:StartTime
Write-Host ""
Write-Host "╔══════════════════════════════════════════════════════════════╗" -ForegroundColor Green
Write-Host "║  Installation Complete!                                      ║" -ForegroundColor Green
Write-Host "╚══════════════════════════════════════════════════════════════╝" -ForegroundColor Green
Write-Host ""
Write-Info "Location:      $InstallDir"
Write-Info "Binary:        $InstallDir\xavier.exe"
Write-Info "Config:        $configPath"
Write-Info "Data:          $InstallDir\data"
Write-Info "Token:         $($Token.Substring(0,8))... (saved in config)"
Write-Info "Port:          $Port"
Write-Info ""
Write-Host "  Quick Start:" -ForegroundColor Cyan
Write-Host "    $InstallDir\start.bat              # Start server"
Write-Host "    $InstallDir\verify.ps1             # Verify installation"
Write-Host "    $InstallDir\install-service.bat     # Install as Windows Service (Admin)"
Write-Host ""
Write-Host "  API Endpoints:" -ForegroundColor Cyan
Write-Host "    POST /memory/add       - Add memory"
Write-Host "    POST /memory/search    - Search memory"
Write-Host "    POST /xavier/verify/save - Verify save/retrieve"
Write-Host "    GET  /health           - Health check"
Write-Host ""
Write-Host "  Compatible with Xavier2 clients (X-Xavier-Token auth)" -ForegroundColor Cyan
Write-Host ""
Write-Host "  Time: $($elapsed.TotalMinutes.ToString('0.0')) minutes" -ForegroundColor Gray
Write-Host ""
