# Stop-All: Graceful shutdown for Xavier + Gestalt + Web Panel UI (Windows)
param(
    [switch]$Force
)

Write-Host "Shutting down SWAL Ecosystem services..." -ForegroundColor Yellow

# Find and stop processes
$stopped = 0

# 1. Stop Gestalt Bus
Get-Process -Name "gestalt_cli" -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "Stopping Gestalt CLI (PID $($_.Id))..." -ForegroundColor Gray
    Stop-Process -Id $_.Id -Force
    $stopped++
}

# 2. Stop Xavier Server
Get-Process -Name "xavier" -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "Stopping Xavier (PID $($_.Id))..." -ForegroundColor Gray
    Stop-Process -Id $_.Id -Force
    $stopped++
}

# 3. Stop Node (Vite on port 4174 if running)
$viteConnections = Get-NetTCPConnection -LocalPort 4174 -ErrorAction SilentlyContinue
foreach ($conn in $viteConnections) {
    if ($conn.OwningProcess -gt 0) {
        Write-Host "Stopping Web Panel UI process (PID $($conn.OwningProcess))..." -ForegroundColor Gray
        Stop-Process -Id $conn.OwningProcess -Force -ErrorAction SilentlyContinue
        $stopped++
    }
}

Write-Host "`nShutdown completed. Stopped $stopped processes." -ForegroundColor Green
