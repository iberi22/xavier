# Xavier Installer Build Script
# This script builds the Windows installer for Xavier using WiX or Inno Setup.

$ErrorActionPreference = "Stop"
$PSScriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Definition
Set-Location $PSScriptRoot

Write-Host "--- Xavier Installer Build ---" -ForegroundColor Cyan

# 1. Check for binaries
$Binaries = @("..\target\release\xavier.exe", "..\target\release\xavier-tui.exe", "..\target\release\xavier-gui.exe")
foreach ($bin in $Binaries) {
    if (-not (Test-Path $bin)) {
        Write-Warning "Binary not found: $bin. Building now..."
        Push-Location ..
        cargo build --release --features "cli-interactive,egui-standalone"
        Pop-Location
        break
    }
}

# 2. Check for Panel UI
if (-not (Test-Path "..\panel-ui\build")) {
    Write-Warning "Panel UI build not found. Attempting to build..."
    Push-Location ..\panel-ui
    npm install
    npm run build
    Pop-Location
}

# 3. Detect Installer Tool
$WiXFound = Get-Command candle.exe -ErrorAction SilentlyContinue
$InnoFound = Get-Command iscc.exe -ErrorAction SilentlyContinue

# Prefer Inno Setup as it currently has better handling for the Panel UI directory structure
if ($InnoFound) {
    Write-Host "Building Inno Setup Installer..." -ForegroundColor Green
    iscc.exe setup.iss
    Write-Host "Done! Created output in Output/XavierSetup.exe" -ForegroundColor Cyan
}
elseif ($WiXFound) {
    Write-Host "Building WiX Installer..." -ForegroundColor Green

    # Check for heat.exe to harvest Panel UI files
    $HeatFound = Get-Command heat.exe -ErrorAction SilentlyContinue
    if ($HeatFound -and (Test-Path "..\panel-ui\build")) {
        Write-Host "Harvesting Panel UI files..." -ForegroundColor Gray
        & heat.exe dir "..\panel-ui\build" -dr PANELUIFOLDER -cg PanelUIComponents -gg -sreg -sfrag -srd -out panel-ui-files.wxs
        candle.exe xavier.wxs panel-ui-files.wxs
        light.exe xavier.wixobj panel-ui-files.wixobj -o XavierInstaller.msi
    }
    else {
        Write-Warning "heat.exe not found or panel-ui/build missing. MSI will have an empty panel-ui folder."
        candle.exe xavier.wxs
        light.exe xavier.wixobj -o XavierInstaller.msi
    }

    Write-Host "Done! Created XavierInstaller.msi" -ForegroundColor Cyan
}
elseif ($InnoFound) {
    Write-Host "Building Inno Setup Installer..." -ForegroundColor Green
    iscc.exe setup.iss
    Write-Host "Done! Created output in Output/XavierSetup.exe" -ForegroundColor Cyan
}
else {
    Write-Error "Neither WiX Toolset nor Inno Setup was found in PATH."
    Write-Host "Please install WiX Toolset (v3.11+) or Inno Setup (v6+) to continue." -ForegroundColor Yellow
    exit 1
}
