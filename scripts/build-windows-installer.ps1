# Full Windows rebuild: clean optional, panel-ui, release bins, dist package, Tauri NSIS.
# Usage:
#   .\scripts\build-windows-installer.ps1
#   .\scripts\build-windows-installer.ps1 -SkipClean -SkipTauri

[CmdletBinding()]
param(
    [switch]$SkipClean,
    [switch]$SkipTauri,
    [switch]$SkipTests
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot\..").Path
Set-Location $root

function Step($m) { Write-Host "`n=== $m ===" -ForegroundColor Cyan }
function Ok($m) { Write-Host "[ok] $m" -ForegroundColor Green }

Step "Stop locking processes"
Get-Process xavier, xavier-tui -EA SilentlyContinue | Stop-Process -Force -EA SilentlyContinue
Start-Sleep -Seconds 1

if (-not $SkipClean) {
    Step "Clean old builds"
    if (Test-Path "target\debug") { Remove-Item -Recurse -Force "target\debug" -EA SilentlyContinue }
    if (Test-Path "panel-ui\build") { Remove-Item -Recurse -Force "panel-ui\build" -EA SilentlyContinue }
    Ok "cleaned debug + panel-ui/build (release kept for incremental unless cargo clean)"
}

Step "Panel UI"
Push-Location panel-ui
try {
    pnpm install
    if (-not $SkipTests) {
        pnpm test
        pnpm run typecheck
    }
    pnpm run build
} finally { Pop-Location }
Ok "panel-ui/build ready"

Step "Rust release"
cargo build --release --bin xavier --bin xavier-tui --features "cli-interactive"
Ok "target/release/xavier.exe + xavier-tui.exe"

Step "Package dist/"
$dist = Join-Path $root "dist"
New-Item -ItemType Directory -Force -Path (Join-Path $dist "panel-ui\build") | Out-Null
Copy-Item "target\release\xavier.exe" (Join-Path $dist "xavier.exe") -Force
Copy-Item "target\release\xavier.exe" (Join-Path $dist "xavier-ola-graph.exe") -Force
if (Test-Path "target\release\xavier-tui.exe") {
    Copy-Item "target\release\xavier-tui.exe" (Join-Path $dist "xavier-tui.exe") -Force
}
Copy-Item "panel-ui\build\*" (Join-Path $dist "panel-ui\build") -Recurse -Force
Ok "dist/xavier.exe + dist/panel-ui/build"

# Tauri sidecar
$triple = (rustc -Vv | Select-String "host:").ToString().Split(" ")[1]
$binDir = Join-Path $root "panel-ui\src-tauri\binaries"
New-Item -ItemType Directory -Force -Path $binDir | Out-Null
Copy-Item "target\release\xavier.exe" (Join-Path $binDir "xavier-$triple.exe") -Force
Ok "Tauri sidecar xavier-$triple.exe"

$nsisOut = $null
if (-not $SkipTauri) {
    Step "Tauri desktop installer (NSIS/MSI)"
    Push-Location panel-ui
    try {
        pnpm exec tauri build 2>&1 | Tee-Object -FilePath (Join-Path $root "feedback\tauri-build.log")
        if ($LASTEXITCODE -ne 0) { throw "tauri build failed (see feedback/tauri-build.log)" }
    } finally { Pop-Location }

    $bundle = Get-ChildItem "panel-ui\src-tauri\target\release\bundle" -Recurse -Include *.exe,*.msi -EA SilentlyContinue |
        Sort-Object LastWriteTime -Descending
    if ($bundle) {
        foreach ($b in $bundle) {
            Copy-Item $b.FullName (Join-Path $dist $b.Name) -Force
            Ok "installer artifact: dist\$($b.Name) ($([math]::Round($b.Length/1MB,1)) MB)"
        }
        $nsisOut = $bundle[0].FullName
    } else {
        Write-Warning "No Tauri bundle artifacts found"
    }
}

# Optional Inno if installed
$iscc = Get-Command iscc.exe -EA SilentlyContinue
if ($iscc) {
    Step "Inno Setup (XavierSetup.exe)"
    Push-Location installer
    try {
        & iscc.exe setup.iss
        if (Test-Path "Output\XavierSetup.exe") {
            Copy-Item "Output\XavierSetup.exe" (Join-Path $dist "XavierSetup.exe") -Force
            Ok "dist/XavierSetup.exe"
        }
    } finally { Pop-Location }
} else {
    Write-Host "Inno Setup (iscc) not in PATH — using Tauri NSIS + dist portable package" -ForegroundColor Yellow
}

Step "Summary"
Get-ChildItem $dist -File | Format-Table Name, @{n='MB';e={[math]::Round($_.Length/1MB,2)}}, LastWriteTime -AutoSize
Write-Host "Portable install:" -ForegroundColor Cyan
Write-Host "  cd dist; .\xavier-windows-setup.ps1 -StartNow" -ForegroundColor White
if ($nsisOut) {
    Write-Host "Desktop installer: $nsisOut" -ForegroundColor White
}
Ok "done"
