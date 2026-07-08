# scripts/start-xavier-with-ui.ps1
# Script para arrancar Xavier con el Panel UI completo (servidor + interfaz Tauri)

param(
    [switch]$DevMode = $false
)

$ErrorActionPreference = "Stop"
$rootDir = (Get-Item -Path "$PSScriptRoot\..").FullName
$envFile = Join-Path $rootDir ".env"
$logDir = Join-Path $rootDir "logs"

Write-Host "=== Xavier System Startup ===" -ForegroundColor Cyan
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
# 2. Verificar servidor backend
# ──────────────────────────────────────────────
Write-Host "🔍 Verificando servidor Xavier..." -ForegroundColor Gray

$xavierRunning = $false
$xavierPort = 8006

try {
    $response = Invoke-RestMethod -Uri "http://localhost:$xavierPort/health" -Method Get -TimeoutSec 2 -ErrorAction SilentlyContinue
    if ($response.status -eq "ok" -or $response.status -eq "healthy") {
        $xavierRunning = $true
        Write-Host "✅ Xavier servidor ya está corriendo en puerto $xavierPort (v$($response.version))" -ForegroundColor Green
    }
} catch {
    Write-Host "⚠️  Xavier servidor no está corriendo" -ForegroundColor Yellow
}

# Si no está corriendo, iniciarlo
if (-not $xavierRunning) {
    Write-Host "🚀 Iniciando Xavier servidor..." -ForegroundColor Cyan
    
    $binPath = "C:\Users\belal\bin\xavier.exe"
    if (-not (Test-Path $binPath)) {
        $binPath = Join-Path $rootDir "target\release\xavier.exe"
        if (-not (Test-Path $binPath)) {
            Write-Host "❌ Error: No se encuentra el binario de Xavier" -ForegroundColor Red
            Write-Host "   Ejecuta: cargo build --release" -ForegroundColor Yellow
            exit 1
        }
    }
    
    # Crear directorio de logs
    if (-not (Test-Path $logDir)) {
        New-Item -Path $logDir -ItemType Directory | Out-Null
    }
    
    $timestamp = Get-Date -Format "yyyy-MM-dd"
    $logFile = Join-Path $logDir "xavier-$timestamp.log"
    
    # Iniciar servidor en background
    Start-Process -FilePath $binPath -ArgumentList "http", $xavierPort -RedirectStandardOutput $logFile -RedirectStandardError $logFile -WindowStyle Hidden
    
    Write-Host "⏳ Esperando que el servidor inicie..." -ForegroundColor Gray
    Start-Sleep -Seconds 3
    
    # Verificar que inició correctamente
    try {
        $response = Invoke-RestMethod -Uri "http://localhost:$xavierPort/health" -Method Get -TimeoutSec 2
        Write-Host "✅ Xavier servidor iniciado exitosamente (v$($response.version))" -ForegroundColor Green
    } catch {
        Write-Host "❌ Error: El servidor no responde después de iniciar" -ForegroundColor Red
        Write-Host "   Revisa el log: $logFile" -ForegroundColor Yellow
        exit 1
    }
}

# ──────────────────────────────────────────────
# 3. Iniciar Panel UI (Tauri)
# ──────────────────────────────────────────────
Write-Host ""
Write-Host "🎨 Iniciando Panel UI..." -ForegroundColor Cyan

$panelDir = Join-Path $rootDir "panel-ui"

if (-not (Test-Path $panelDir)) {
    Write-Host "❌ Error: No se encuentra el directorio panel-ui" -ForegroundColor Red
    exit 1
}

Push-Location $panelDir

try {
    if ($DevMode) {
        Write-Host "🔧 Modo desarrollo - Iniciando con hot-reload..." -ForegroundColor Yellow
        Write-Host "   (Presiona Ctrl+C para detener ambos servicios)" -ForegroundColor Gray
        Write-Host ""
        
        # En modo dev, Tauri iniciará el servidor Vite automáticamente
        & pnpm tauri dev
    } else {
        # Verificar si existe el build
        $tauriExe = Join-Path $panelDir "src-tauri\target\release\xavier.exe"
        
        if (-not (Test-Path $tauriExe)) {
            Write-Host "⚠️  Panel UI no está construido. Construyendo..." -ForegroundColor Yellow
            Write-Host "   (Esto puede tomar varios minutos la primera vez)" -ForegroundColor Gray
            Write-Host ""
            
            & pnpm install
            & pnpm tauri build
            
            if (-not (Test-Path $tauriExe)) {
                Write-Host "❌ Error: La construcción del panel falló" -ForegroundColor Red
                exit 1
            }
        }
        
        Write-Host "✅ Panel UI construido. Iniciando..." -ForegroundColor Green
        
        # Iniciar el panel UI en release mode
        Start-Process -FilePath $tauriExe
        
        Start-Sleep -Seconds 2
        Write-Host "✅ Panel UI iniciado" -ForegroundColor Green
    }
    
} catch {
    Write-Host "❌ Error al iniciar el Panel UI: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
} finally {
    Pop-Location
}

Write-Host ""
Write-Host "═══════════════════════════════════════" -ForegroundColor Cyan
Write-Host "✨ Xavier System iniciado correctamente" -ForegroundColor Green
Write-Host "═══════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""
Write-Host "📍 Servidor Backend:  http://localhost:$xavierPort" -ForegroundColor White
Write-Host "🎨 Panel UI:          Iniciado (busca el icono en la bandeja del sistema)" -ForegroundColor White
Write-Host ""
Write-Host "Para detener: scripts\stop-xavier-all.ps1" -ForegroundColor Gray
Write-Host ""
