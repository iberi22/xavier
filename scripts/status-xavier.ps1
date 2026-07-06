# scripts/status-xavier.ps1
# Script para mostrar el estado y estadisticas basicas de Xavier

$port = 8006
$url = "http://localhost:$port"

Write-Host "--- Xavier Server Status ---" -ForegroundColor Cyan

# 1. Verificar si el proceso existe
$procs = Get-Process xavier -ErrorAction SilentlyContinue
if ($procs) {
    Write-Host "Proceso: xavier.exe esta CORRIENDO (PID: $($procs[0].Id))" -ForegroundColor Green
} else {
    Write-Host "Proceso: xavier.exe NO ESTA CORRIENDO" -ForegroundColor Red
    exit
}

# 2. Consultar Health API
try {
    # Intentar obtener el token de .env para la peticion si existe
    $rootDir = (Get-Item -Path "$PSScriptRoot\..").FullName
    $envFile = Join-Path $rootDir ".env"
    $token = $null

    if (Test-Path $envFile) {
        $tokenLine = Get-Content $envFile | Where-Object { $_ -match '^XAVIER_TOKEN=' }
        if ($tokenLine -match '^XAVIER_TOKEN=(.*)$') {
            $token = $Matches[1].Trim() -replace '^["'']|["'']$', ''
        }
    }

    $headers = @{}
    if ($token) { $headers["X-Xavier-Token"] = $token }

    $health = Invoke-RestMethod -Uri "$url/health" -Method Get -Headers $headers -TimeoutSec 2

    Write-Host "API Status: $($health.status)" -ForegroundColor Green
    Write-Host "Version:    $($health.version)"

    # 3. Mostrar estadisticas si estan disponibles
    $stats = Invoke-RestMethod -Uri "$url/memory/stats" -Method Get -Headers $headers -ErrorAction SilentlyContinue
    if ($stats) {
        Write-Host "`n--- Estadisticas de Memoria ---" -ForegroundColor Cyan
        Write-Host "Total Registros: $($stats.total_records)"
        Write-Host "Backend:         $($stats.backend)"
        Write-Host "Workspace:       $($stats.workspace_id)"
    }

} catch {
    Write-Host "API Status: NO RESPONDE en $url/health" -ForegroundColor Red
    Write-Host "Detalle: $($_.Exception.Message)"
}
