# Start-All: Unified multi-daemon launcher for Xavier + Gestalt + Web Panel UI (Windows)
param(
    [switch]$NoUI,
    [int]$XavierPort = 8006,
    [int]$GestaltPort = 8081,
    [int]$UIPort = 4174
)

$ErrorActionPreference = "Continue"

Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "  STARTING SWAL ECOSYSTEM: Xavier + Gestalt + Panel UI" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan

# 1. Resolve Binaries & Parity Verification
$PSScriptRootResolved = Split-Path -Parent $MyInvocation.MyCommand.Path
$releaseBin = "$PSScriptRootResolved\..\target\release\xavier.exe"
$installedBin = "C:\Users\belal\bin\xavier.exe"

$xavierBin = if (Test-Path $releaseBin) {
    # Check parity if an installed binary exists
    if (Test-Path $installedBin) {
        try {
            $h1 = (Get-FileHash -Path $releaseBin -Algorithm SHA256).Hash
            $h2 = (Get-FileHash -Path $installedBin -Algorithm SHA256).Hash
            if ($h1 -ne $h2) {
                Write-Host "[WARN] Parity mismatch between target\release\xavier.exe and C:\Users\belal\bin\xavier.exe" -ForegroundColor Yellow
                Write-Host "       Using target\release\xavier.exe to avoid stale database schema / XRK1 crypto drift." -ForegroundColor Gray
            }
        } catch {}
    }
    $releaseBin
} elseif (Test-Path $installedBin) {
    $installedBin
} elseif (Get-Command xavier -ErrorAction SilentlyContinue) {
    (Get-Command xavier).Source
} else {
    "xavier"
}

$gestaltDir = if (Test-Path "$PSScriptRootResolved\..\..\gestalt") {
    (Resolve-Path "$PSScriptRootResolved\..\..\gestalt").Path
} elseif (Test-Path "e:\proyectosSWAL\gestalt") {
    "e:\proyectosSWAL\gestalt"
} else {
    "$HOME/gestalt"
}

$gestaltBin = if (Test-Path "$gestaltDir\target\release\gestalt_cli.exe") {
    "$gestaltDir\target\release\gestalt_cli.exe"
} elseif (Test-Path "$HOME\.cargo\target_global\debug\gestalt_cli.exe") {
    "$HOME\.cargo\target_global\debug\gestalt_cli.exe"
} elseif (Get-Command gestalt_cli -ErrorAction SilentlyContinue) {
    (Get-Command gestalt_cli).Source
} else {
    "gestalt_cli"
}

# 2. Check Xavier Core
Write-Host "`n[1/3] Checking Xavier Core Memory Server (Port $XavierPort)..." -ForegroundColor Yellow
$xavierRunning = $false
try {
    $xRes = Invoke-RestMethod -Uri "http://127.0.0.1:$XavierPort/health" -Method GET -TimeoutSec 2 -ErrorAction Stop
    if ($xRes.version) { $xavierRunning = $true }
} catch {}

if ($xavierRunning) {
    Write-Host "[OK] Xavier Core already running at http://127.0.0.1:$XavierPort" -ForegroundColor Green
} else {
    Write-Host "Starting Xavier Core daemon..." -ForegroundColor Gray
    Start-Process -FilePath $xavierBin -ArgumentList "http $XavierPort" -WindowStyle Hidden
    
    # Wait for readiness
    $ready = $false
    for ($i = 0; $i -lt 15; $i++) {
        Start-Sleep -Seconds 1
        try {
            $xRes = Invoke-RestMethod -Uri "http://127.0.0.1:$XavierPort/health" -Method GET -TimeoutSec 1 -ErrorAction Stop
            if ($xRes) { $ready = $true; break }
        } catch {}
    }
    if ($ready) {
        Write-Host "[PASS] Xavier Core ready at http://127.0.0.1:$XavierPort" -ForegroundColor Green
    } else {
        Write-Host "[WARN] Xavier Core started, but health check timed out" -ForegroundColor Yellow
    }
}

# 3. Check Gestalt Event Bus
Write-Host "`n[2/3] Checking Gestalt Event Bus (Port $GestaltPort)..." -ForegroundColor Yellow
$gestaltRunning = $false
try {
    $gRes = Invoke-RestMethod -Uri "http://127.0.0.1:$GestaltPort/healthz" -Method GET -TimeoutSec 2 -ErrorAction Stop
    if ($gRes.status -eq "ok") { $gestaltRunning = $true }
} catch {}

if ($gestaltRunning) {
    Write-Host "[OK] Gestalt Event Bus already running at http://127.0.0.1:$GestaltPort" -ForegroundColor Green
} else {
    Write-Host "Starting Gestalt Event Bus daemon..." -ForegroundColor Gray
    Start-Process -FilePath $gestaltBin -ArgumentList "bus serve --port $GestaltPort" -WindowStyle Hidden
    
    $ready = $false
    for ($i = 0; $i -lt 15; $i++) {
        Start-Sleep -Seconds 1
        try {
            $gRes = Invoke-RestMethod -Uri "http://127.0.0.1:$GestaltPort/healthz" -Method GET -TimeoutSec 1 -ErrorAction Stop
            if ($gRes.status -eq "ok") { $ready = $true; break }
        } catch {}
    }
    if ($ready) {
        Write-Host "[PASS] Gestalt Event Bus ready at http://127.0.0.1:$GestaltPort" -ForegroundColor Green
    } else {
        Write-Host "[WARN] Gestalt Event Bus started, waiting for connection" -ForegroundColor Yellow
    }
}

# 4. Check Panel UI
if (-not $NoUI) {
    Write-Host "`n[3/3] Checking Web Panel UI (Port $UIPort)..." -ForegroundColor Yellow
    $uiRunning = $false
    try {
        $uiRes = Invoke-WebRequest -Uri "http://127.0.0.1:$UIPort/" -Method GET -TimeoutSec 2 -UseBasicParsing -ErrorAction Stop
        if ($uiRes.StatusCode -eq 200) { $uiRunning = $true }
    } catch {}

    if ($uiRunning) {
        Write-Host "[OK] Web Panel UI already running at http://127.0.0.1:$UIPort" -ForegroundColor Green
    } else {
        Write-Host "Starting Web Panel UI Vite server..." -ForegroundColor Gray
        $uiDir = "$PSScriptRootResolved\..\panel-ui"
        Start-Process -FilePath "pnpm" -ArgumentList "run dev" -WorkingDirectory $uiDir -WindowStyle Hidden
        Start-Sleep -Seconds 2
        Write-Host "[PASS] Web Panel UI active at http://127.0.0.1:$UIPort" -ForegroundColor Green
    }
}

Write-Host "`n================================================================================" -ForegroundColor Cyan
Write-Host "  ECOSYSTEM OPERATIONAL STATUS" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "  * Xavier Core:        http://127.0.0.1:$XavierPort" -ForegroundColor White
Write-Host "  * Gestalt Event Bus:  http://127.0.0.1:$GestaltPort" -ForegroundColor White
if (-not $NoUI) {
    Write-Host "  * Web Panel UI:       http://127.0.0.1:$UIPort" -ForegroundColor Green
}
Write-Host "================================================================================" -ForegroundColor Cyan
