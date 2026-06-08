# Xavier Build Script for Tauri Sidecar (PowerShell)
$ErrorActionPreference = "Stop"

Write-Host "Building Xavier backend..."
cargo build --release --bin xavier

# Get the target triple
$TargetTriple = (rustc -Vv | Select-String "host:").ToString().Split(" ")[1]
Write-Host "Detected target triple: $TargetTriple"

# Create binaries directory if it doesn't exist
$BinDir = "panel-ui/src-tauri/binaries"
if (!(Test-Path $BinDir)) {
    New-Item -ItemType Directory -Path $BinDir
}

# Copy and rename binary
$BinaryPath = "target/release/xavier.exe"
if (!(Test-Path $BinaryPath)) {
    $BinaryPath = "target/release/xavier"
}

$DestPath = "$BinDir/xavier-$TargetTriple"
if ($BinaryPath.EndsWith(".exe")) {
    $DestPath += ".exe"
}

Copy-Item $BinaryPath $DestPath
Write-Host "Binary copied to $DestPath"

# Build Tauri App
Write-Host "Building Tauri App..."
Set-Location panel-ui
pnpm install
pnpm tauri build
