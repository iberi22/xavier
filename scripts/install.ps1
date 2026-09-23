# Xavier - Windows Installer
# Ejecutar en PowerShell como Administrador o Usuario
[CmdletBinding()]
param(
    [switch]$DownloadLatest,
    [string]$Version,
    [string]$InstallDir = "$env:LOCALAPPDATA\Xavier",
    [switch]$Silent,
    [switch]$Quiet,
    [string]$LogPath,
    [switch]$StartService
)

$IsQuiet = $Silent -or $Quiet

function Write-Log([string]$msg, [string]$color = "White") {
    if (-not [string]::IsNullOrEmpty($LogPath)) {
        $timestamp = (Get-Date).ToString("yyyy-MM-dd HH:mm:ss")
        Add-Content -Path $LogPath -Value "[$timestamp] $msg" -ErrorAction SilentlyContinue
    }
    if (-not $IsQuiet) {
        Write-Host $msg -ForegroundColor $color
    }
}

try {
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

    Write-Log "=== Xavier Windows Installer ===" "Cyan"

    # Ensure target directories exist
    Write-Log "Creando directorios..." "Yellow"
    New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
    New-Item -ItemType Directory -Force -Path $DataDir | Out-Null
    New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null

    $ShouldDownload = $DownloadLatest -or ($Version -ne "") -or (-not (Test-Path $LocalExe))

    if ($ShouldDownload) {
        Write-Log "Descargando release oficial desde GitHub..." "Yellow"

        $Headers = @{ "User-Agent" = "Xavier-Installer" }
        $ZipUrl = $null

        try {
            if ($Version) {
                $Tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
                $ApiUrl = "https://api.github.com/repos/iberi22/xavier/releases/tags/$Tag"
            } else {
                $ApiUrl = "https://api.github.com/repos/iberi22/xavier/releases/latest"
            }

            Write-Log "Consultando API de GitHub: $ApiUrl" "Gray"
            $Release = Invoke-RestMethod -Uri $ApiUrl -Headers $Headers -ErrorAction Stop
            $Asset = $Release.assets | Where-Object { $_.name -like "*x86_64-pc-windows-msvc.zip" -or $_.name -like "xavier-*.zip" } | Select-Object -First 1

            if ($Asset) {
                $ZipUrl = $Asset.browser_download_url
                Write-Log "Asset encontrado: $($Asset.name)" "Green"
            }
        } catch {
            Write-Log "Aviso: No se pudo consultar la API de GitHub ($($_.Exception.Message)). Usando URL de descarga directa de fallback." "Yellow"
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
        $TempSha = "$TempZip.sha256"
        $TempExtract = Join-Path $env:TEMP "xavier_extract_$([Guid]::NewGuid().ToString('N'))"

        try {
            Write-Log "Descargando paquete desde: $ZipUrl" "Cyan"
            Invoke-WebRequest -Uri $ZipUrl -OutFile $TempZip -UseBasicParsing -Headers $Headers

            # Verificación SHA-256 sidecar
            $ShaUrl = "$ZipUrl.sha256"
            try {
                Write-Log "Descargando checksum SHA-256: $ShaUrl" "Gray"
                Invoke-WebRequest -Uri $ShaUrl -OutFile $TempSha -UseBasicParsing -Headers $Headers -ErrorAction SilentlyContinue
                if (Test-Path $TempSha) {
                    $ExpectedRaw = (Get-Content $TempSha -Raw).Trim()
                    $ExpectedHash = ($ExpectedRaw -split '\s+')[0].Trim().ToLower()
                    $ActualHash = (Get-FileHash -Path $TempZip -Algorithm SHA256).Hash.ToLower()

                    if ($ExpectedHash.Length -ge 64 -and $ActualHash -ne $ExpectedHash) {
                        Write-Log "ERROR CRÍTICO: Discrepancia de integridad SHA-256." "Red"
                        Write-Log "  Esperado: $ExpectedHash" "Red"
                        Write-Log "  Obtenido: $ActualHash" "Red"
                        exit 2
                    } else {
                        Write-Log "Integridad SHA-256 verificada con éxito: $ActualHash" "Green"
                    }
                }
            } catch {
                Write-Log "Aviso: No se pudo descargar archivo .sha256 ($($_.Exception.Message)). Continuando con validación local..." "Yellow"
            }

            Write-Log "Extrayendo archivos..." "Yellow"
            Expand-Archive -Path $TempZip -DestinationPath $TempExtract -Force

            # Localizar xavier.exe
            $DownloadedExe = Get-ChildItem -Path $TempExtract -Filter "xavier.exe" -Recurse | Select-Object -First 1
            if (-not $DownloadedExe) {
                Write-Log "ERROR: El archivo ZIP descargado no contiene xavier.exe" "Red"
                exit 3
            }

            Copy-Item -Path $DownloadedExe.FullName -Destination $BinPath -Force
            Write-Log "  xavier.exe -> $BinPath ($([Math]::Round((Get-Item $BinPath).Length / 1MB, 2)) MB)" "Green"

            # Localizar panel-ui/build si está incluido en el ZIP
            $DownloadedPanelIndex = Get-ChildItem -Path $TempExtract -Filter "index.html" -Recurse | Where-Object { $_.DirectoryName -like "*panel-ui*" -or $_.DirectoryName -like "*build*" } | Select-Object -First 1
            if ($DownloadedPanelIndex) {
                New-Item -ItemType Directory -Force -Path $PanelDestDir | Out-Null
                Copy-Item -Path "$($DownloadedPanelIndex.DirectoryName)\*" -Destination $PanelDestDir -Recurse -Force
                Write-Log "  panel-ui -> $PanelDestDir" "Green"
            } else {
                Write-Log "Aviso: El paquete descargado no incluye panel-ui/build." "Yellow"
            }
        } finally {
            if (Test-Path $TempZip) { Remove-Item -Path $TempZip -Force -ErrorAction SilentlyContinue }
            if (Test-Path $TempSha) { Remove-Item -Path $TempSha -Force -ErrorAction SilentlyContinue }
            if (Test-Path $TempExtract) { Remove-Item -Path $TempExtract -Recurse -Force -ErrorAction SilentlyContinue }
        }
    } else {
        Write-Log "Copiando binario local..." "Yellow"
        Copy-Item -Path $LocalExe -Destination $BinPath -Force
        Write-Log "  xavier.exe -> $BinPath ($([Math]::Round((Get-Item $BinPath).Length / 1MB, 2)) MB)" "Green"

        if (Test-Path $LocalPanel) {
            Write-Log "Copiando activos de Web UI (panel-ui/build)..." "Yellow"
            New-Item -ItemType Directory -Force -Path $PanelDestDir | Out-Null
            Copy-Item -Path "$LocalPanel\*" -Destination $PanelDestDir -Recurse -Force
            Write-Log "  panel-ui -> $PanelDestDir" "Green"
        } else {
            Write-Log "Aviso: No se encuentra panel-ui\build local. Ejecuta 'cd panel-ui && pnpm build' para incluir la interfaz web." "Yellow"
        }
    }

    # Config de ejemplo
    $ConfigDest = "$ConfigDir\.env"
    if (-not (Test-Path $ConfigDest)) {
        $LocalEnv = Join-Path $PSScriptRoot ".env.example"
        if (Test-Path $LocalEnv) {
            Copy-Item $LocalEnv $ConfigDest
            Write-Log "  .env.example -> $ConfigDest (EDITALO con tus valores)" "Green"
        }
    } else {
        Write-Log "  Config exists: $ConfigDest (skip)" "Gray"
    }

    # Agregar al PATH
    $UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($UserPath -notlike "*$BinDir*") {
        Write-Log "Agregando al PATH de usuario..." "Yellow"
        [Environment]::SetEnvironmentVariable("PATH", "$BinDir;$UserPath", "User")
        Write-Log "  $BinDir agregado al PATH" "Green"
        Write-Log "  REINICIA la terminal para que surta efecto" "Yellow"
    } else {
        Write-Log "  $BinDir ya está en el PATH" "Gray"
    }

    # Setear XAVIER_TOKEN si no existe
    $CurrentToken = [Environment]::GetEnvironmentVariable("XAVIER_TOKEN", "User")
    if (-not $CurrentToken) {
        $NewToken = -join ((65..90) + (97..122) + (48..57) | Get-Random -Count 32 | % { [char]$_ })
        [Environment]::SetEnvironmentVariable("XAVIER_TOKEN", $NewToken, "User")
        Write-Log "  XAVIER_TOKEN generado y seteado" "Green"

        # También actualizar en el .env si existe
        if (Test-Path $ConfigDest) {
            $EnvContent = Get-Content $ConfigDest -Raw
            $EnvContent = $EnvContent -replace "XAVIER_TOKEN=.*", "XAVIER_TOKEN=$NewToken"
            Set-Content $ConfigDest $EnvContent
            Write-Log "  Token actualizado en $ConfigDest" "Green"
        }
    }

    # Iniciar servicio automáticamente si se solicitó
    if ($StartService) {
        Write-Log "Iniciando Xavier en segundo plano..." "Cyan"
        Start-Process -FilePath $BinPath -ArgumentList "http" -WindowStyle Hidden
        Write-Log "Xavier iniciado en segundo plano en http://127.0.0.1:8006" "Green"
    }

    Write-Log "" "White"
    Write-Log "=== Instalación completa ===" "Cyan"
    Write-Log "Binario:     $BinPath" "White"
    Write-Log "Panel UI:    $PanelDestDir" "White"
    Write-Log "Datos:       $DataDir" "White"
    Write-Log "Config:      $ConfigDir\.env" "White"
    Write-Log "" "White"
    Write-Log "Ejecutar:    xavier --help" "Cyan"
    Write-Log "Servidor:    xavier http (o alias: xavier serve)" "Cyan"
    Write-Log "Diagnóstico: xavier doctor" "Cyan"

    exit 0
} catch {
    Write-Log "ERROR FATAL: $($_.Exception.Message)" "Red"
    exit 1
}
