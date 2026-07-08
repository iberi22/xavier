# fix-windows-installation.ps1
# Script para corregir una instalación de Xavier que está corriendo en modo servidor
# y migrarla para que use el Panel UI con icono en bandeja del sistema

$ErrorActionPreference = "Stop"
$rootDir = (Get-Item -Path "$PSScriptRoot\..").FullName

Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  Xavier Installation Fix Tool" -ForegroundColor Cyan
Write-Host "  Migrando de modo servidor a Panel UI completo" -ForegroundColor Cyan
Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""

# ─────────────────────────────────────────────
# 1. Detectar instalación actual
# ─────────────────────────────────────────────
Write-Host "[1/5] Detectando instalación actual..." -ForegroundColor Yellow

$xavierProcs = Get-Process xavier -ErrorAction SilentlyContinue
if ($xavierProcs) {
    Write-Host "  ✅ Xavier está corriendo (PID: $($xavierProcs[0].Id))" -ForegroundColor Green
    $xavierPath = $xavierProcs[0].Path
    Write-Host "  📍 Ubicación: $xavierPath" -ForegroundColor Gray
    
    # Detectar si es el Panel UI o el servidor
    if ($xavierPath -like "*panel-ui*" -or $xavierPath -like "*xavier-panel*") {
        Write-Host ""
        Write-Host "  ✅ Ya estás usando el Panel UI. No se requiere migración." -ForegroundColor Green
        Write-Host "  💡 El icono debería estar en la bandeja del sistema." -ForegroundColor Gray
        exit 0
    } else {
        Write-Host "  ⚠️  Estás usando el servidor en modo CLI (sin UI)" -ForegroundColor Yellow
    }
} else {
    Write-Host "  ℹ️  Xavier no está corriendo actualmente" -ForegroundColor Gray
}

# ─────────────────────────────────────────────
# 2. Verificar si Panel UI está construido
# ─────────────────────────────────────────────
Write-Host ""
Write-Host "[2/5] Verificando Panel UI..." -ForegroundColor Yellow

$tauriExe = Join-Path $rootDir "target\release\app.exe"

if (-not (Test-Path $tauriExe)) {
    Write-Host "  ⚠️  Panel UI no está construido" -ForegroundColor Yellow
    Write-Host ""
    $buildNow = Read-Host "  ¿Deseas construir el Panel UI ahora? (s/n)"
    
    if ($buildNow -eq 's' -or $buildNow -eq 'S' -or $buildNow -eq 'y' -or $buildNow -eq 'Y') {
        Write-Host ""
        Write-Host "  Construyendo Panel UI..." -ForegroundColor Gray
        Write-Host "  (Esto tomará 5-15 minutos la primera vez)" -ForegroundColor Gray
        Write-Host ""
        
        Push-Location (Join-Path $rootDir "panel-ui")
        try {
            # Verificar pnpm
            $pnpmExists = Get-Command pnpm -ErrorAction SilentlyContinue
            if (-not $pnpmExists) {
                Write-Host "  ❌ pnpm no está instalado" -ForegroundColor Red
                Write-Host "  Instala pnpm con: npm install -g pnpm" -ForegroundColor Yellow
                exit 1
            }
            
            # Instalar dependencias si es necesario
            if (-not (Test-Path "node_modules")) {
                Write-Host "  Instalando dependencias de Node..." -ForegroundColor Gray
                & pnpm install
            }
            
            # Construir con Tauri
            Write-Host "  Construyendo aplicación Tauri..." -ForegroundColor Gray
            & pnpm tauri build
            
            if (-not (Test-Path "..\target\release\app.exe")) {
                Write-Host ""
                Write-Host "  ❌ La construcción falló" -ForegroundColor Red
                Write-Host "  Revisa los errores arriba y reintenta." -ForegroundColor Yellow
                exit 1
            }
            
            Write-Host ""
            Write-Host "  ✅ Panel UI construido exitosamente" -ForegroundColor Green
            
        } finally {
            Pop-Location
        }
    } else {
        Write-Host ""
        Write-Host "  ℹ️  No se puede continuar sin el Panel UI" -ForegroundColor Gray
        Write-Host "  Ejecuta manualmente:" -ForegroundColor Yellow
        Write-Host "    cd panel-ui" -ForegroundColor Gray
        Write-Host "    pnpm install" -ForegroundColor Gray
        Write-Host "    pnpm tauri build" -ForegroundColor Gray
        exit 0
    }
} else {
    Write-Host "  ✅ Panel UI encontrado" -ForegroundColor Green
}

