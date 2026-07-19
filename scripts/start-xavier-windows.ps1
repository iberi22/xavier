# scripts/start-xavier-windows.ps1
# Kill -> start -> health-check Xavier HTTP on Windows (Graph Explorer ready).
#
# Prefer HTTP over MCP until tools/list protocol drift is fixed.
#
# Usage:
#   .\scripts\start-xavier-windows.ps1
#   .\scripts\start-xavier-windows.ps1 -Port 8006 -McpPort 0
#   $env:XAVIER_TOKEN = "<64-hex>"; .\scripts\start-xavier-windows.ps1
#
# Env (optional):
#   XAVIER_TOKEN              required (or in user env / .env)
#   XAVIER_DATA_DIR           default: %APPDATA%\xavier  (real production DB)
#   XAVIER_MEMORY_VEC_PATH    default: $XAVIER_DATA_DIR\vec-store.sqlite3
#   XAVIER_BIN                override binary path

[CmdletBinding()]
param(
    [int]$Port = 8006,
    [int]$McpPort = 0,
    [int]$HealthTimeoutSec = 40,
    [switch]$SkipKill,
    [switch]$NoInstall
)

$ErrorActionPreference = "Stop"
$rootDir = (Get-Item -Path "$PSScriptRoot\..").FullName

function Write-Step([string]$msg) { Write-Host "[xavier] $msg" -ForegroundColor Cyan }
function Write-Ok([string]$msg)   { Write-Host "[xavier] $msg" -ForegroundColor Green }
function Write-Warn([string]$msg) { Write-Host "[xavier] $msg" -ForegroundColor Yellow }
function Write-ErrMsg([string]$msg)  { Write-Host "[xavier] $msg" -ForegroundColor Red }

# --- Load .env if present (does not override existing process env) ---
$envFile = Join-Path $rootDir ".env"
if (Test-Path $envFile) {
    Get-Content $envFile | Where-Object { $_ -match '=' -and $_ -notmatch '^\s*#' } | ForEach-Object {
        if ($_ -match '^\s*([^=]+)=(.*)$') {
            $name = $Matches[1].Trim()
            $value = ($Matches[2].Trim() -replace '^["'']|["'']$', '')
            if (-not [Environment]::GetEnvironmentVariable($name, "Process")) {
                [Environment]::SetEnvironmentVariable($name, $value, "Process")
            }
        }
    }
}

# --- Defaults for Windows production data ---
if (-not $env:XAVIER_DATA_DIR) {
    $env:XAVIER_DATA_DIR = Join-Path $env:APPDATA "xavier"
}
if (-not $env:XAVIER_MEMORY_VEC_PATH) {
    $env:XAVIER_MEMORY_VEC_PATH = Join-Path $env:XAVIER_DATA_DIR "vec-store.sqlite3"
}
if (-not $env:XAVIER_HOME) {
    $env:XAVIER_HOME = Join-Path $env:USERPROFILE ".xavier"
}

if (-not $env:XAVIER_TOKEN -or $env:XAVIER_TOKEN.Length -lt 16) {
    Write-ErrMsg "XAVIER_TOKEN missing or too short. Set it in the environment or .env"
    exit 1
}

# --- Resolve binary (prefer release, then ola-graph install, then PATH) ---
$candidates = @(
    $env:XAVIER_BIN,
    (Join-Path $rootDir "target\release\xavier.exe"),
    (Join-Path $rootDir "dist\xavier-ola-graph.exe"),
    (Join-Path $rootDir "dist\xavier.exe"),
    "C:\Users\belal\bin\xavier-ola-graph.exe",
    "C:\Users\belal\bin\xavier.exe"
) | Where-Object { $_ -and (Test-Path $_) }

if (-not $candidates) {
    Write-ErrMsg "No xavier.exe found. Build with:"
    Write-Host '  cargo build --release --bin xavier --features "cli-interactive"'
    exit 1
}
$bin = $candidates[0]
Write-Step "Binary: $bin"

# --- Optional install to stable path (skip if locked) ---
if (-not $NoInstall) {
    $installDir = "C:\Users\belal\bin"
    if (Test-Path $installDir) {
        try {
            Copy-Item $bin (Join-Path $installDir "xavier-ola-graph.exe") -Force
            Write-Ok "Installed xavier-ola-graph.exe"
        } catch {
            Write-Warn "Could not install xavier-ola-graph.exe: $_"
        }
        try {
            Copy-Item $bin (Join-Path $installDir "xavier.exe") -Force
            Write-Ok "Installed xavier.exe"
        } catch {
            Write-Warn "xavier.exe locked (Grok MCP respawn?) - use xavier-ola-graph.exe: $_"
        }
    }
    $dist = Join-Path $rootDir "dist"
    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    Copy-Item $bin (Join-Path $dist "xavier.exe") -Force -ErrorAction SilentlyContinue
    Copy-Item $bin (Join-Path $dist "xavier-ola-graph.exe") -Force -ErrorAction SilentlyContinue
}

