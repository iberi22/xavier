# scripts/stop-xavier-all.ps1
# Script para detener tanto el servidor Xavier como el Panel UI

Write-Host "=== Deteniendo Xavier System ===" -ForegroundColor Cyan
Write-Host ""

$stopped = $false

# Detener procesos de xavier.exe (servidor backend)
$xavierProcs = Get-Process xavier -ErrorAction SilentlyContinue
if ($xavierProcs) {
    Write-Host "🛑 Deteniendo servidor Xavier ($($xavierProcs.Count) proceso(s))..." -ForegroundColor Yellow
    Stop-Process -Name xavier -Force
    $stopped = $true
}

# Detener procesos del Panel UI de Tauri
# El panel de Tauri también se llama "xavier.exe" pero puede estar en diferente ubicación
# Buscaremos todos los procesos y los detendremos
$allXavierProcs = Get-Process | Where-Object { $_.ProcessName -eq "xavier" -or $_.MainWindowTitle -like "*xavier*" }
if ($allXavierProcs) {
    foreach ($proc in $allXavierProcs) {
        try {
            Write-Host "🛑 Deteniendo proceso Xavier (PID: $($proc.Id))..." -ForegroundColor Yellow
            Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
            $stopped = $true
        } catch {
            # Ignorar errores si el proceso ya se detuvo
        }
    }
}

if ($stopped) {
    Write-Host ""
    Write-Host "✅ Xavier System detenido correctamente" -ForegroundColor Green
} else {
    Write-Host "⚠️  No se encontraron procesos de Xavier corriendo" -ForegroundColor Yellow
}

Write-Host ""
