# Xavier - Windows Installer
# Ejecutar en PowerShell como Administrador o Usuario
[CmdletBinding()]
param(
    [switch]$DownloadLatest,
    [string]$Version,
    [string]$InstallDir = "$env:LOCALAPPDATA\Xavier"
)

$ErrorActionPreference = "Stop"

# Fuerza TLS 1.2 para compatibilidad con GitHub API en PowerShell 5.1 / 7+
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$XavierDir = $InstallDir
$BinDir = "$XavierDir\bin"
$BinPath = "$BinDir\xavier.exe"
$DataDir = "$XavierDir\data"
$ConfigDir = "$XavierDir\config"
$PanelDestDir = "$XavierDir\panel-ui\build"

$LocalExe = Join-Path $PSScriptRoot "target\release\xavier.exe"
$LocalPanel = Join-Path $PSScriptRoot "panel-ui\build"

Write-Host "=== Xavier Windows Installer ===" -ForegroundColor Cyan

# Ensure target directories exist
Write-Host "Creando directorios..." -ForegroundColor Yellow
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
New-Item -ItemType Directory -Force -Path $DataDir | Out-Null
New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null

$ShouldDownload = $DownloadLatest -or ($Version -ne "") -or (-not (Test-Path $LocalExe))

if ($ShouldDownload) {
    Write-Host "Binario local no disponible o descarga explícita solicitada. Descargando desde GitHub Releases..." -ForegroundColor Yellow

    $Headers = @{ "User-Agent" = "Xavier-Installer" }
    $ZipUrl = $null

    try {
        if ($Version) {
            $Tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
            $ApiUrl = "https://api.github.com/repos/iberi22/xavier/releases/tags/$Tag"
        } else {
            $ApiUrl = "https://api.github.com/repos/iberi22/xavier/releases/latest"
        }

        Write-Host "Consultando API de GitHub: $ApiUrl" -ForegroundColor Gray
        $Release = Invoke-RestMethod -Uri $ApiUrl -Headers $Headers -ErrorAction Stop
        $Asset = $Release.assets | Where-Object { $_.name -like "*x86_64-pc-windows-msvc.zip" -or $_.name -like "xavier-*.zip" } | Select-Object -First 1

        if ($Asset) {
            $ZipUrl = $Asset.browser_download_url
            Write-Host "Asset encontrado: $($Asset.name)" -ForegroundColor Green
        }
    } catch {
        Write-Host "Aviso: No se pudo consultar la API de GitHub ($($_.Exception.Message)). Usando URL de descarga directa de fallback." -ForegroundColor Yellow
    }

    if (-not $ZipUrl) {
        if ($Version) {
            $Tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
            $ZipUrl = "https://github.com/iberi22/xavier/releases/download/$Tag/xavier-$Tag-x86_64-pc-windows-msvc.zip"
        } else {
            $ZipUrl = "https://github.com/iberi22/xavier/releases/latest/download/xavier-x86_64-pc-windows-msvc.zip"
        }
    }

    $TempZip = Join-Path $env:TEMP "xavier_release_$([Guid]::NewGuid().ToString('N')).zip"
    $TempExtract = Join-Path $env:TEMP "xavier_extract_$([Guid]::NewGuid().ToString('N'))"

    try {
        Write-Host "Descargando paquete desde: $ZipUrl" -ForegroundColor Cyan
        Invoke-WebRequest -Uri $ZipUrl -OutFile $TempZip -UseBasicParsing -Headers $Headers

        Write-Host "Extrayendo archivos..." -ForegroundColor Yellow
        Expand-Archive -Path $TempZip -DestinationPath $TempExtract -Force

        # Localizar xavier.exe
        $DownloadedExe = Get-ChildItem -Path $TempExtract -Filter "xavier.exe" -Recurse | Select-Object -First 1
        if (-not $DownloadedExe) {
            throw "El archivo ZIP descargado no contiene xavier.exe"
        }

        Copy-Item -Path $DownloadedExe.FullName -Destination $BinPath -Force
        Write-Host "  xavier.exe -> $BinPath ($((Get-Item $BinPath).Length / 1MB) MB)" -ForegroundColor Green

        # Localizar panel-ui/build si está incluido en el ZIP
        $DownloadedPanelIndex = Get-ChildItem -Path $TempExtract -Filter "index.html" -Recurse | Where-Object { $_.DirectoryName -like "*panel-ui*" -or $_.DirectoryName -like "*build*" } | Select-Object -First 1
        if ($DownloadedPanelIndex) {
            New-Item -ItemType Directory -Force -Path $PanelDestDir | Out-Null
            Copy-Item -Path "$($DownloadedPanelIndex.DirectoryName)\*" -Destination $PanelDestDir -Recurse -Force
            Write-Host "  panel-ui -> $PanelDestDir" -ForegroundColor Green
        } else {
            Write-Host "Aviso: El paquete descargado no incluye panel-ui/build." -ForegroundColor Yellow
        }
    } finally {
        if (Test-Path $TempZip) { Remove-Item -Path $TempZip -Force -ErrorAction SilentlyContinue }
        if (Test-Path $TempExtract) { Remove-Item -Path $TempExtract -Recurse -Force -ErrorAction SilentlyContinue }
    }
} else {
    Write-Host "Copiando binario local..." -ForegroundColor Yellow
    Copy-Item -Path $LocalExe -Destination $BinPath -Force
    Write-Host "  xavier.exe -> $BinPath ($((Get-Item $BinPath).Length / 1MB) MB)" -ForegroundColor Green

    if (Test-Path $LocalPanel) {
        Write-Host "Copiando activos de Web UI (panel-ui/build)..." -ForegroundColor Yellow
        New-Item -ItemType Directory -Force -Path $PanelDestDir | Out-Null
        Copy-Item -Path "$LocalPanel\*" -Destination $PanelDestDir -Recurse -Force
        Write-Host "  panel-ui -> $PanelDestDir" -ForegroundColor Green
    } else {
        Write-Host "Aviso: No se encuentra panel-ui\build local. Ejecuta 'cd panel-ui && pnpm build' para incluir la interfaz web." -ForegroundColor Yellow
    }
}

