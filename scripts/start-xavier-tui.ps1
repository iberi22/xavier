# scripts/start-xavier-tui.ps1
# Script para arrancar Xavier con el TUI interactivo (sin necesidad de construir Tauri)

$ErrorActionPreference = "Stop"
$rootDir = (Get-Item -Path "$PSScriptRoot\..").FullName
$envFile = Join-Path $rootDir ".env"

Write-Host "=== Xavier TUI Startup ===" -ForegroundColor Cyan
Write-Host ""

# ──────────────────────────────────────────────
# 1. Cargar variables de entorno
# ──────────────────────────────────────────────
if (-not (Test-Path $envFile)) {
    Write-Host "❌ Error: Archivo .env no encontrado en $rootDir" -ForegroundColor Red
    exit 1
}

Write-Host "📋 Cargando variables de .env..." -ForegroundColor Gray
Get-Content $envFile | Where-Object { $_ -match '=' -and $_ -notmatch '^#' } | ForEach-Object {
    $line = $_.Trim()
    if ($line -match '^([^=]+)=(.*)$') {
        $name = $Matches[1].Trim()
        $value = $Matches[2].Trim()
        $value = $value -replace '^["'']|["'']$', ''
        [System.Environment]::SetEnvironmentVariable($name, $value, [System.EnvironmentVariableTarget]::Process)
    }
}

# ──────────────────────────────────────────────
# 2. Buscar binario TUI
# ──────────────────────────────────────────────
$tuiBinPath = Join-Path $rootDir "target\release\xavier-tui.exe"

if (-not (Test-Path $tuiBinPath)) {
    Write-Host "⚠️  TUI no está construido. Construyendo..." -ForegroundColor Yellow
    
    Push-Location $rootDir
    try {
        & cargo build --bin xavier-tui --release
        
        if (-not (Test-Path $tuiBinPath)) {
            Write-Host "❌ Error: La construcción del TUI falló" -ForegroundColor Red
            exit 1
        }
    } finally {
        Pop-Location
    }
}

# ──────────────────────────────────────────────
# 3. Iniciar TUI
# ──────────────────────────────────────────────
Write-Host ""
Write-Host "🎨 Iniciando Xavier TUI Dashboard..." -ForegroundColor Cyan
Write-Host "   (Presiona 'q' para salir del TUI)" -ForegroundColor Gray
Write-Host ""

& $tuiBinPath