# --- Kill existing ---
if (-not $SkipKill) {
    Write-Step "Stopping existing xavier.exe processes..."
    Get-Process xavier -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2
    $left = @(Get-Process xavier -ErrorAction SilentlyContinue)
    if ($left.Count -gt 0) {
        Write-Warn "Still running after stop: $($left.Id -join ', ') - taskkill /F"
        taskkill /F /IM xavier.exe 2>$null | Out-Null
        Start-Sleep -Seconds 2
    }
}

# --- Logs ---
$logDir = $env:XAVIER_HOME
if (-not (Test-Path $logDir)) { New-Item -ItemType Directory -Force -Path $logDir | Out-Null }
$logOut = Join-Path $logDir "xavier-http-start.log"
$logErr = Join-Path $logDir "xavier-http-start.err"
Remove-Item $logOut, $logErr -ErrorAction SilentlyContinue

# --- Start ---
Write-Step "Starting HTTP on 0.0.0.0:$Port (mcp-port $McpPort)"
Write-Step "DATA_DIR=$($env:XAVIER_DATA_DIR)"
Write-Step "VEC=$($env:XAVIER_MEMORY_VEC_PATH)"

$argList = @("http", "$Port", "--mcp-port", "$McpPort")
$proc = Start-Process -FilePath $bin -ArgumentList $argList -PassThru -NoNewWindow `
    -RedirectStandardOutput $logOut -RedirectStandardError $logErr `
    -WorkingDirectory $rootDir

Write-Step "PID=$($proc.Id)"

# --- Health wait ---
$deadline = (Get-Date).AddSeconds($HealthTimeoutSec)
$healthy = $false
while ((Get-Date) -lt $deadline) {
    if ($proc.HasExited) {
        Write-ErrMsg "Process exited with code $($proc.ExitCode)"
        if (Test-Path $logOut) { Get-Content $logOut -Tail 40 | ForEach-Object { Write-Host $_ } }
        if (Test-Path $logErr) { Get-Content $logErr -Tail 40 | ForEach-Object { Write-Host $_ } }
        exit 1
    }
    try {
        $hdr = @{ "X-Xavier-Token" = $env:XAVIER_TOKEN }
        $r = Invoke-WebRequest -Uri "http://127.0.0.1:$Port/health" -Headers $hdr -UseBasicParsing -TimeoutSec 3
        if ($r.StatusCode -ge 200 -and $r.StatusCode -lt 300) {
            Write-Ok "Health OK ($($r.StatusCode))"
            $healthy = $true
            break
        }
    } catch {
        Start-Sleep -Seconds 2
    }
}

if (-not $healthy) {
    Write-ErrMsg "Health check timed out after ${HealthTimeoutSec}s"
    if (Test-Path $logOut) { Get-Content $logOut -Tail 30 | ForEach-Object { Write-Host $_ } }
    if (Test-Path $logErr) { Get-Content $logErr -Tail 30 | ForEach-Object { Write-Host $_ } }
    exit 1
}

# --- Quick smoke (graph stack) ---
Write-Step "Smoke: graph + panel endpoints"
$smokePaths = @(
    "/memory/graph/view",
    "/memory/graph/entities",
    "/code/graph/view",
    "/code/stats",
    "/panel/api/graph"
)
$hdr = @{ "X-Xavier-Token" = $env:XAVIER_TOKEN }
$fail = 0
foreach ($path in $smokePaths) {
    try {
        $uri = "http://127.0.0.1:${Port}${path}"
        $r = Invoke-WebRequest -Uri $uri -Headers $hdr -UseBasicParsing -TimeoutSec 15
        $code = [int]$r.StatusCode
        if ($code -ge 200 -and $code -lt 300) {
            Write-Ok "  $code $path"
        } else {
            Write-Warn "  $code $path"
            $fail++
        }
    } catch {
        $code = $null
        if ($_.Exception.Response) { $code = [int]$_.Exception.Response.StatusCode }
        Write-ErrMsg "  FAIL $code $path - $($_.Exception.Message)"
        $fail++
    }
}

Write-Host ""
Write-Ok "Xavier HTTP ready on :$Port (PID $($proc.Id))"
Write-Host "  Prefer HTTP memory API over MCP until tools/list is fixed."
Write-Host "  Logs: $logOut"
Write-Host "  If embeddings were invalidated: POST /memory/reindex"
if ($fail -gt 0) {
    Write-Warn "Smoke had $fail issue(s) - check logs"
    exit 2
}
exit 0
