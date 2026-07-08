# Xavier Installer Build Script
# This script builds the Windows installer for Xavier using WiX or Inno Setup.
# The installer includes the Tauri Panel UI (with system tray) as the main app.

$ErrorActionPreference = "Stop"
$PSScriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Definition
Set-Location $PSScriptRoot

Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  Xavier Windows Installer Build" -ForegroundColor Cyan
Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""

# 1. Check and build backend binaries
Write-Host "[1/4] Checking backend binaries..." -ForegroundColor Yellow
$BackendBinaries = @("..\target\release\xavier.exe", "..\target\release\xavier-tui.exe")
$needsBuild = $false
foreach ($bin in $BackendBinaries) {
    if (-not (Test-Path $bin)) {
        Write-Warning "Binary not found: $bin"
        $needsBuild = $true
    }
}

if ($needsBuild) {
    Write-Host "  Building backend binaries..." -ForegroundColor Gray
    Push-Location ..
    try {
        cargo build --release --features "cli-interactive"
        Write-Host "  ✅ Backend binaries built" -ForegroundColor Green
    } finally {
        Pop-Location
    }
} else {
    Write-Host "  ✅ Backend binaries found" -ForegroundColor Green
}

# 2. Check and build Panel UI (Tauri)
Write-Host ""
Write-Host "[2/4] Checking Panel UI (Tauri)..." -ForegroundColor Yellow
$TauriExe = "..\target\release\app.exe"

if (-not (Test-Path $TauriExe)) {
    Write-Host "  Panel UI not built. Building now..." -ForegroundColor Gray
    Write-Host "  (This may take several minutes on first build)" -ForegroundColor Gray
    
    Push-Location ..\panel-ui
    try {
        # Install dependencies if needed
        if (-not (Test-Path "node_modules")) {
            Write-Host "  Installing Node dependencies..." -ForegroundColor Gray
            & pnpm install
        }
        
        # Build Panel UI with Tauri
        Write-Host "  Building Tauri application..." -ForegroundColor Gray
        & pnpm tauri build
        
        if (-not (Test-Path "..\target\release\app.exe")) {
            Write-Error "Panel UI build failed. Check the output above for errors."
            exit 1
        }
        
        Write-Host "  ✅ Panel UI built successfully" -ForegroundColor Green
    } finally {
        Pop-Location
    }
} else {
    Write-Host "  ✅ Panel UI found" -ForegroundColor Green
}

# 3. Detect Installer Tool
Write-Host ""
Write-Host "[3/4] Detecting installer tool..." -ForegroundColor Yellow
$WiXFound = Get-Command candle.exe -ErrorAction SilentlyContinue
$InnoFound = Get-Command iscc.exe -ErrorAction SilentlyContinue

# Prefer Inno Setup as it currently has better handling for the Panel UI directory structure
if ($InnoFound) {
    Write-Host "  Found: Inno Setup" -ForegroundColor Green
    Write-Host ""
    Write-Host "[4/4] Building installer with Inno Setup..." -ForegroundColor Yellow
    
    try {
        & iscc.exe setup.iss
        Write-Host ""
        Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan
        Write-Host "  ✅ Installer built successfully!" -ForegroundColor Green
        Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan
        Write-Host ""
        Write-Host "  Output: installer\Output\XavierSetup.exe" -ForegroundColor White
        Write-Host ""
    } catch {
        Write-Error "Inno Setup build failed: $_"
        exit 1
    }
}
elseif ($WiXFound) {
    Write-Host "  Found: WiX Toolset" -ForegroundColor Green
    Write-Host ""
    Write-Host "[4/4] Building installer with WiX..." -ForegroundColor Yellow
    
    try {
        candle.exe xavier.wxs
        light.exe xavier.wixobj -o XavierInstaller.msi
        
        Write-Host ""
        Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan
        Write-Host "  ✅ Installer built successfully!" -ForegroundColor Green
        Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan
        Write-Host ""
        Write-Host "  Output: installer\XavierInstaller.msi" -ForegroundColor White
        Write-Host ""
    } catch {
        Write-Error "WiX build failed: $_"
        exit 1
    }
}
else {
    Write-Error "Neither WiX Toolset nor Inno Setup was found in PATH."
    Write-Host ""
    Write-Host "Please install one of the following:" -ForegroundColor Yellow
    Write-Host "  • Inno Setup v6+: https://jrsoftware.org/isdl.php" -ForegroundColor Gray
    Write-Host "  • WiX Toolset v3.11+: https://wixtoolset.org/releases/" -ForegroundColor Gray
    Write-Host ""
    exit 1
}
