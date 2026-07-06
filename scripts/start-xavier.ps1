# scripts/start-xavier.ps1
# Script para arrancar el servidor Xavier con carga de .env y health check

$rootDir = (Get-Item -Path "$PSScriptRoot\..").FullName
$envFile = Join-Path $rootDir ".env"
$binPath = Join-Path $rootDir "target\debug\xavier.exe"
$logDir = Join-Path $rootDir "logs"

# 1. Verificar .env
if (-not (Test-Path $envFile)) {
    Write-Host "Error: Archivo .env no encontrado en la raiz del proyecto ($rootDir)." -ForegroundColor Red
    exit 1
}

# 2. Cargar variables de .env
Write-Host "Cargando variables de .env..." -ForegroundColor Cyan
Get-Content $envFile | Where-Object { $_ -match '=' -and $_ -notmatch '^#' } | ForEach-Object {
    $line = $_.Trim()
    if ($line -match '^([^=]+)=(.*)$') {
        $name = $Matches[1].Trim()
        $value = $Matches[2].Trim()
        # Quitar comillas si existen
        $value = $value -replace '^["'']|["'']$', ''
        [System.Environment]::SetEnvironmentVariable($name, $value, [System.EnvironmentVariableTarget]::Process)
    }
}

# 3. Validar variables requeridas
$requiredVars = @("XAVIER_TOKEN", "XAVIER_EMBEDDING_PROVIDER_MODE", "XAVIER_EMBEDDING_API_KEY", "XAVIER_DATA_DIR")
foreach ($var in $requiredVars) {
    if (-not [System.Environment]::GetEnvironmentVariable($var, [System.EnvironmentVariableTarget]::Process)) {
        Write-Host "Error: Variable requerida '$var' no definida en .env" -ForegroundColor Red
        exit 1
    }
}

# 4. Verificar ejecutable
if (-not (Test-Path $binPath)) {
    Write-Host "Error: $binPath no existe." -ForegroundColor Red
    Write-Host "Ejecuta: 'cargo build' primero para generar el binario de debug."
    exit 1
}

# 5. Reinicio limpio (matar procesos previos)
Write-Host "Buscando instancias previas de xavier.exe..." -ForegroundColor Cyan
$oldProcs = Get-Process xavier -ErrorAction SilentlyContinue
if ($oldProcs) {
    Write-Host "Deteniendo $($oldProcs.Count) proceso(s) existente(s)..."
    Stop-Process -Name xavier -Force
    Start-Sleep -Seconds 2
}

# 6. Preparar logs
if (-not (Test-Path $logDir)) {
    New-Item -Path $logDir -ItemType Directory | Out-Null
}

$timestamp = Get-Date -Format "yyyy-MM-dd"
$logFile = Join-Path $logDir "xavier-$timestamp.log"

# 7. Lanzar en background
Write-Host "Iniciando Xavier http 8006 en background..." -ForegroundColor Cyan
Write-Host "Logs redirigidos a: $logFile"
Start-Process -FilePath $binPath -ArgumentList "http", "8006" -RedirectStandardOutput $logFile -RedirectStandardError $logFile -WindowStyle Hidden

# 8. Esperar y Health Check
Write-Host "Esperando 4 segundos para verificacion..." -ForegroundColor Gray
Start-Sleep -Seconds 4

try {
    # Usar el token cargado para el health check si es necesario (aunque /health suele ser libre o usar el token en header)
    $token = [System.Environment]::GetEnvironmentVariable("XAVIER_TOKEN", [System.EnvironmentVariableTarget]::Process)

    $headers = @{}
    if ($token) { $headers["X-Xavier-Token"] = $token }

    $response = Invoke-RestMethod -Uri "http://localhost:8006/health" -Method Get -Headers $headers -ErrorAction Stop

    $status = $response.status
    $version = $response.version

    if ($status -eq "ok" -or $status -eq "healthy") {
        Write-Host "Xavier v$version corriendo exitosamente en :8006" -ForegroundColor Green
    } else {
        Write-Host "Xavier reporta un estado inesperado: $status" -ForegroundColor Yellow
    }
} catch {
    Write-Host "ERROR: No se pudo verificar el health check en http://localhost:8006/health" -ForegroundColor Red
    Write-Host "Detalle: $($_.Exception.Message)"
    Write-Host "Consulta el log para diagnostico: $logFile"
    exit 1
}
