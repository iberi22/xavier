# scripts/stop-xavier.ps1
# Script para detener el servidor Xavier gracefully

Write-Host "Buscando procesos xavier.exe..." -ForegroundColor Cyan

$procs = Get-Process xavier -ErrorAction SilentlyContinue

if ($procs) {
    Write-Host "Deteniendo $($procs.Count) proceso(s) de Xavier..."
    Stop-Process -Name xavier -Force
    Write-Host "Xavier se ha detenido." -ForegroundColor Green
} else {
    Write-Host "No se encontraron procesos de Xavier en ejecucion." -ForegroundColor Yellow
}