# Config de ejemplo
$ConfigDest = "$ConfigDir\.env"
if (-not (Test-Path $ConfigDest)) {
    $LocalEnv = Join-Path $PSScriptRoot ".env.example"
    if (Test-Path $LocalEnv) {
        Copy-Item $LocalEnv $ConfigDest
        Write-Host "  .env.example -> $ConfigDest (EDITALO con tus valores)" -ForegroundColor Green
    }
} else {
    Write-Host "  Config exists: $ConfigDest (skip)" -ForegroundColor Gray
}

# Agregar al PATH
$UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($UserPath -notlike "*$BinDir*") {
    Write-Host "Agregando al PATH de usuario..." -ForegroundColor Yellow
    [Environment]::SetEnvironmentVariable("PATH", "$BinDir;$UserPath", "User")
    Write-Host "  $BinDir agregado al PATH" -ForegroundColor Green
    Write-Host "  REINICIA la terminal para que surta efecto" -ForegroundColor Yellow
} else {
    Write-Host "  $BinDir ya está en el PATH" -ForegroundColor Gray
}

# Setear XAVIER_TOKEN si no existe
$CurrentToken = [Environment]::GetEnvironmentVariable("XAVIER_TOKEN", "User")
if (-not $CurrentToken) {
    $NewToken = -join ((65..90) + (97..122) + (48..57) | Get-Random -Count 32 | % { [char]$_ })
    [Environment]::SetEnvironmentVariable("XAVIER_TOKEN", $NewToken, "User")
    Write-Host "  XAVIER_TOKEN generado y seteado" -ForegroundColor Green

    # También actualizar en el .env si existe
    if (Test-Path $ConfigDest) {
        $EnvContent = Get-Content $ConfigDest -Raw
        $EnvContent = $EnvContent -replace "XAVIER_TOKEN=.*", "XAVIER_TOKEN=$NewToken"
        Set-Content $ConfigDest $EnvContent
        Write-Host "  Token actualizado en $ConfigDest" -ForegroundColor Green
    }
}

Write-Host ""
Write-Host "=== Instalación completa ===" -ForegroundColor Cyan
Write-Host "Binario:     $BinPath" -ForegroundColor White
Write-Host "Panel UI:    $PanelDestDir" -ForegroundColor White
Write-Host "Datos:       $DataDir" -ForegroundColor White
Write-Host "Config:      $ConfigDir\.env" -ForegroundColor White
Write-Host ""
Write-Host "Ejecutar: xavier --help" -ForegroundColor Cyan
Write-Host "Servidor: xavier serve" -ForegroundColor Cyan
Write-Host "Modo dev:  xavier monitor" -ForegroundColor Cyan