# ─────────────────────────────────────────────
# 3. Detener servidor actual si está corriendo
# ─────────────────────────────────────────────
Write-Host ""
Write-Host "[3/5] Deteniendo servidor actual..." -ForegroundColor Yellow

if ($xavierProcs) {
    Write-Host "  Deteniendo Xavier (PID: $($xavierProcs[0].Id))..." -ForegroundColor Gray
    Stop-Process -Name xavier -Force
    Start-Sleep -Seconds 2
    Write-Host "  ✅ Servidor detenido" -ForegroundColor Green
} else {
    Write-Host "  ℹ️  No hay servidor corriendo" -ForegroundColor Gray
}

# ─────────────────────────────────────────────
# 4. Copiar Panel UI al directorio de instalación
# ─────────────────────────────────────────────
Write-Host ""
Write-Host "[4/5] Actualizando instalación..." -ForegroundColor Yellow

$installDir = "C:\Users\belal\bin"
if (-not (Test-Path $installDir)) {
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
}

Write-Host "  Copiando xavier-panel.exe a $installDir..." -ForegroundColor Gray
Copy-Item $tauriExe "$installDir\xavier-panel.exe" -Force

Write-Host "  ✅ Panel UI instalado" -ForegroundColor Green

# ─────────────────────────────────────────────
# 5. Crear acceso directo en Startup (opcional)
# ─────────────────────────────────────────────
Write-Host ""
Write-Host "[5/5] Configuración de inicio automático..." -ForegroundColor Yellow

$createStartup = Read-Host "  ¿Deseas que Xavier inicie automáticamente con Windows? (s/n)"

if ($createStartup -eq 's' -or $createStartup -eq 'S' -or $createStartup -eq 'y' -or $createStartup -eq 'Y') {
    $startupFolder = [Environment]::GetFolderPath('Startup')
    $shortcutPath = Join-Path $startupFolder "Xavier.lnk"
    
    $WScriptShell = New-Object -ComObject WScript.Shell
    $Shortcut = $WScriptShell.CreateShortcut($shortcutPath)
    $Shortcut.TargetPath = "$installDir\xavier-panel.exe"
    $Shortcut.WorkingDirectory = $installDir
    $Shortcut.Description = "Xavier Memory Runtime"
    $Shortcut.Save()
    
    Write-Host "  ✅ Acceso directo creado en Startup" -ForegroundColor Green
} else {
    Write-Host "  ℹ️  Inicio automático omitido" -ForegroundColor Gray
}

# ─────────────────────────────────────────────
# 6. Iniciar Panel UI
# ─────────────────────────────────────────────
Write-Host ""
Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  ✅ Migración completada exitosamente" -ForegroundColor Green
Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""

$startNow = Read-Host "¿Deseas iniciar Xavier Panel UI ahora? (s/n)"

if ($startNow -eq 's' -or $startNow -eq 'S' -or $startNow -eq 'y' -or $startNow -eq 'Y') {
    Write-Host ""
    Write-Host "🚀 Iniciando Xavier Panel UI..." -ForegroundColor Cyan
    Start-Process "$installDir\xavier-panel.exe"
    
    Start-Sleep -Seconds 3
    
    Write-Host ""
    Write-Host "✨ Xavier está corriendo" -ForegroundColor Green
    Write-Host ""
    Write-Host "  🔍 Busca el icono de Xavier en la bandeja del sistema" -ForegroundColor White
    Write-Host "     (esquina inferior derecha, junto al reloj)" -ForegroundColor Gray
    Write-Host ""
} else {
    Write-Host ""
    Write-Host "  Para iniciar Xavier manualmente:" -ForegroundColor Yellow
    Write-Host "    $installDir\xavier-panel.exe" -ForegroundColor Gray
    Write-Host ""
    Write-Host "  O busca 'Xavier' en el menú inicio" -ForegroundColor Gray
    Write-Host ""
}
