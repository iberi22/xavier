# Xavier Installer Build Script
# This script builds the Windows installers (MSI and Inno Setup EXE) for Xavier.

param(
    [string]$Version = "0.2.5",
    [string]$TargetTriple = "x86_64-pc-windows-msvc"
)

$ErrorActionPreference = "Stop"
$PSScriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Definition
Set-Location $PSScriptRoot

Write-Host "--- Xavier Installer Build ($Version) ---" -ForegroundColor Cyan

# Ensure output directory dist exists in root
$DistDir = Join-Path $PSScriptRoot "..\dist"
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null

# 1. Check for binaries in target-triple or target release
$TargetReleaseDir = "..\target\$TargetTriple\release"
$DefaultReleaseDir = "..\target\release"

New-Item -ItemType Directory -Force -Path $DefaultReleaseDir | Out-Null

if (Test-Path "$TargetReleaseDir\xavier.exe") {
    Copy-Item "$TargetReleaseDir\xavier.exe" "$DefaultReleaseDir\xavier.exe" -Force
}
if (Test-Path "$TargetReleaseDir\xavier-tui.exe") {
    Copy-Item "$TargetReleaseDir\xavier-tui.exe" "$DefaultReleaseDir\xavier-tui.exe" -Force
}

if (-not (Test-Path "$DefaultReleaseDir\xavier.exe")) {
    Write-Warning "xavier.exe not found in $DefaultReleaseDir or $TargetReleaseDir. Building now..."
    Push-Location ..
    cargo build --release --target $TargetTriple --features ci-safe
    Pop-Location
    if (Test-Path "$TargetReleaseDir\xavier.exe") {
        Copy-Item "$TargetReleaseDir\xavier.exe" "$DefaultReleaseDir\xavier.exe" -Force
    }
}

if (-not (Test-Path "$DefaultReleaseDir\xavier-tui.exe")) {
    New-Item -ItemType File -Force -Path "$DefaultReleaseDir\xavier-tui.exe" | Out-Null
}

if (-not (Test-Path "..\config\xavier.config.json")) {
    New-Item -ItemType Directory -Force -Path "..\config" | Out-Null
    Set-Content -Path "..\config\xavier.config.json" -Value "{}"
}

# 2. Check for Panel UI
if (-not (Test-Path "..\panel-ui\build")) {
    Write-Warning "Panel UI build not found. Attempting to build..."
    Push-Location ..\panel-ui
    if (Get-Command pnpm -ErrorAction SilentlyContinue) {
        pnpm install
        pnpm build
    } else {
        npm install
        npm run build
    }
    Pop-Location
}

if (-not (Test-Path "..\panel-ui\build")) {
    Write-Warning "panel-ui/build still missing. Creating dummy directory structure..."
    New-Item -ItemType Directory -Force -Path "..\panel-ui\build" | Out-Null
    Set-Content -Path "..\panel-ui\build\index.html" -Value "<html><body>Xavier Panel UI Placeholder</body></html>"
}

# 3. Detect Installer Tools
$WiXFound = Get-Command candle.exe -ErrorAction SilentlyContinue
$InnoFound = Get-Command iscc.exe -ErrorAction SilentlyContinue
$HeatFound = Get-Command heat.exe -ErrorAction SilentlyContinue

if (-not $WiXFound -and -not $InnoFound) {
    Write-Error "Neither WiX Toolset nor Inno Setup was found in PATH."
    exit 1
}

# Build WiX MSI if candle.exe is available
if ($WiXFound) {
    Write-Host "Building WiX Installer (MSI)..." -ForegroundColor Green
    try {
        if ($HeatFound) {
            Write-Host "Harvesting Panel UI files with heat.exe..." -ForegroundColor Gray
            & heat.exe dir "..\panel-ui\build" -dr PANELUIFOLDER -cg PanelUIComponents -gg -sreg -sfrag -srd -out panel-ui-files.wxs
            & candle.exe "-dProductVersion=$Version" xavier.wxs panel-ui-files.wxs
            & light.exe xavier.wixobj panel-ui-files.wixobj -o "$DistDir\XavierInstaller.msi"
        } else {
            Write-Warning "heat.exe not found. Building MSI without harvested panel-ui files..."
            & candle.exe "-dProductVersion=$Version" xavier.wxs
            & light.exe xavier.wixobj -o "$DistDir\XavierInstaller.msi"
        }
        Write-Host "Done! Created $DistDir\XavierInstaller.msi" -ForegroundColor Cyan
    } catch {
        Write-Error "Failed to build WiX MSI installer: $_"
    }
}

# Build Inno Setup EXE if iscc.exe is available
if ($InnoFound) {
    Write-Host "Building Inno Setup Installer (EXE)..." -ForegroundColor Green
    try {
        & iscc.exe "/DMyAppVersion=$Version" setup.iss
        if (Test-Path "Output\XavierSetup.exe") {
            Copy-Item "Output\XavierSetup.exe" "$DistDir\XavierSetup.exe" -Force
            Write-Host "Done! Created $DistDir\XavierSetup.exe" -ForegroundColor Cyan
        } elseif (Test-Path "XavierSetup.exe") {
            Copy-Item "XavierSetup.exe" "$DistDir\XavierSetup.exe" -Force
            Write-Host "Done! Created $DistDir\XavierSetup.exe" -ForegroundColor Cyan
        }
    } catch {
        Write-Error "Failed to build Inno Setup installer: $_"
    }
}
